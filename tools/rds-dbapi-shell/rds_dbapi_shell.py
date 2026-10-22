#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
rds-dbapi-shell：rds-driver/1 Python 通用 DB-API 2.0 驱动壳。

v1 内置两种驱动适配（DB-API 2.0 统一接口，execute 全库共用）：
- sqlite：标准库 sqlite3（无外部依赖，用作插件链路验证载体）
- postgresql：psycopg2 / psycopg3（import 失败给可读报错与 pip 安装提示；
  驱动由用户环境提供，壳不内嵌）

与 tools/rds-jdbc-shell 完全同构：
- 进程形态：sidecar（spawn → initialize → session → meta/query → shutdown）
- 分帧协议 / 方法面 / 错误码与宿主 Rust 实现对齐（见 crates/plugin/src/sidecar/）
- 不内嵌 Python 环境：解释器由宿主 runtime.rs 三级查找（用户指定 → PATH → third_party/python）

线程模型（同 Java 壳）：
- 主线程只读帧；query.cancel 即时处理（sqlite3 Connection.interrupt() /
  psycopg Connection.cancel() 线程安全、可打断另一线程正在执行的查询）；
  其余请求入队给 worker 顺序执行（保持 RPC 顺序语义）。
- 响应写回加锁（worker 与主线程并发写 stdout）。

连接配置（session.open 的 params.params）：
- url_override 优先 → url_template 模板替换 → driver 兜底拼 URL
  （sqlite 用 file_path / :memory:；postgresql 用 postgresql://host:port/db）

内联结果 v1：上限 200 行 / 256 KiB，超出截断 + truncated + notice（Arrow 第二期）。
"""

import json
import os
import queue
import struct
import sys
import threading

PROTOCOL_VERSION = 1
KIND_JSON = 0x01
MAX_FRAME = 64 * 1024 * 1024
INLINE_MAX_ROWS = 200
INLINE_MAX_BYTES = 256 * 1024

# 错误码（对齐 RpcErrorCode -32001…-32009）
ERR_DRIVER_NOT_SUPPORTED = -32001
ERR_SESSION_NOT_FOUND = -32002
ERR_SQL_ERROR = -32003
ERR_CANCELLED = -32004
ERR_TIMEOUT = -32005
ERR_CAPABILITY_DENIED = -32006
ERR_PROTOCOL_MISMATCH = -32007
ERR_RESOURCE_LIMIT = -32008
ERR_CONNECT_FAILED = -32009


class RpcError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


class FrameIO:
    """二进制分帧（对齐 sidecar/proto.rs）：[u32 BE total_len][u8 kind][payload]。"""

    def __init__(self):
        self._r = sys.stdin.buffer
        self._w = sys.stdout.buffer
        self._lock = threading.Lock()

    def read(self):
        header = self._read_exact(5)
        if header is None:
            return None
        total = struct.unpack(">I", header[:4])[0]
        kind = header[4]
        if total < 5 or total > MAX_FRAME:
            raise IOError(f"帧长非法：{total}")
        if kind != KIND_JSON:
            raise IOError(f"未知帧种类：{kind}")
        payload = self._read_exact(total - 5)
        if payload is None:
            raise IOError("载荷读取中断")
        return payload.decode("utf-8")

    def write(self, text):
        payload = text.encode("utf-8")
        total = 5 + len(payload)
        with self._lock:
            self._w.write(struct.pack(">I", total))
            self._w.write(bytes([KIND_JSON]))
            self._w.write(payload)
            self._w.flush()

    def _read_exact(self, n):
        buf = bytearray()
        while len(buf) < n:
            chunk = self._r.read(n - len(buf))
            if not chunk:
                return None if len(buf) == 0 else buf
            buf.extend(chunk)
        return bytes(buf)


class Session:
    """一个 DB-API 会话：连接 + 执行 + meta（按驱动适配，见 _connect_*）。"""

    def __init__(self, session_id, driver_id, config):
        self.id = session_id
        self.driver_id = driver_id
        # 适配器按「引擎驱动 id 优先、会话 driver_id 兜底」选择（引擎传的才是权威）
        self.kind = _normalize_driver(config.get("driver") or driver_id)
        self.conn = self._connect(config)
        self._cancelled = set()  # 已请求取消的 request_id（execute 异常时区分取消与 SQL 错）
        self._server_version = self._server_version_of()

    # ---- 连接 ----

    def _connect(self, cfg):
        if self.kind == "sqlite":
            return self._connect_sqlite(cfg)
        if self.kind == "postgresql":
            return self._connect_postgresql(cfg)
        raise RpcError(ERR_DRIVER_NOT_SUPPORTED,
                       f"壳不支持的驱动类型：{self.kind}（支持 sqlite / postgresql）")

    def _connect_sqlite(self, cfg):
        import sqlite3
        database = self._resolve_sqlite_path(cfg)
        try:
            return sqlite3.connect(database, check_same_thread=False)
        except sqlite3.Error as e:
            raise RpcError(ERR_CONNECT_FAILED, f"连接失败 {database}：{e}")

    def _connect_postgresql(self, cfg):
        url = self._resolve_pg_url(cfg)
        try:
            import psycopg2  # 用户环境提供；壳不内嵌驱动
        except ImportError:
            try:
                import psycopg  # psycopg 3（binary 版与 2 的 connect 同形）
                # psycopg3 的 connect 走模块级 connect；这里统一交给下方 connect
                conn = psycopg.connect(url)
                # psycopg3 是 autocommit=False，显式标一下（与 psycopg2 默认一致）
                return conn
            except ImportError:
                raise RpcError(
                    ERR_CONNECT_FAILED,
                    "未安装 PostgreSQL 驱动：pip install psycopg2-binary（或 psycopg[binary]）。"
                    "解释器路径可在插件设置里指定（含虚拟环境）。")
        try:
            return psycopg2.connect(url)
        except Exception as e:
            raise RpcError(ERR_CONNECT_FAILED, f"连接失败 {url}：{e}")

    def _resolve_sqlite_path(self, cfg):
        override = cfg.get("url_override")
        if override:
            return override
        template = cfg.get("url_template")
        if template:
            return self._fill_template(template, cfg)
        if cfg.get("driver") == "sqlite":
            return cfg.get("file_path") or ":memory:"
        if cfg.get("file_path"):
            return cfg["file_path"]
        raise RpcError(ERR_CONNECT_FAILED,
                       "无法确定数据库路径：url_override / url_template / file_path 都没有")

    def _resolve_pg_url(self, cfg):
        override = cfg.get("url_override")
        if override:
            return override
        template = cfg.get("url_template")
        if template:
            return self._fill_template(template, cfg)
        host = cfg.get("host") or "localhost"
        port = cfg.get("port") or "5432"
        database = cfg.get("database") or "postgres"
        user = cfg.get("username") or "postgres"
        password = cfg.get("password") or ""
        auth = f"{user}:{password}@" if password else f"{user}@"
        return f"postgresql://{auth}{host}:{port}/{database}"

    @staticmethod
    def _fill_template(template, cfg):
        return (template.replace("{file}", cfg.get("file_path") or "")
                        .replace("{host}", cfg.get("host") or "")
                        .replace("{port}", cfg.get("port") or "")
                        .replace("{database}", cfg.get("database") or "")
                        .replace("{user}", cfg.get("username") or "")
                        .replace("{password}", cfg.get("password") or "")
                        .replace("{file_path}", cfg.get("file_path") or ""))

    def _server_version_of(self):
        if self.kind == "sqlite":
            import sqlite3
            return sqlite3.sqlite_version
        if self.kind == "postgresql":
            try:
                return self.conn.server_version  # psycopg2 / psycopg3 都是 int
            except Exception:
                return "postgresql"
        return self.kind

    def describe(self):
        caps = {
            "schemas": True,
            "views": True,
            "transactions": True,
            "cancel": True,
            "cursor": False,
            "arrow": False,  # v1 未接 Arrow 附件
        }
        return {
            "id": self.driver_id,
            "display_name": self.driver_id,
            "server_version": self.server_version,
            "capabilities": caps,
            "default_schema": "main" if self.kind == "sqlite" else "public",
        }

    @property
    def server_version(self):
        return self._server_version

    def cancel(self, request_id):
        # sqlite3 的 interrupt() / psycopg 的 cancel() 都是线程安全的：
        # 打断另一线程正在执行的查询（按驱动分叉，方法名不同）
        self._cancelled.add(request_id)
        try:
            if self.kind == "sqlite":
                self.conn.interrupt()
            else:
                self.conn.cancel()
        except Exception:
            # 有的驱动版本没有 cancel（如未连接状态）；取消标记已记，execute 侧兜底
            pass

    def execute(self, request_id, sql, params, max_rows, timeout_ms):
        limit = max_rows if max_rows and max_rows > 0 else 1000
        params = params if isinstance(params, list) else []
        try:
            cur = self.conn.cursor()
            cur.execute(sql, params)
            if cur.description is None:
                # DML / DDL
                affected = cur.rowcount
                page = {
                    "columns": [],
                    "rows": [],
                    "row_count": affected if affected and affected >= 0 else 0,
                    "has_more": False,
                    "truncated": False,
                    "notices": [],
                }
                if affected is not None and affected >= 0:
                    page["affected_rows"] = affected
                cur.close()
                return page
            # 查询：内联阈值
            columns = [{"name": d[0], "type_raw": d[1] or "TEXT", "nullable": True}
                       for d in (cur.description or [])]
            rows = []
            read = 0
            byte_budget = INLINE_MAX_BYTES
            byte_truncated = False
            row_truncated = False
            while True:
                if read >= limit:
                    row_truncated = True
                    break
                row = cur.fetchone()
                if row is None:
                    break
                obj = {}
                for i, col in enumerate(columns):
                    obj[col["name"]] = _to_json(row[i])
                encoded = json.dumps(obj, ensure_ascii=False)
                if byte_budget >= 0:
                    byte_budget -= len(encoded.encode("utf-8"))
                    if byte_budget < 0:
                        byte_truncated = True
                        break
                rows.append(obj)
                read += 1
            cur.close()
            truncated = row_truncated or byte_truncated
            notices = []
            if byte_truncated:
                notices.append("结果超过内联上限（256 KiB），已截断；Arrow 附件 v1 未接入，请缩小查询范围")
            return {
                "columns": columns,
                "row_count": read,
                "rows": rows,
                "has_more": False,
                "truncated": truncated,
                "notices": notices,
            }
        except Exception as e:
            # 取消语义：标记已记 + 错误里带 interrupt/cancel 字样 → 取消；否则 SQL 错
            lowered = str(e).lower()
            if request_id in self._cancelled and (
                    "interrupt" in lowered or "cancel" in lowered or "57014" in lowered):
                raise RpcError(ERR_CANCELLED, f"查询已取消：{e}")
            raise RpcError(ERR_SQL_ERROR, f"SQL 错误：{e}")

    # ---- meta（按驱动分叉：SQLite = PRAGMA + sqlite_master；PG = information_schema）----

    def meta_catalogs(self):
        if self.kind == "postgresql":
            # PG 单库无 catalog 维度：导航用 schema 层即可，catalogs 给空（宿主会跳过）
            return {"catalogs": []}
        return {"catalogs": []}

    def meta_schemas(self, catalog):
        try:
            if self.kind == "sqlite":
                rows = self.conn.execute("PRAGMA database_list").fetchall()
                return {"schemas": [{"name": r[1]} for r in rows]}
            rows = self.conn.execute(
                "SELECT nspname FROM pg_namespace "
                "WHERE nspname NOT IN ('pg_catalog','information_schema','pg_toast') "
                "ORDER BY nspname").fetchall()
            return {"schemas": [{"name": r[0]} for r in rows]}
        except Exception as e:
            raise RpcError(ERR_SQL_ERROR, f"meta.schemas 失败：{e}")

    def meta_objects(self, catalog, schema):
        try:
            if self.kind == "sqlite":
                schema = schema or "main"
                rows = self.conn.execute(
                    "SELECT name, type FROM sqlite_master "
                    "WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' "
                    "ORDER BY name").fetchall()
                out = []
                for name, typ in rows:
                    out.append({
                        "name": name,
                        "kind": "view" if typ == "view" else "table",
                        "parent": schema,
                    })
                return {"objects": out}
            # PG：表 + 视图（information_schema.tables）
            schema = schema or "public"
            rows = self.conn.execute(
                "SELECT table_name, table_type FROM information_schema.tables "
                "WHERE table_schema = %s ORDER BY table_name", (schema,)).fetchall()
            out = []
            for name, typ in rows:
                kind = "view" if typ.upper() == "VIEW" else "table"
                out.append({"name": name, "kind": kind, "parent": schema})
            # 例程（procedure / function）
            try:
                routines = self.conn.execute(
                    "SELECT routine_name, routine_type FROM information_schema.routines "
                    "WHERE routine_schema = %s ORDER BY routine_name", (schema,)).fetchall()
                for name, typ in routines:
                    kind = "procedure" if typ.upper() == "PROCEDURE" else "function"
                    out.append({"name": name, "kind": kind, "parent": schema})
            except Exception:
                pass  # 老版本 information_schema 无 routines 视图 → 少一类对象不是错误
            return {"objects": out}
        except Exception as e:
            raise RpcError(ERR_SQL_ERROR, f"meta.objects 失败：{e}")

    def meta_object_detail(self, catalog, schema, obj):
        try:
            if self.kind == "sqlite":
                return self._meta_object_detail_sqlite(schema, obj)
            return self._meta_object_detail_pg(schema, obj)
        except Exception as e:
            raise RpcError(ERR_SQL_ERROR, f"meta.object_detail 失败：{e}")

    def _meta_object_detail_sqlite(self, schema, obj):
        schema = schema or "main"
        columns = []
        for row in self.conn.execute(f"PRAGMA table_info({_q(obj)})").fetchall():
            cid, name, typ, notnull, default, pk = row
            columns.append({
                "name": name,
                "type_raw": typ or "TEXT",
                "nullable": not notnull,
                "is_pk": bool(pk),
                "is_fk": False,
            })
        fks = self.conn.execute(f"PRAGMA foreign_key_list({_q(obj)})").fetchall()
        fk_cols = {r[3] for r in fks}
        for col in columns:
            if col["name"] in fk_cols:
                col["is_fk"] = True
        return {
            "object": {"name": obj, "kind": "table", "parent": schema},
            "columns": columns,
        }

    def _meta_object_detail_pg(self, schema, obj):
        schema = schema or "public"
        # 主键列
        pk_rows = self.conn.execute(
            "SELECT a.attname FROM pg_index i "
            "JOIN pg_class c ON c.oid = i.indrelid "
            "JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(i.indkey) "
            "JOIN pg_namespace n ON n.oid = c.relnamespace "
            "WHERE i.indisprimary AND n.nspname = %s AND c.relname = %s",
            (schema, obj)).fetchall()
        pks = {r[0] for r in pk_rows}
        # 外键列
        fk_rows = self.conn.execute(
            "SELECT a.attname FROM pg_constraint con "
            "JOIN pg_class c ON c.oid = con.conrelid "
            "JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(con.conkey) "
            "JOIN pg_namespace n ON n.oid = c.relnamespace "
            "WHERE con.contype = 'f' AND n.nspname = %s AND c.relname = %s",
            (schema, obj)).fetchall()
        fks = {r[0] for r in fk_rows}
        # 列定义
        rows = self.conn.execute(
            "SELECT column_name, data_type, is_nullable, column_default "
            "FROM information_schema.columns "
            "WHERE table_schema = %s AND table_name = %s ORDER BY ordinal_position",
            (schema, obj)).fetchall()
        columns = []
        for name, data_type, nullable, default in rows:
            col = {
                "name": name,
                "type_raw": data_type or "text",
                "nullable": nullable == "YES",
                "is_pk": name in pks,
                "is_fk": name in fks,
            }
            if default is not None:
                col["default"] = default
            columns.append(col)
        return {
            "object": {"name": obj, "kind": "table", "parent": schema},
            "columns": columns,
        }


def _normalize_driver(raw):
    """引擎驱动 id → 壳适配器名（`sqlite` / `postgresql`）。"""
    d = (raw or "").lower()
    if d in ("postgresql", "postgres", "pg"):
        return "postgresql"
    if d == "sqlite":
        return "sqlite"
    return d


def _q(name):
    """SQLite 标识符引号（避开 PRAGMA 注入）。"""
    return '"' + name.replace('"', '""') + '"'


def _to_json(v):
    if v is None:
        return None
    if isinstance(v, bool):
        return v
    if isinstance(v, int):
        return v
    if isinstance(v, float):
        return v
    if isinstance(v, bytes):
        return "0x" + v.hex()
    if isinstance(v, str):
        return v
    return str(v)


class Server:
    def __init__(self):
        self.io = FrameIO()
        self.sessions = {}
        self._sessions_lock = threading.Lock()

    def run(self):
        q = queue.Queue()
        worker = threading.Thread(target=self._worker_loop, args=(q,), daemon=True)
        worker.start()
        try:
            while True:
                text = self.io.read()
                if text is None:
                    break  # stdin EOF：宿主已死
                try:
                    req = json.loads(text)
                except ValueError:
                    self.io.write(self._error(None, -32700, "parse error"))
                    continue
                if "method" not in req:
                    continue  # 响应帧，忽略
                method = req["method"]
                rid = req.get("id")
                params = req.get("params") or {}

                if method == "query.cancel":
                    # 主线程即时处理：interrupt() 不阻塞，能打断 worker 里的 execute
                    try:
                        self._cancel(params)
                        self.io.write(self._response(rid, {}))
                    except Exception as e:
                        self.io.write(self._error(rid, self._code_of(e), str(e)))
                    continue

                shutdown = method == "shutdown"
                q.put((rid, method, params, shutdown))
                if shutdown:
                    worker.join(5)
                    break
        finally:
            with self._sessions_lock:
                for s in self.sessions.values():
                    try:
                        s.conn.close()
                    except Exception:
                        pass

    def _worker_loop(self, q):
        while True:
            rid, method, params, shutdown = q.get()
            if shutdown:
                self.io.write(self._response(rid, {}))
                return
            try:
                result = self._dispatch(method, params)
                self.io.write(self._response(rid, result))
            except Exception as e:
                self.io.write(self._error(rid, self._code_of(e), str(e)))

    def _dispatch(self, method, params):
        if method == "initialize":
            peer = params.get("protocol") if isinstance(params, dict) else 0
            if peer != PROTOCOL_VERSION:
                raise RpcError(ERR_PROTOCOL_MISMATCH,
                               f"协议版本不匹配：宿主 {peer}，插件 {PROTOCOL_VERSION}")
            return {"protocol": PROTOCOL_VERSION, "driver_ids": ["dbapi"]}
        if method == "ping":
            return {}
        if method == "session.open":
            sid = params["session_id"]
            driver_id = params.get("driver_id") or "dbapi"
            cfg = params.get("params") or {}
            with self._sessions_lock:
                self.sessions[sid] = Session(sid, driver_id, cfg)
            return {}
        if method == "session.close":
            s = self._session(params)
            s.conn.close()
            with self._sessions_lock:
                self.sessions.pop(s.id, None)
            return {}
        if method == "session.ping":
            self._session(params)
            return {}
        if method == "driver.describe":
            return self._session(params).describe()
        if method == "meta.catalogs":
            return self._session(params).meta_catalogs()
        if method == "meta.schemas":
            return self._session(params).meta_schemas(params.get("catalog"))
        if method == "meta.objects":
            return self._session(params).meta_objects(params.get("catalog"), params.get("schema"))
        if method == "meta.object_detail":
            return self._session(params).meta_object_detail(
                params.get("catalog"), params.get("schema"), params.get("object"))
        if method == "meta.routine_source":
            return None
        if method == "query.execute":
            s = self._session(params)
            return s.execute(params.get("request_id"), params.get("sql"),
                             params.get("params") or [], params.get("max_rows"), params.get("timeout_ms"))
        if method == "query.fetch":
            raise RpcError(ERR_DRIVER_NOT_SUPPORTED,
                           "query.fetch 需要驱动游标能力（v1 内联结果，第二期接 Arrow 附件）")
        raise RpcError(-32601, f"方法不存在：{method}")

    def _cancel(self, params):
        s = self._session(params)
        s.cancel(params.get("request_id"))

    def _session(self, params):
        sid = params.get("session_id")
        with self._sessions_lock:
            s = self.sessions.get(sid)
        if s is None:
            raise RpcError(ERR_SESSION_NOT_FOUND, f"会话不存在：{sid}")
        return s

    @staticmethod
    def _code_of(e):
        if isinstance(e, RpcError):
            return e.code
        # 非 RpcError 一律按 SQL 错处理（sqlite / psycopg 的异常类型都是驱动私有的）
        return ERR_SQL_ERROR

    @staticmethod
    def _response(rid, result):
        return json.dumps({"jsonrpc": "2.0", "id": rid, "result": result}, ensure_ascii=False)

    @staticmethod
    def _error(rid, code, message):
        return json.dumps({"jsonrpc": "2.0", "id": rid,
                           "error": {"code": code, "message": message}}, ensure_ascii=False)


if __name__ == "__main__":
    Server().run()
