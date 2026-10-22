package rds.jdbc;

import java.io.File;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.sql.Driver;
import java.sql.DriverManager;
import java.util.ArrayList;
import java.util.List;
import java.util.ServiceLoader;

/**
 * 驱动自举：`java -jar` 时 classpath 只有壳本身，本 jar 同目录（及同目录/lib）下的
 * 驱动 jar 在这里用 URLClassLoader 加载。
 *
 * 关键取舍：**连接不走 DriverManager**。JDBC 4.0 的 `DriverManager.getConnection` 会
 * 按「调用者类加载器」做 isDriverAllowed 检查——驱动是子 URLClassLoader 加载的，
 * 调用者是系统加载器，必被拒（经典陷阱，实测 "No suitable driver found"）。
 * 因此驱动注册后直接保留实例列表，连接时按 acceptsURL 匹配实例直连。
 *
 * 两条加载路（双保险）：
 * 1. {@link ServiceLoader}：JDBC 4.0+ 驱动 jar 都带 `META-INF/services/java.sql.Driver`；
 * 2. 已知驱动类名逐个尝试（个别老驱动没有 services 文件也能注册）。
 */
public final class DriverRegistry {

    /** 常见驱动类名表（services 之外的兜底）。 */
    private static final String[] KNOWN_DRIVERS = {
        "oracle.jdbc.OracleDriver",
        "org.h2.Driver",
        "org.postgresql.Driver",
        "com.mysql.cj.jdbc.Driver",
        "com.mysql.jdbc.Driver",
        "com.microsoft.sqlserver.jdbc.SQLServerDriver",
        "org.sqlite.JDBC",
    };

    /** 已加载的驱动实例（连接专用；DriverManager 注册仅作兼容保留）。 */
    private static volatile List<Driver> drivers = List.of();

    private DriverRegistry() {}

    /** 启动时调用一次：扫描壳 jar 同目录（及同目录/lib）下的驱动 jar。 */
    public static void bootstrap() throws Exception {
        Path base = shellDir();
        List<URL> urls = new ArrayList<>();
        collectJars(base, urls);
        collectJars(base.resolve("lib"), urls);
        // 用户自定驱动 jar（宿主经 RDS_DRIVER_JARS 环境变量传入，';' 分隔，
        // 文件或目录均可）——覆盖「用户自己下载 JDBC 包、不必塞进插件包 lib/」的场景。
        collectExternal(urls);
        if (urls.isEmpty()) return;

        URLClassLoader loader = new URLClassLoader(urls.toArray(new URL[0]), DriverRegistry.class.getClassLoader());
        Thread.currentThread().setContextClassLoader(loader);

        List<Driver> found = new ArrayList<>();
        // 路 1：services 自动发现（标准 JDBC 驱动都有）
        for (Driver d : ServiceLoader.load(Driver.class, loader)) {
            found.add(d);
        }
        // 路 2：已知类名（触发静态注册，之后从 DriverManager 收回实例）
        for (String name : KNOWN_DRIVERS) {
            try {
                Class.forName(name, true, loader);
            } catch (ClassNotFoundException ignored) {
                // 这个驱动不在 lib 里，跳过
            }
        }
        var registered = DriverManager.getDrivers();
        while (registered.hasMoreElements()) {
            Driver d = registered.nextElement();
            if (found.stream().noneMatch(x -> x.getClass() == d.getClass())) {
                found.add(d);
            }
        }
        drivers = found;
    }

    /** 全部驱动实例（连接匹配用）。 */
    public static List<Driver> all() {
        return drivers;
    }

    /** 壳 jar 所在目录（<插件>/lib）。 */
    private static Path shellDir() {
        try {
            URL loc = DriverRegistry.class.getProtectionDomain().getCodeSource().getLocation();
            File f = new File(loc.toURI());
            return f.isDirectory() ? f.toPath() : f.getParentFile().toPath();
        } catch (Exception e) {
            return Path.of(".");
        }
    }

    private static void collectJars(Path dir, List<URL> urls) {
        if (!Files.isDirectory(dir)) return;
        try (var stream = Files.list(dir)) {
            stream.filter(p -> p.getFileName().toString().endsWith(".jar"))
                    .forEach(p -> {
                        try {
                            urls.add(p.toUri().toURL());
                        } catch (Exception ignored) {
                            // 单个 jar 解析失败不该拖垮整个壳
                        }
                    });
        } catch (Exception ignored) {
            // 目录不可读（权限/占用）就跳过
        }
    }

    /** 读 `RDS_DRIVER_JARS` 环境变量（宿主部署期注入），逐个加入加载列表。 */
    private static void collectExternal(List<URL> urls) {
        String raw = System.getenv("RDS_DRIVER_JARS");
        if (raw == null || raw.isBlank()) return;
        for (String entry : raw.split(";", -1)) {
            String trimmed = entry.trim();
            if (trimmed.isEmpty()) continue;
            Path p = Path.of(trimmed);
            try {
                if (Files.isDirectory(p)) {
                    collectJars(p, urls);
                } else if (Files.isRegularFile(p)) {
                    urls.add(p.toUri().toURL());
                }
                // 不存在 / 解析失败：跳过（与 collectJars 的容忍口径一致）
            } catch (Exception ignored) {
                // 单个条目坏掉不该拖垮整个壳
            }
        }
    }
}
