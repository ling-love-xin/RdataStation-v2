# RDS beta 0.2.15

> 2026-11-28 · 相对 `RDS-beta-0.2.14` 的 183 个提交 · beta：本版只出 Windows 包

## 本次重点

- **分析会话收成「一条源只出一行、一条会话一个 owner」**：会话行并入连接行，源段按可用性分两段（参与分析 / 可加入）；底座「卸载」从 `DETACH` 改成会话视角遮蔽，临时结果表按 owner 登记，加速档 owner 收敛到 `accel:{conn_id}`。
- **catalog 事实面落进 engine**：一个别名一条事实，数据对象面板改读 engine 的文件索引，`Shared.loaded_files` 整条删除；`use_duckdb_fed` 字段删除，通道门控改看「能力 + 登记清单」。
- **SQL 发送前预检**：发送前抽表引用、列多源候选、校验别名占用；别名冲突改「拒绝 + 点名占用者」，不再用 `_2` 让位；有人在用的别名拒绝 `DETACH`。
- **分析资源补全局文件 CAS 与表快照闭环**：文件资产按 CAS 发布 / 取回 / 归档，全局文件版本链改稳定父链；表资产改为冻结快照（元数据表 + manifest + 快照索引），全局引用按资产 id 登记。
- **回收站收成唯一入口**：只从草稿箱栏进入，清单合并项目 / 全局三路来源，清空同时清两侧；文件只能解锁退回草稿箱，表型分逻辑删除与永久删除。
- **结果集网格继续按 DBeaver 真值收紧**：行高 26→20、表头 26/40→28/50；列头排序图标改自绘 SVG、三角槽宽 28→19、获焦列四边框；多维数组逐级展开与子行行号缩进。
- **工具链钉到 `1.100.0`**：`rust-toolchain.toml` 与 CI / 发版工作流三处对齐（clippy job 显式装组件），并把 cargo 1.100 新点名的 14 条 unused dependency 告警清干净。

## 分析会话与引擎

- engine 新增 catalog 事实面（一个别名一条），面板改读 engine 的文件索引，`Shared.loaded_files` 删除。
- 源记录唯一：会话行并入连接行，一条源只出一行。
- 源段按可用性分两段：参与分析 / 可加入。
- SQL 发送前预检：表引用抽取 + 多源候选 + 别名校验；连接级临时表看不见时不再误判「表不存在」。
- 别名冲突改「拒绝 + 点名占用者」，删掉 `_2` 让位路径。
- 引用计数守 `DETACH`：还有人在用就不许摘。
- 主源进意图清单（`analysis_primary` 单行表），组装会话时 replay；headless 重启探针验收意图清单回放。
- 临时结果表登记 owner；底座「卸载」落成会话视角遮蔽（不 `DETACH`）。
- mock 落地成功即收临时表；加速档 owner 收敛到 `accel:{conn_id}`。
- 删掉 `use_duckdb_fed` 字段：通道门控改「能力由驱动常量声明 + 由登记清单启用」。
- 外部源执行计划认 `*_QUERY` 整句下推；`ExplainAnalyzer` 改走 `EXPLAIN (FORMAT JSON)`，跨源扫描可判下推。
- `Decimal256` 直灌前在 Arrow 层降级 `DOUBLE`，不再拒收。
- 密钥管理器用过一次后不再因 `secret_directory` 配置失败。

## 分析资源与资产库

- 全局文件资产：CAS 发布与取回、归档与元数据往返。
- 全局文件版本链按全局名聚合、改稳定父链；全局版本历史支持恢复旧版本。
- 版本历史支持任意两版对比（含表版本对比）。
- 表资产改冻结快照闭环：快照元数据表、manifest、快照索引三件套。
- 全局引用改按资产 id 登记，快照带挂载档；全局表只走快照闭环。
- 回收站唯一入口（草稿箱栏打开资产库回收站），清单显示项目与全局条数、全局行标来源版本；撤掉绕过回收站的删除入口。
- 回收站清单合并三路来源，`global` 前缀路由还原与永久删除，清空同时清两侧。
- 还原 / 补登前校验显示名唯一（项目回收站、全局回收站、索引补登三处）。
- 资产删除语义收口：文件只能解锁退回草稿箱；表型分逻辑删除与永久删除。
- 表归档身份收口并补物理快照补偿；详情与取回对话框显示活跃取回。
- 拆掉文件归档保留策略旧链路，保留设置收窄到全局表版本。

## 结果集面板

- 网格密度与表头带高按 DBeaver 真值：行高 26→20，表头 26/40→28/50。
- 列头排序图标改自绘 DBeaver SVG；三角槽宽 28→19；排序箭头补手形光标。
- 获焦列补四边框（vendor 未动）。
- 多维数组逐级展开：层级键改造，子行行号逐级缩进（对齐 `level * LEVEL_SPACING`）。
- 行号槽宽按实测只增不减（对齐 `computeHeaderSizes`）。
- 列头右键补「面板 ▸」子菜单。

## 编辑器

- 执行计划回执展平成计划表（审计 P1-5）。
- 预检看不见连接级临时表时不再误判「表不存在」。

## 草稿箱

- 剪贴板粘贴闭环；导入改应用内弹窗。
- 书签扫描读文件旁的 sidecar，补回归用例。
- 示例草稿真正落进草稿箱（`sample_drafts` 从死字段接成真行为）。

## 工作台与启动

- 启动默认进空态（欢迎页）；「恢复上次项目」接成真开关。
- 欢迎页补「从示例项目开始」入口。
- 示例项目自带双数据源（SQLite + DuckDB，由当前内核生成）。
- 启动路径的项目状态必须看得见：只读、恢复失败、名册预载。

## 数据对象面板

- 段头常驻，空段也占位（源 / 表 / 文件）；段头换共享 `tree::GroupHeader`。
- 挂载按钮判据回到「能连」，不再被登记挡住。
- 面板挂载事实改读分析实例，联邦源不再依赖死缓存。
- 会话角色改按挂载事实判，不再看 label。

## 驱动

- MySQL 原生驱动按列声明类型出 Arrow：INT / DECIMAL 不再退化成字符串。
- `mysql_native` 兼容 sqlx 下发来的 SSL 参数（`ssl-mode`）。

## 工程与工具链

- `rust-toolchain.toml` 钉到 `1.100.0`，CI / 发版工作流三处对齐；clippy job 显式安装 clippy 组件（具名工具链按 minimal 安装不自带）。
- 清掉 cargo 1.100 新点名的 unused dependency 告警：14 条（`shared` 9、`file-meta` 2、`connection` / `driver` / `engine` 各 1），工作区里随之失去引用的 `dashmap` / `x509-parser` 条目一并下线。
- `cargo test` 默认串行（`.cargo/config.toml` 新增 `RUST_TEST_THREADS=1`）：`paths` 的测试数据根按进程隔离（`rds-test-root-<pid>`），同一测试二进制里的带库用例并行会互抢 DuckDB / SQLite 文件——默认并发下 10 个 target / 48 条失败（`Catalog write-write conflict`、`Duplicate key "version: 1"`、`Cannot open file … 另一个程序正在使用此文件`），串行后全量 159 个 target / 5002 条用例全绿。要并发时命令行显式 `cargo test -- --test-threads=<N>`。
- 门控脚本补两处会「撒谎」的洞（包名逗号、没跑出结果时假绿）。
- `analysis_kernel_probe` 用例间不再抢 `rds_src`；底座指纹测试的错断言修正（「无底座」指纹是 `-`，不是空串）。

## 本版验证

- `cargo check-all`：0 error / **0 warning**（本版清掉的 14 条 unused dependency 告警即此处的来源）。
- `cargo clippy-all`：exit 0、**0 error**；存量风格 lint 仍在千条量级（仓内 `release-pipeline.md` / `hardening-plan.md` 已记为「存量债、幂清后加 `-- -D warnings`」，CI 因此不设硬门禁）。
- `cargo test --workspace`（串行）：**159 个 target / 5002 条用例通过、0 失败**；两个真库探针（`probe_duckdb_extensions` / `probe_and_connect_all_fixtures`）按既有口径排除。


## 下载与校验

| 平台 | 文件 | 说明 |
| --- | --- | --- |
| Windows x86_64 | `rds-app-0.2.15-windows-x86_64.zip` | 解压即用，含 DuckDB 2.0.1 动态库与 `assets/` |
| 校验 | `rds-app-0.2.15-windows-x86_64.zip.sha256` | `Get-FileHash -Algorithm SHA256` 或 `sha256sum -c` |

- **未签名**：首次运行会有 SmartScreen 提示，选「更多信息 → 仍要运行」。
- macOS / Linux 包在定版（`v*`）标签里出，beta 只构建 Windows。
- 本版与 `0.2.14` 的库兼容：DuckDB 仍是 2.0.1（`storage version 69`），旧版 1.5 内核依旧打不开新库。

## 已知问题

- beta 只出 Windows 包；Linux / macOS 需要等定版标签。
- 未签名、未公证：Windows SmartScreen 会拦一次。
- 驱动能力靠插件：Oracle / SQL Server 需要装对应的驱动插件包，且两版 Oracle 互斥（默认装 Rust 版）。
- 主题包是 opt-in，需要在外观设置里手动选择才会启用。
- 整项目导出、文件引用、全局版本回退 UI 后置。
- `cargo fmt --check` 仍不作闸门：存量文件有 rustfmt 漂移，本仓只格式化本轮 hunk。

**完整对比**：https://github.com/ling-love-xin/RdataStation-v2/compare/RDS-beta-0.2.14...RDS-beta-0.2.15
