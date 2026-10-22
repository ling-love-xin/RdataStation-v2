# 驱动插件包（tools/plugin-drivers）

「一个通用壳 + 每库一个插件包」的落地目录。插件包 = manifest（`plugin.toml`）+
驱动资产（jar / python 壳），**不内嵌 Java / Python 运行时**：解释器由宿主
`crates/plugin/src/runtime.rs` 三级查找（用户指定 → 环境变量 / PATH → third_party）。

## 结构

```
tools/plugin-drivers/
├── oracle-jdbc/     # rds.driver.oracle：通用 JDBC 壳 + ojdbc11（+ h2 无证书验证）
├── postgres-py/     # rds.driver.postgres-py：Python DB-API 壳 + psycopg（用户环境）
└── install.ps1      # 构建壳 / 下载驱动 jar / 安装到 RDS_HOME/plugins/<manifest-id>
```

## 使用

```powershell
powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\install.ps1
```

产物原则：驱动 jar / 壳 jar 不入库（各包 `.gitignore` 忽略 `lib/`），全部由脚本
运行时组装（Maven Central 下载 + 壳构建）。离线环境用 `-SkipOjdbc` 跳过下载。

## 新增一个 JDBC / DB-API 数据库驱动

1. 复制一个现有插件包目录（如 `oracle-jdbc`）；
2. `plugin.toml`：改 `plugin.id` / `name` / `license`；`contributes.drivers` 改
   `id`（连接面板下拉显示的驱动 id）/ `display_name` / `default_port` /
   `url_template`（占位符 `{host}` `{port}` `{database}` `{user}` `{password}`，
   壳内替换）；
3. 驱动 jar 放入 `lib/`（或 Python 侧改 `rds_dbapi_shell.py` 的适配器）；
4. `install.ps1` 的 `$targets` 表加一行（src 目录名 + manifest id）。

壳零改动：JDBC / DB-API 2.0 是标准接口，协议 / 执行 / meta 映射已在壳内一次实现。

## 许可提醒

- ojdbc11：OTN（Oracle Technology Network）许可，见 `oracle-jdbc/README.md`；
- H2：MPL 2.0 / EPL 1.0；psycopg：LGPL / MIT（随用户环境）。
