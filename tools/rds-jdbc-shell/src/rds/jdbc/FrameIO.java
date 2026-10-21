package rds.jdbc;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;

/**
 * rds-driver/1 帧层（对齐 `crates/plugin/src/sidecar/proto.rs`）：
 *
 * <pre>
 * Frame = [u32 BE total_len][u8 kind][payload]
 * total_len = 5 + payload.len()（含头 5 字节；读侧用 total_len - 5 得载荷长）
 * kind：0x01 JSON（v1 只走 JSON 控制面；Arrow 附件第二期）
 * </pre>
 *
 * 上限 64 MiB（防御对端谎报长度）。读写各一个独立流（stdio：读 stdin / 写 stdout）。
 */
public final class FrameIO {

    public static final int KIND_JSON = 0x01;
    public static final int MAX_FRAME_LEN = 64 * 1024 * 1024;

    private final InputStream in;
    private final OutputStream out;

    public FrameIO(InputStream in, OutputStream out) {
        this.in = new BufferedInputStream(in);
        this.out = new BufferedOutputStream(out);
    }

    /** 读一帧；返回 payload 字节（不含头）。流结束返回 null。 */
    public byte[] read() throws IOException {
        byte[] header = new byte[5];
        int n = readFully(header);
        if (n == -1) return null; // EOF
        if (n < 5) throw new IOException("帧头不完整（读到 " + n + " 字节）");
        long total = ((long) (header[0] & 0xFF) << 24)
                | ((header[1] & 0xFF) << 16)
                | ((header[2] & 0xFF) << 8)
                | (header[3] & 0xFF);
        int kind = header[4] & 0xFF;
        if (total < 5) throw new IOException("帧长 < 5（声明 " + total + "）");
        if (total > MAX_FRAME_LEN) throw new IOException("帧长超上限（声明 " + total + "）");
        int payloadLen = (int) total - 5;
        byte[] payload = new byte[payloadLen];
        readFully(payload);
        if (kind != KIND_JSON) {
            throw new IOException("未知帧种类 " + kind + "（v1 只支持 JSON）");
        }
        return payload;
    }

    /** 写一帧 JSON。 */
    public void writeJson(byte[] payload) throws IOException {
        int total = 5 + payload.length;
        if (total > MAX_FRAME_LEN) throw new IOException("帧超上限");
        out.write((total >>> 24) & 0xFF);
        out.write((total >>> 16) & 0xFF);
        out.write((total >>> 8) & 0xFF);
        out.write(total & 0xFF);
        out.write(KIND_JSON);
        out.write(payload);
        out.flush();
    }

    /** 写一帧 JSON（字符串入参）。 */
    public void writeJson(String payload) throws IOException {
        writeJson(payload.getBytes(StandardCharsets.UTF_8));
    }

    private int readFully(byte[] buf) throws IOException {
        int off = 0;
        while (off < buf.length) {
            int n = in.read(buf, off, buf.length - off);
            if (n == -1) {
                return off == 0 ? -1 : off;
            }
            off += n;
        }
        return off;
    }
}
