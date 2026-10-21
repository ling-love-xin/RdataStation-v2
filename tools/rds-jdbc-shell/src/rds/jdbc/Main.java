package rds.jdbc;

/**
 * 入口：自举驱动注册 → 协议服务主循环。
 *
 * 进程协议（对齐宿主侧约定）：
 * - stdin / stdout 走二进制分帧（FrameIO）；stderr 留给宿主日志
 * - stdin EOF = 宿主已死，自行退出（不需要 Job Object / PDEATHSIG）
 * - `shutdown` 通知后收摊退出
 */
public final class Main {

    private Main() {}

    public static void main(String[] args) throws Exception {
        DriverRegistry.bootstrap();
        new Server(new FrameIO(System.in, System.out)).run();
        System.exit(0);
    }
}
