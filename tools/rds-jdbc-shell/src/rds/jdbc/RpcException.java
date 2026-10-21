package rds.jdbc;

/**
 * 协议错误（对齐宿主 RpcErrorCode：-32001…-32009）。
 */
public final class RpcException extends RuntimeException {

    /** -32001 驱动不支持（方法/能力未实现）。 */
    public static final int CODE_DRIVER_NOT_SUPPORTED = -32001;
    /** -32002 会话不存在。 */
    public static final int CODE_SESSION_NOT_FOUND = -32002;
    /** -32003 SQL 错误。 */
    public static final int CODE_SQL_ERROR = -32003;
    /** -32004 已取消。 */
    public static final int CODE_CANCELLED = -32004;
    /** -32005 超时。 */
    public static final int CODE_TIMEOUT = -32005;
    /** -32006 能力被拒。 */
    public static final int CODE_CAPABILITY_DENIED = -32006;
    /** -32007 协议版本不匹配。 */
    public static final int CODE_PROTOCOL_MISMATCH = -32007;
    /** -32008 资源超限。 */
    public static final int CODE_RESOURCE_LIMIT = -32008;
    /** -32009 连不上目标库。 */
    public static final int CODE_CONNECT_FAILED = -32009;

    public final int code;

    public RpcException(int code, String message) {
        super(message);
        this.code = code;
    }

    public RpcException(int code, String message, Throwable cause) {
        super(message, cause);
        this.code = code;
    }
}
