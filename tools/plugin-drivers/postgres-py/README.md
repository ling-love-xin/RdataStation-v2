# rds.driver.postgres-py — PostgreSQL（Python）驱动插件

通用 DB-API 2.0 壳（`tools/rds-dbapi-shell/rds_dbapi_shell.py`）的打包形态。
与 Java 侧完全同构：一个壳 + 每库一个插件包（manifest 声明 + 驱动适配），壳零改动。

## 运行时要求

- **Python 3**。宿主按以下顺序找解释器（`crates/plugin/src/runtime.rs`）：
  1. 用户显式指定的 `python` 路径（插件设置，可指虚拟环境）；
  2. PATH 里的 `python3` / `python`；
  3. `<仓库根>/third_party\python\python.exe`（若存在则用并告知；自动下载二期）。
- **PostgreSQL 驱动由用户环境提供**（壳不内嵌）：
  `pip install psycopg2-binary`（或 `psycopg[binary]`，psycopg 3）。
  壳 import 失败时返回可读报错并提示安装命令，不会静默失败。

## 连接

- 驱动 id：`postgresql`（连接面板下拉出现）
- 标准五件套：主机 / 端口（默认 5432）/ 库名 / 用户名 / 口令
- URL 模板：`postgresql://{user}:{password}@{host}:{port}/{database}`
  （模板在壳内替换；想手工指定完整 URL 可用连接配置的 url_override）

## meta 能力

- catalogs：PG 单库无 catalog 维度 → 空（宿主走 schema 层）
- schemas：`pg_namespace`（排除系统 schema）
- objects：表 / 视图（information_schema）+ 例程（procedure / function）
- object_detail：列 + 主键 / 外键（pg_constraint）+ 默认值

## 无证书验证

没有 PostgreSQL 服务器时，把连接配置的 url_override 指到 SQLite 文件（或
driver 选 sqlite 用 file_path）—— 壳的 sqlite 适配器（标准库）跑通协议全链路，
行为与 psycopg 分支共用同一套 DB-API execute 与分帧实现。
