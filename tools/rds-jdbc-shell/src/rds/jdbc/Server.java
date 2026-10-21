package rds.jdbc;

import java.nio.charset.StandardCharsets;
import java.util.Map;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.LinkedBlockingQueue;

/**
 * rds-driver/1 服务端主循环：读帧 → 分发 → 响应。
 *
 * 线程模型（cancel 必须即时，否则执行阻塞时 cancel 帧读不到——单线程会排队到
 * execute 结束后才处理，取消就成了空操作）：
 * - 主线程：只读帧。`query.cancel` 立即处理（Statement.cancel() 线程安全、不阻塞），
 *   其余请求入队交给 worker 顺序执行。
 * - worker：顺序执行非 cancel 请求（保持 RPC 顺序语义），execute 阻塞不影响 cancel。
 * - 响应写回：FrameIO 内部同步（worker 写 execute 响应、主线程写 cancel 响应并发）。
 * - shutdown：交给 worker 收尾（保证先到先出），随后 join 退出。
 *
 * 信封（对齐宿主 `sidecar/conn.rs`）：
 * - 请求：{"jsonrpc":"2.0","id":n,"method":"...","params":{...}}
 * - 响应：{"jsonrpc":"2.0","id":n,"result":...} / {"jsonrpc":"2.0","id":n,"error":{"code","message"}}
 * - stdin EOF = 宿主已死，直接退出（协议级约定）
 *
 * v1 方法面：initialize / ping / shutdown / session.open|close|ping /
 * driver.describe / meta.catalogs|schemas|objects|object_detail|routine_source /
 * query.execute|fetch|cancel。Arrow 附件、query.fetch 游标第二期。
 */
public final class Server {

    static final int CODE_DRIVER_NOT_SUPPORTED = RpcException.CODE_DRIVER_NOT_SUPPORTED;
    static final int CODE_SESSION_NOT_FOUND = RpcException.CODE_SESSION_NOT_FOUND;
    static final int CODE_SQL_ERROR = RpcException.CODE_SQL_ERROR;
    static final int CODE_CANCELLED = RpcException.CODE_CANCELLED;
    static final int CODE_CONNECT_FAILED = RpcException.CODE_CONNECT_FAILED;
    static final int CODE_PROTOCOL_MISMATCH = RpcException.CODE_PROTOCOL_MISMATCH;

    /** 协议版本（对齐宿主 PROTOCOL_VERSION = 1）。 */
    static final long PROTOCOL_VERSION = 1;

    private final FrameIO io;
    /** 会话表（session_id → 会话）。 */
    private final Map<String, Session> sessions = new ConcurrentHashMap<>();

    /** worker 队列里的一个请求。 */
    private record Job(Json.Value id, String method, Json.Value params, boolean shutdown) {}

    public Server(FrameIO io) {
        this.io = io;
    }

    public void run() throws Exception {
        BlockingQueue<Job> queue = new LinkedBlockingQueue<>();
        Thread worker = new Thread(() -> runWorker(queue), "rds-jdbc-worker");
        worker.setDaemon(true);
        worker.start();

        while (true) {
            byte[] payload = io.read();
            if (payload == null) break; // stdin EOF：宿主已死
            String text = new String(payload, StandardCharsets.UTF_8);
            Json.Obj req;
            try {
                req = (Json.Obj) Json.parse(text);
            } catch (Exception e) {
                io.writeJson(errorFrame(null, -32700, "parse error: " + e.getMessage()));
                continue;
            }
            if (req.fields.get("method") == null) continue; // 响应帧（v1 宿主不回发），忽略
            String method = req.fields.get("method").asString();
            Json.Value id = req.fields.get("id");
            Json.Value params = req.fields.getOrDefault("params", Json.Null.INSTANCE);

            if ("query.cancel".equals(method)) {
                // 主线程即时处理：stmt.cancel() 不阻塞，能打断 worker 里跑着的 execute
                try {
                    dispatch(method, params);
                    io.writeJson(responseFrame(id, new Json.Obj()));
                } catch (Throwable t) {
                    io.writeJson(errorFrame(id, toErrorCode(t), messageOf(t)));
                }
                continue;
            }
            boolean shutdown = "shutdown".equals(method);
            queue.add(new Job(id, method, params, shutdown));
            if (shutdown) {
                worker.join(5000);
                break;
            }
        }
        // 收摊：关掉全部会话（stdin EOF 也会走到这里）
        for (Session s : sessions.values()) {
            try { s.close(); } catch (Exception ignored) {}
        }
    }

    private void runWorker(BlockingQueue<Job> queue) {
        while (true) {
            Job job;
            try {
                job = queue.take();
            } catch (InterruptedException e) {
                return;
            }
            try {
                if (job.shutdown) {
                    io.writeJson(responseFrame(job.id, new Json.Obj()));
                    return;
                }
                Json.Value result = dispatch(job.method, job.params);
                io.writeJson(responseFrame(job.id, result));
            } catch (Throwable t) {
                try {
                    io.writeJson(errorFrame(job.id, toErrorCode(t), messageOf(t)));
                } catch (Exception e) {
                    return; // stdout 坏了（宿主已死），worker 退出
                }
            }
        }
    }

    private static int toErrorCode(Throwable t) {
        if (t instanceof RpcException e) return e.code;
        if (t instanceof java.sql.SQLException) return CODE_SQL_ERROR;
        return CODE_SQL_ERROR;
    }

    private static String messageOf(Throwable t) {
        if (t instanceof RpcException e) return e.getMessage();
        if (t instanceof java.sql.SQLException e) return "SQL 错误：" + e.getMessage();
        return "内部错误：" + t;
    }

    private Json.Value dispatch(String method, Json.Value params) throws Exception {
        switch (method) {
            case "initialize": return initialize(params);
            case "ping": return new Json.Obj();
            case "session.open": return sessionOpen(params);
            case "session.close": {
                session(params).close();
                sessions.remove(sessionId(params));
                return new Json.Obj();
            }
            case "session.ping": {
                session(params); // 不存在会抛 -32002
                return new Json.Obj();
            }
            case "driver.describe": return session(params).describe();
            case "meta.catalogs": return Meta.catalogs(session(params));
            case "meta.schemas": return Meta.schemas(session(params), strParam(params, "catalog"));
            case "meta.objects": return Meta.objects(session(params), strParam(params, "catalog"), strParam(params, "schema"));
            case "meta.object_detail":
                return Meta.objectDetail(session(params), strParam(params, "catalog"),
                        strParam(params, "schema"), strParam(params, "object"));
            case "meta.routine_source": return Meta.routineSource();
            case "query.execute": return queryExecute(params);
            case "query.fetch":
                // 游标第二期；显式报不支持而不是静默返回空
                throw new RpcException(CODE_DRIVER_NOT_SUPPORTED,
                        "query.fetch 需要驱动游标能力（v1 内联结果，第二期接 Arrow 附件）");
            case "query.cancel": {
                Session s = session(params);
                s.cancel(strParam(params, "request_id"));
                return new Json.Obj();
            }
            default:
                // JSON-RPC 标准：未知方法 -32601
                throw new RpcException(-32601, "方法不存在：" + method);
        }
    }

    private Json.Value initialize(Json.Value params) {
        Json.Obj p = params.isObject() ? (Json.Obj) params : new Json.Obj();
        Json.Value protocol = p.fields.get("protocol");
        long peer = protocol == null || !protocol.isNumber() ? 0 : protocol.asLong();
        if (peer != PROTOCOL_VERSION) {
            throw new RpcException(CODE_PROTOCOL_MISMATCH,
                    "协议版本不匹配：宿主 " + peer + "，插件 " + PROTOCOL_VERSION);
        }
        Json.Obj res = new Json.Obj();
        res.fields.put("protocol", new Json.Num(PROTOCOL_VERSION));
        Json.Arr ids = new Json.Arr();
        ids.items.add(new Json.Str("jdbc"));
        res.fields.put("driver_ids", ids);
        return res;
    }

    private Json.Value sessionOpen(Json.Value params) throws Exception {
        Json.Obj p = params.isObject() ? (Json.Obj) params : new Json.Obj();
        String sessionId = strParam(params, "session_id");
        String driverId = strParam(params, "driver_id");
        Json.Value inner = p.fields.get("params");
        Json.Obj config = inner != null && inner.isObject() ? (Json.Obj) inner : new Json.Obj();
        Session s = new Session(sessionId, driverId == null ? "jdbc" : driverId, config);
        sessions.put(sessionId, s);
        return new Json.Obj();
    }

    private Json.Value queryExecute(Json.Value params) throws Exception {
        Session s = session(params);
        String requestId = strParam(params, "request_id");
        String sql = strParam(params, "sql");
        Json.Value bind = params.isObject() ? ((Json.Obj) params).fields.get("params") : null;
        long maxRows = numParam(params, "max_rows", 1000);
        Long timeout = params.isObject()
                ? optionalNum(((Json.Obj) params).fields.get("timeout_ms"))
                : null;
        return s.execute(requestId, sql, bind, maxRows, timeout);
    }

    private Session session(Json.Value params) throws RpcException {
        String id = strParam(params, "session_id");
        Session s = id == null ? null : sessions.get(id);
        if (s == null) {
            throw new RpcException(CODE_SESSION_NOT_FOUND,
                    "会话不存在：" + (id == null ? "<null>" : id));
        }
        return s;
    }

    private static String sessionId(Json.Value params) {
        return strParam(params, "session_id");
    }

    static String strParam(Json.Value params, String key) {
        if (params == null || !params.isObject()) return null;
        Json.Value v = ((Json.Obj) params).fields.get(key);
        return v == null || v.isNull() ? null : v.asString();
    }

    static long numParam(Json.Value params, String key, long def) {
        if (params == null || !params.isObject()) return def;
        Json.Value v = ((Json.Obj) params).fields.get(key);
        return v == null || !v.isNumber() ? def : v.asLong();
    }

    static Long optionalNum(Json.Value v) {
        return v == null || !v.isNumber() ? null : v.asLong();
    }

    private static String responseFrame(Json.Value id, Json.Value result) {
        Json.Obj out = new Json.Obj();
        out.fields.put("jsonrpc", new Json.Str("2.0"));
        out.fields.put("id", id == null ? Json.Null.INSTANCE : id);
        out.fields.put("result", result);
        return out.toString();
    }

    private static String errorFrame(Json.Value id, int code, String message) {
        Json.Obj err = new Json.Obj();
        err.fields.put("code", new Json.Num(code));
        err.fields.put("message", new Json.Str(message == null ? "" : message));
        Json.Obj out = new Json.Obj();
        out.fields.put("jsonrpc", new Json.Str("2.0"));
        out.fields.put("id", id == null ? Json.Null.INSTANCE : id);
        out.fields.put("error", err);
        return out.toString();
    }
}
