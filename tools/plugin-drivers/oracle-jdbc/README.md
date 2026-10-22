# rds.driver.oracle — Oracle JDBC 驱动插件

通用 JDBC 壳（`tools/rds-jdbc-shell`）+ ojdbc11 驱动 jar 的打包形态。
一次开发全库复用：换数据库 = 换 `lib/` 里的驱动 jar + `contributes.drivers` 的
`url_template`，壳零改动（JDBC 标准接口天然统一）。

## 运行时要求

- **Java 21+**（壳按 Java 21 编译）。宿主按以下顺序找解释器（`crates/plugin/src/runtime.rs`）：
  1. 用户显式指定的 `java` 路径（插件设置）；
  2. `JAVA_HOME\bin\java.exe` / PATH 里的 `java`；
  3. `<仓库根>/third_party\jre\bin\java.exe`（若存在则用并告知；自动下载二期）。
  三者都缺 → 宿主返回可读错误，引导用户安装 JRE 或指定路径。
- **不内嵌 Java 环境**：以上任一路径满足即可，插件本体只有 jar。

## 驱动 jar

`lib/` 由 `tools/plugin-drivers/install.ps1` 组装：

| 文件 | 来源 | 用途 |
| --- | --- | --- |
| `rds-jdbc-shell.jar` | `tools/rds-jdbc-shell/build.ps1` 构建 | 通用壳（协议/执行/meta 映射） |
| `ojdbc11.jar` | Maven Central `com.oracle.database.jdbc:ojdbc11` | Oracle JDBC 驱动（JDK 11+） |
| `h2-2.x.x.jar` | Maven Central `com.h2database:h2` | **仅验证用**：无证书跑通协议全链路 |

> **许可说明**：ojdbc11 为 Oracle Technology Network（OTN）许可，仅用于经授权的
> Oracle 数据库访问；分发请遵守 OTN 条款。H2 为 MPL 2.0 / EPL 1.0 双许可。
> 安装脚本只从 Oracle 官方 Maven Central 坐标下载，不在本地分发重打包。

## 连接

- 驱动 id：`oracle`（连接面板下拉出现）
- 标准五件套：主机 / 端口（默认 1521）/ 服务名（database）/ 用户名 / 口令
- URL 模板：`jdbc:oracle:thin:@//{host}:{port}/{database}`
  （模板在壳内替换；想手工指定完整 URL 可用连接配置的 url_override）

## 无证书验证

没有 Oracle 服务器时，把连接配置的 url_override 指到 H2（`jdbc:h2:mem:rds`），
壳会按 `acceptsURL` 匹配到 lib/ 里的 h2 驱动 —— 协议 / 进程 / 会话 / 查询 / meta
全链路照跑，只是数据面换成嵌入式库。

## 指定自己的 ojdbc 包

不想用 install.ps1 下载的 ojdbc11、或已从 Oracle 官方站点手动下载了驱动 jar 时，
在 `plugin.toml` 的 `[backend]` 加 `driver_jars = ["<jar 绝对路径>"]`（可多个，目录亦可），
重装/热插拔后生效。宿主校验路径存在后经 `RDS_DRIVER_JARS` 环境变量交给 Java 壳加载。
