package rds.jdbc;

import java.math.BigDecimal;
import java.sql.Connection;
import java.sql.DatabaseMetaData;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.SQLException;
import java.sql.Statement;
import java.sql.Timestamp;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.LocalTime;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * 一个 JDBC 会话（`session.open` 建连接；meta.* / query.* 都挂会话上）。
 *
 * 与宿主 wire 契约（对齐 `crates/plugin/src/sidecar/driver.rs` + `meta.rs`）：
 * - 连接配置 = session.open 的 `params.params`（DriverConnectionConfig 序列化）
 * - 查询响应 = QueryPage JSON：columns / row_count / affected_rows / has_more /
 *   cursor_id / truncated / rows / notices
 * - 内联阈值：200 行 / 256 KiB（对齐 INLINE_JSON_MAX_ROWS / MAX_BYTES）；
 *   超过阈值 v1 截断 + truncated=true + notice（Arrow 附件第二期）
 */
public final class Session {

    /** 内联 JSON 阈值（对齐宿主 proto.rs）。 */
    static final int INLINE_MAX_ROWS = 200;
    static final int INLINE_MAX_BYTES = 256 * 1024;

    private final String id;
    private final Connection conn;
    private final String driverId;
    private final String serverVersion;
    private final String identifierQuote;
    private final String defaultSchema;

    /** 在跑的语句（request_id → statement，供 query.cancel 真取消）。 */
    private final Map<String, Statement> running = new ConcurrentHashMap<>();
    /** 已请求取消的 request_id（execute 线程据它区分「被取消」与「SQL 错」）。 */
    private final Map<String, AtomicBoolean> cancelled = new ConcurrentHashMap<>();

    public Session(String id, String driverId, Json.Obj config) throws SQLException {
        this.id = id;
        this.driverId = driverId;
        String url = buildUrl(config);
        String user = nullableStr(config, "username");
        String password = nullableStr(config, "password");
        try {
            java.util.Properties props = new java.util.Properties();
            if (user != null) props.setProperty("user", user);
            if (password != null) props.setProperty("password", password);
            // 不走 DriverManager：子类加载器驱动会被 isDriverAllowed 拒（见 DriverRegistry 注释）
            this.conn = DriverRegistry.all().stream()
                    .filter(d -> {
                        try {
                            return d.acceptsURL(url);
                        } catch (Exception e) {
                            return false;
                        }
                    })
                    .map(d -> {
                        try {
                            return d.connect(url, props);
                        } catch (SQLException e) {
                            throw new RpcException(Server.CODE_CONNECT_FAILED,
                                    "连接失败 " + url + "：" + e.getMessage(), e);
                        }
                    })
                    .findFirst()
                    .orElseThrow(() -> new RpcException(Server.CODE_CONNECT_FAILED,
                            "没有驱动接受 " + url + "（插件 lib/ 下缺少对应驱动 jar？）"));
        } catch (RpcException e) {
            throw e;
        }
        this.serverVersion = conn.getMetaData().getDatabaseProductVersion();
        this.identifierQuote = conn.getMetaData().getIdentifierQuoteString();
        String schema;
        try {
            schema = conn.getSchema();
        } catch (SQLException | AbstractMethodError e) {
            schema = null;
        }
        this.defaultSchema = schema;
    }

    public String id() { return id; }

    /** 拼 JDBC URL：url_override 优先 → url_template 模板替换 → 按 driver 兜底。 */
    static String buildUrl(Json.Obj config) throws RpcException {
        String override = nullableStr(config, "url_override");
        if (override != null && !override.isBlank()) return override.trim();

        String driver = nullableStr(config, "driver");
        String host = nullableStr(config, "host");
        String port = nullableStr(config, "port");
        String db = nullableStr(config, "database");
        String file = nullableStr(config, "file_path");
        String user = nullableStr(config, "username");
        String password = nullableStr(config, "password");

        String template = nullableStr(config, "url_template");
        if (template != null && !template.isBlank()) {
            return template
                    .replace("{host}", host == null ? "" : host)
                    .replace("{port}", port == null ? "" : port)
                    .replace("{database}", db == null ? "" : db)
                    .replace("{file}", file == null ? "" : file)
                    .replace("{user}", user == null ? "" : user)
                    .replace("{password}", password == null ? "" : password);
        }

        // 兜底：常见驱动形状
        if ("h2".equals(driver)) {
            return file != null ? "jdbc:h2:file:" + file : "jdbc:h2:mem:rds";
        }
        if ("sqlite".equals(driver)) {
            return file != null ? "jdbc:sqlite:" + file : "jdbc:sqlite::memory:";
        }
        if (host == null) {
            throw new RpcException(Server.CODE_CONNECT_FAILED,
                    "无法构造连接 URL：既没有 url_override / url_template，host 也为空");
        }
        String hp = port == null || port.isBlank() ? host : host + ":" + port;
        return "jdbc:" + (driver == null ? "generic" : driver) + "://" + hp + "/" + (db == null ? "" : db);
    }

    /** driver.describe 的自述（运行时那份，清单可以撒谎、进程不会）。 */
    public Json.Obj describe() {
        Json.Obj caps = new Json.Obj();
        caps.fields.put("schemas", new Json.Bool(true));
        caps.fields.put("views", new Json.Bool(true));
        caps.fields.put("transactions", new Json.Bool(true));
        caps.fields.put("cancel", new Json.Bool(true));
        caps.fields.put("cursor", new Json.Bool(false));
        caps.fields.put("arrow", new Json.Bool(false)); // v1 未接 Arrow 附件

        Json.Obj out = new Json.Obj();
        out.fields.put("id", new Json.Str(driverId));
        out.fields.put("display_name", new Json.Str(driverId));
        out.fields.put("server_version", new Json.Str(serverVersion));
        out.fields.put("capabilities", caps);
        if (identifierQuote != null && !identifierQuote.isBlank()) {
            out.fields.put("identifier_quote", new Json.Str(identifierQuote));
        }
        if (defaultSchema != null) {
            out.fields.put("default_schema", new Json.Str(defaultSchema));
        }
        return out;
    }

    /**
     * 执行一条查询（query.execute）。
     * 返回 QueryPage JSON；被取消时抛 RpcException(-32004)。
     */
    public Json.Obj execute(String requestId, String sql, Json.Value params,
                            long maxRows, Long timeoutMs) throws SQLException {
        long limit = maxRows > 0 ? maxRows : 1000;
        Statement stmt = conn.createStatement();
        if (timeoutMs != null) {
            stmt.setQueryTimeout((int) Math.max(1, Math.round(timeoutMs / 1000.0)));
        }
        running.put(requestId, stmt);
        try {
            List<Json.Value> bind = params == null || params.isNull() ? List.of()
                    : params.isArray() ? params.asArray() : List.of();

            boolean hasResultSet = false;
            if (bind.isEmpty()) {
                hasResultSet = stmt.execute(sql);
            } else {
                PreparedStatement ps = conn.prepareStatement(sql);
                try {
                    for (int i = 0; i < bind.size(); i++) {
                        ps.setObject(i + 1, toJdbc(bind.get(i)));
                    }
                    hasResultSet = ps.execute();
                } finally {
                    // DML 或 execute 抛异常：立即关；查询（hasResultSet=true）保留给结果集读
                    if (!hasResultSet) ps.close();
                }
                stmt = ps; // 让 cancel 能取消 prepared 语句
            }

            if (hasResultSet) {
                try (ResultSet rs = stmt.getResultSet()) {
                    return pageFrom(rs, limit, sql);
                }
            }
            // DML / DDL：影响行数
            Json.Obj page = new Json.Obj();
            int affected = stmt.getUpdateCount();
            if (affected >= 0) page.fields.put("affected_rows", new Json.Num(affected));
            page.fields.put("columns", new Json.Arr()); // 空结果集也要有 schema（宿主契约）
            page.fields.put("rows", new Json.Arr());
            page.fields.put("row_count", new Json.Num(affected >= 0 ? affected : 0));
            page.fields.put("has_more", new Json.Bool(false));
            page.fields.put("truncated", new Json.Bool(false));
            page.fields.put("notices", new Json.Arr());
            return page;
        } catch (SQLException e) {
            if (cancelled.remove(requestId) != null) {
                throw new RpcException(Server.CODE_CANCELLED, "查询已被取消：" + e.getMessage(), e);
            }
            throw e;
        } finally {
            running.remove(requestId);
        }
    }

    /** 真取消一条在跑的查询（query.cancel；JDBC Statement.cancel() 是标准能力）。 */
    public void cancel(String requestId) throws SQLException {
        cancelled.put(requestId, new AtomicBoolean(true));
        Statement stmt = running.get(requestId);
        if (stmt != null) stmt.cancel();
    }

    public void close() throws SQLException {
        conn.close();
    }

    // ==================== 结果集 → QueryPage ====================

    private Json.Obj pageFrom(ResultSet rs, long limit, String sql) throws SQLException {
        ResultSetMetaData md = rs.getMetaData();
        int cols = md.getColumnCount();

        // 列信息
        Json.Arr columns = new Json.Arr();
        for (int i = 1; i <= cols; i++) {
            Json.Obj col = new Json.Obj();
            col.fields.put("name", new Json.Str(md.getColumnName(i)));
            col.fields.put("type_raw", new Json.Str(md.getColumnTypeName(i)));
            col.fields.put("nullable", new Json.Bool(md.isNullable(i) != ResultSetMetaData.columnNoNulls));
            columns.items.add(col);
        }

        // 行（先按 max_rows 截断，再按内联阈值兜底）
        Json.Arr rows = new Json.Arr();
        long read = 0;
        boolean rowTruncated = false;
        int byteBudget = INLINE_MAX_BYTES;
        boolean byteTruncated = false;
        while (rs.next()) {
            if (read >= limit) { rowTruncated = true; break; }
            Json.Obj row = new Json.Obj();
            for (int i = 1; i <= cols; i++) {
                row.fields.put(md.getColumnName(i), cellValue(rs, i));
            }
            String encoded = row.toString();
            if (byteBudget >= 0) {
                byteBudget -= encoded.getBytes(java.nio.charset.StandardCharsets.UTF_8).length;
                if (byteBudget < 0) { byteTruncated = true; break; }
            }
            rows.items.add(row);
            read++;
        }

        boolean truncated = rowTruncated || byteTruncated;
        Json.Arr notices = new Json.Arr();
        if (byteTruncated) {
            notices.items.add(new Json.Str(
                    "结果超过内联上限（256 KiB），已截断；Arrow 附件 v1 未接入，请缩小查询范围"));
        }

        Json.Obj page = new Json.Obj();
        page.fields.put("columns", columns);
        page.fields.put("row_count", new Json.Num(read));
        page.fields.put("rows", rows);
        page.fields.put("has_more", new Json.Bool(false));
        page.fields.put("truncated", new Json.Bool(truncated));
        page.fields.put("notices", notices);
        return page;
    }

    private static Json.Value cellValue(ResultSet rs, int i) throws SQLException {
        Object v = rs.getObject(i);
        if (v == null) return Json.Null.INSTANCE;
        if (v instanceof Boolean b) return new Json.Bool(b);
        if (v instanceof Byte || v instanceof Short || v instanceof Integer || v instanceof Long) {
            return new Json.Num(((Number) v).longValue());
        }
        if (v instanceof java.math.BigInteger bi) return new Json.Num(bi.longValue());
        if (v instanceof Float || v instanceof Double || v instanceof BigDecimal) {
            return new Json.Num(((Number) v).doubleValue());
        }
        if (v instanceof String s) return new Json.Str(s);
        if (v instanceof byte[] bytes) {
            return new Json.Str(Base64.getEncoder().encodeToString(bytes));
        }
        if (v instanceof java.sql.Date d) return new Json.Str(d.toLocalDate().toString());
        if (v instanceof Timestamp ts) return new Json.Str(ts.toLocalDateTime().toString());
        if (v instanceof java.sql.Time t) return new Json.Str(t.toLocalTime().toString());
        if (v instanceof LocalDate d) return new Json.Str(d.toString());
        if (v instanceof LocalDateTime dt) return new Json.Str(dt.toString());
        if (v instanceof LocalTime t) return new Json.Str(t.toString());
        if (v instanceof UUID u) return new Json.Str(u.toString());
        if (v instanceof java.sql.Array arr) {
            Object[] elems = (Object[]) arr.getArray();
            Json.Arr out = new Json.Arr();
            for (Object e : elems) out.items.add(scalar(e));
            return out;
        }
        return new Json.Str(v.toString());
    }

    private static Json.Value scalar(Object v) {
        if (v == null) return Json.Null.INSTANCE;
        if (v instanceof Boolean b) return new Json.Bool(b);
        if (v instanceof Number n) return new Json.Num(n.doubleValue());
        return new Json.Str(v.toString());
    }

    /** JSON 参数 → JDBC setObject 值。 */
    private static Object toJdbc(Json.Value v) {
        if (v.isNull()) return null;
        if (v.isBool()) return v.asBool();
        if (v.isNumber()) {
            Json.Num n = (Json.Num) v;
            return n.isIntegral ? n.asLong() : n.asDouble();
        }
        if (v.isString()) return v.asString();
        if (v.isArray()) {
            List<Json.Value> items = v.asArray();
            Object[] arr = new Object[items.size()];
            for (int i = 0; i < items.size(); i++) arr[i] = toJdbc(items.get(i));
            return arr;
        }
        return v.toString();
    }

    // ==================== meta（DatabaseMetaData → meta.*） ====================

    public DatabaseMetaData meta() {
        try {
            return conn.getMetaData();
        } catch (SQLException e) {
            throw new RpcException(Server.CODE_SQL_ERROR, "取元数据失败：" + e.getMessage(), e);
        }
    }

    private static String nullableStr(Json.Obj cfg, String key) {
        Json.Value v = cfg.fields.get(key);
        if (v == null || v.isNull()) return null;
        if (v.isString()) return v.asString();
        return v.toString();
    }
}
