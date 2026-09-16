# third_party —— 外部预编译产物

本目录只放**不进 git、由脚本可取**的外部二进制。

| 子目录 | 内容 | 取法 | 文档 |
| --- | --- | --- | --- |
| `duckdb/<版本>/` | DuckDB 预编译库（`duckdb.dll` / `duckdb.lib` / `duckdb.h`） | `tools/fetch-duckdb.sh` | `docs/architecture/dependencies/duckdb-linking.md` |

约定：

1. **不要手改这里的文件**：内容由脚本从官方 release 取得，版本目录名即版本号；
2. **路径被构建配置引用**：`.cargo/config.toml` 的 `DUCKDB_LIB_DIR` 指向具体版本目录，
   换版本要同时改那一行（步骤见上述文档 §5）；
3. 新增其它预编译产物前先想清楚：能不能走 crates.io / 系统包？只有「构建期联网不可控 + 体积大 + 与 crate 版本强绑定」
   这三条同时成立时，才值得在这里开一个目录。
