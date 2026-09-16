# rds-paths

运行时数据路径的**唯一解析点**。设计文档：`docs/architecture/runtime/data-paths.md`。

## 一句话

这个软件生成的任何信息（配置 / 数据 / 日志 / 临时 / 扩展）都待在软件自己的目录下，
默认 = **可执行文件所在目录**（安装目录）；路径解析只在这一个 crate 里发生。

```
<RDS_HOME>/
├── config/       settings.json
├── data/         global.db / analytics.duckdb / 密钥库 / samples
├── logs/         app.YYYY-MM-DD
├── tmp/          DuckDB spill / 联邦临时库 / 进程 scratch
└── extensions/   DuckDB 扩展
```

## 用法

```rust
paths::config_dir();          // 全局设置
paths::data_dir();            // 全局数据（含密钥库）
paths::log_dir();             // 日志
paths::temp_dir();            // 临时（RDS_TEMP_DIR 可单独覆盖）
paths::extensions_dir();      // DuckDB 扩展
```

启动契约：`crates/app/src/main.rs` 第一条语句调用 `paths::install_process_temp_dir()`
（重定向 `TEMP`/`TMP`/`TMPDIR`，一次性覆盖所有 `std::env::temp_dir()` 调用点），
紧跟 `paths::migrate_legacy_layout()` 做旧布局的一次性迁移。

## 边界

- **不**管用户项目资产（`<用户项目>/{project.db, analytics.duckdb}`），那属于 M1 项目会话。
- **不**管 `~/.ssh/known_hosts`（跨应用共用的用户资产，需要时用 `RDS_KNOWN_HOSTS` 覆盖）。
- 插件目录 / 插件数据目录属三期（P3-a），见 `docs/architecture/plugin/plugin-architecture.md`。
