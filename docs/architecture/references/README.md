# 外部参考索引（同类项目：看什么、学什么、能不能抄）

> **本仓不收录这些项目的代码**，只留索引与结论。七个参考仓都拷在**项目外**（`D:\RdataStation\RDS\<仓>-ref\`），
> 与工作区无关、不进 git、各自独立。
>
> 写法规矩（照 `plugin/plugin-dev-plan.md` §3.5 的体例）：**只写看过的**，标出文件与数字；
> 区分「已核实」（读过那个文件）与「推断」（按结构与 README 判断，未逐行核对）。
> 下表所有规模数字都是本轮实测（`find … | wc -l`、`ls … | wc -l`），不是转述。

## 0. 一览

| 参考仓 | 本地目录 | 规模（实测） | 技术栈 | 许可 | 抄代码 |
| --- | --- | --- | --- | --- | --- |
| **navop** | `navop-ref` | 57 crate / 1937 文件 / **776k 行** | Rust + GPUI | Apache-2.0 **+ Navop 补充许可** | ⚠️ 只读思路（补充许可禁商业转售与竞争产品） |
| **zqlz** | `zqlz-ref` | 29 crate / 774 文件 / **413k 行** | Rust + GPUI | GPL-3.0-or-later | ❌ 只读思路 |
| **dbflux** | `dbflux-ref` | 43 crate | Rust + GPUI | **MIT + Apache-2.0** | ✅ 可抄（保留声明） |
| **fluxDB** | `fluxDB-ref` | 7 crate / 223k 行 / 1460 测试 | Rust + GPUI（`gpui-pre` + `gpui-component` 0.6.0） | GPL-3.0 | ❌ 只读思路 |
| **dbui** | `dbui-ref` | 5 crate / 66 文件 / 44k 行 | Rust + GPUI（**限 macOS/Metal**） | **MIT** | ✅ 可抄 |
| **sqlab** | `sqlab-ref` | 44 文件 / 39.5k 行 / 217 测试 | Rust + GPUI + `gpui-component` + tree-sitter-sql | **MIT** | ✅ 可抄 |
| **Rdata-Sidecar** | `Rdata-Sidecar-ref` | Go 原型（v1 侧的 sidecar 尝试） | Go | 本仓作者自有 | ✅ |

对照我们自己的规模：**253k 行 / 2004 测试**（`module-status.md` 口径）。放在这个群里属中等偏大；
**七仓里六个是 GPUI** —— 这条技术路线在同类产品里已反复验证。

## 1. 按模块查

### 1.1 编辑器内核与文本层（我们 `crates/editor`，24.5k 行）

| 看哪里 | 学什么 |
| --- | --- |
| dbflux `crates/fluxdb-editor-core/src/{sum_tree,block_map,display_map,fold_map,inlay_map,tab_map,wrap_map,layer,coordinates,line_index}.rs` + `examples/offline_baseline.rs` + `perf.rs` | Zed 那套**分层 map + SumTree**：折叠/换行/内联提示各是一个 map 层，坐标换算不靠算而是靠层。13.5k 行、独立 crate、可离线跑基线 |
| zqlz `crates/zqlz-text-editor/src/**`（59.8k 行，`buffer.rs`/`cursor.rs`/`display_map.rs`/`folding/`/`find/`/`bookmarks/`） | 同类分层结构的**更大规模实现**；⚠️ 结构上看是 Zed 编辑器移植（未逐行核对），**GPL 血统**，只看结构别抄代码 |
| sqlab `editor/src/**` + 依赖 `tree-sitter-sql` + `sqlparser` + `sqlformat` | 语法高亮用 tree-sitter、格式化用 sqlformat；我们目前没有 tree-sitter（靠语义着色 provider） |

**结论**：我们现在不做折叠/内联，不痛；**要做的第一天先读 dbflux 那份**，别自研坐标换算。

### 1.2 结果网格（只读 / 可编辑）——P4 网格改造的直接参照

| 看哪里 | 学什么 |
| --- | --- |
| dbui `crates/dbui-ui/src/components/grid.rs`（**766 行**做出类型化网格 + 暂存编辑 + 批量编辑） | 网格不必上万行：类型着色、数字右对齐、NULL 与字符串 `"NULL"` 视觉可分、就地编辑 |
| dbui `README.md` 的交互清单 | 一套**可验收的语义**：编辑先**暂存** → 一个事务提交 → 整批丢弃；多行批量编辑用 `MIXED` 标记不一致列，**留 `MIXED` 的字段谁都不写**；整批删除需要主键；外键跟随但**复合键不做**（"一个单元格不是整个键"） |
| sqlab（in-place data editor / table data editor / 结果面板显示列类型） | 编辑器的第二种形态（表数据浏览器），以及"列类型信息就摆在结果面板上" |
| dbflux `crates/dbflux_ui*`（UI 拆成 5 个 crate） | 大 UI 怎么切（与我们 `workbench` + `workbench_shell` 的切法可对照） |

**结论**：P4 动网格时，把 dbui 那份清单**直接当验收表**；"只读连接由服务端强制"这条要抄（见 1.8）。

### 1.3 执行 / 取消 / 事务（`crates/engine/src/driver`、`crates/editor/src/execution.rs`）

| 看哪里 | 学什么 |
| --- | --- |
| dbui `ARCHITECTURE.md` §Threading、§Testing、**§What the live tests caught**、§Known limits | 把"实机测试抓到了什么"写成一节 —— 我们目前散在各自模块的"已知缺口"里 |
| dbflux `docs/DRIVER_RPC_PROTOCOL.md` §Error handling、`DriverCapability::Cancellation` | 取消是**能力位**+独立错误码（与我们 `-32004` + `cancel` 能力同源） |
| dbui `crates/dbui-driver/`（port + adapter） | 驱动 port 极薄、适配器各自实现；与我们 `Database` trait + 能力表路子一致 |

**结论**：这条我们**领先**（真取消 + 错误按域分流 + `-32009` 连接失败码；dbflux 的错误集里既没有结构化 SQL 错也没有连接失败码）。值得补的是 dbui 的"整批提交在一个事务里 + 只读服务端强制"。

### 1.4 导航树与元数据（`crates/database`、`crates/engine/src/driver/introspection.rs`）

| 看哪里 | 学什么 |
| --- | --- |
| sqlab（connection panel + live schema tree + **go to definition** 表/外键/函数） | 树上直接跳转（我们 `#` 内容档 + 定位是同一件事的另一半） |
| dbui（从 `pg_catalog` / `information_schema` / `sqlite_master` 读；视图与**物化视图**都进树） | 与我们 `NavFolder` 的映射口径对照（我们把物化视图并到"视图"） |
| zqlz `crates/zqlz-schema-engine`（1.6k 行）+ `zqlz-schema-tools` | schema 引擎与工具分开；schema 比较（我们还没做） |
| dbflux `docs/DRIVERS.md` | **逐驱动能力矩阵 + 限制清单** —— 正是我们 `driver-capability-matrix.md` 想要的形态 |

### 1.5 驱动层与插件 / 扩展（`crates/plugin`、`crates/engine/src/driver`）

| 看哪里 | 学什么 |
| --- | --- |
| dbflux `docs/DRIVER_RPC_PROTOCOL.md`（403 行）+ `crates/{dbflux_ipc, dbflux_driver_ipc, dbflux_driver_host}` + `examples/custom_driver` | **我们 §4.2 的已出货对照**：能力协商放在握手（`requested_capabilities` ↔ 我们的后置 describe）、**连接表单由服务下发**（`form_definition`，还有动态下拉 `FetchDynamicOptions`）、一请求多帧用 `done` 标记（流式/进度/审计同一个信封）、socket 传输 4B **LE** + bincode / 16 MiB、进程只杀自己拉起的 |
| dbflux `skills/dbflux-rpc-driver/SKILL.md` | 把"怎么写扩展"做成 **agent 技能**（触发条件 → 事实来源 → 关键口径 → 检查清单） |
| navop `crates/extension-*`（api / protocol / runtime / host / wasm / driver / component / view / plugin-adapter）+ `docs/extension-resource-plugins/{architecture,protocol,gpui-shell-extension-design}.md` | **扩展体系最完整的一份**：扩展分类（驱动 / 渲染器 / 导入器 / 编辑器 / **host 渲染的原生资源工作台**）、扩展仓与市场分离、GPUI 外壳里插件视图的边界 |
| sqlab `drivers/{core,postgres,mysql,sqlite,duckdb,databend}` | **每个驱动一个 crate**（我们的原生驱动都在 `engine` 里） |
| zqlz `crates/zqlz-drivers`（54k 行）+ `ARCHITECTURE.md` 的 `zqlz-core` trait 段 | 驱动 trait 的实际形状（`#[async_trait]` 列表）—— 与我们 `Database` trait 逐条对照 |
| dbflux `docs/RPC_SERVICES_CONFIG.md` + README 的 "Settings → RPC Services" | **"填一条命令就能接"的轻入口**（无签名、无包格式）—— 我们 P5 包格式之外的另一条路 |

**结论**：dbflux 那份协议是**必须先逐条对照**的（见 §4 的 R1）；navop 是 P4 之前必读。

### 1.6 类型映射与数据交换（`crates/shared`、`crates/mock`、`crates/editor/src/analysis.rs`）

| 看哪里 | 学什么 |
| --- | --- |
| zqlz `crates/zqlz-interchange/src/{canonical_types,type_mapping,value_encoding,csv_import,csv_export,document,importer,exporter}.rs`（21.9k 行） | **归一化类型（canonical）→ 逐类型映射 → 值编码**三层分工 —— 正是我们 `rds.canonical` + `infer_type`（现仅四档）要长成的样子 |
| sqlab（导出 CSV / JSON / **Excel** / SQL Inserts / SQL Updates / WHERE 子句） | 导出格式清单与"按结果形状决定格式"的口径 |
| dbflux `crates/dbflux_export`、`docs/CHARTS.md` | 按列类型自动判轴（时间轴 / 数值序列）—— 与我们的类型保真同源需求 |

### 1.7 查询分析与执行计划（`crates/insight`、`crates/engine/src/duckdb/explain.rs`）

| 看哪里 | 学什么 |
| --- | --- |
| zqlz `crates/zqlz-analyzer/src/{explain,suggestions}`（6.1k 行） | 计划解析 + **建议引擎**（给用户"这么改"） |
| zqlz `crates/zqlz-explain-visual`（1.4k 行） | 执行计划可视化（我们目前只有文本计划） |
| dbflux `crates/dbflux_driver_*` 的 instance overview | 只读的实例概览 + 指标面板（我们 `insight` 的邻域） |

### 1.8 连接 / 凭据 / 隧道（`crates/connection`）

| 看哪里 | 学什么 |
| --- | --- |
| dbflux（`dbflux_ssh` / `dbflux_proxy` / `dbflux_ssm` + `docs/{CONNECTIONS,PRIVACY,DATA_AND_PRIVACY}.md`） | SSH / SOCKS5 / HTTP CONNECT / AWS SSM 四类通道 + **凭据抽象（provider）** + 隐私文档 |
| navop（SSH/SFTP/FTP 端口转发、X11、Known Hosts 页、导入 SecureCRT 会话） | 通道种类的全集与"可信主机"这类**安全可见性**页面 |
| dbui README（连接存 `~/.config/dbui/connections.json`，**密码只留内存不落盘**） | 与我们凭据口径对照（我们走 Secret/凭据引用） |

### 1.9 设置 / 主题 / i18n（`crates/settings`、`docs/architecture/theme`）

| 看哪里 | 学什么 |
| --- | --- |
| navop `themes/` + `.theme-schema.json` | **主题 schema 化 + 可导入主题**（我们是 token 注册表 + 明暗两套） |
| dbflux `crates/dbflux_i18n/locales/*.yaml` + `docs/TRANSLATIONS.md` | i18n 落成 YAML + Weblate 流程（我们还没 i18n） |
| sqlab README（"no 200-option settings menus"） | 反面对照：设置项克制的产品主张 |

### 1.10 打包与发布（`docs/architecture/release/`）

| 看哪里 | 学什么 |
| --- | --- |
| sqlab `dist-workspace.toml`（cargo-dist 0.31：`installers = [shell, powershell, msi]`、四 target、`install-path`、**`install-updater = true`**）+ `scripts/package-{macos-dmg,flatpak}.sh` | **最省事的一条路**：装包器与自更新器由 cargo-dist 生成，DMG/Flatpak 自己叠 |
| dbui `RELEASING.md` + `packaging/` | macOS 完整剧本：Developer ID → notarytool → 通用二进制 → **自更新（校验后再替换）** |
| fluxDB `.github/workflows/{ci,release}.yml` | 手写路线：Inno Setup（Windows）、`dpkg-deb` 手工打包 + **包内容断言** + `ldd` 依赖报告、跨平台 sha256 |
| navop（DMG/MSI/EXE/ZIP/deb/rpm/AppImage + Scoop/Flatpak + `sha256sums.txt`） | 发行渠道的全集（含包管理器） |

### 1.11 CI 与测试（`.github/workflows/`、`module-status.md`）

| 看哪里 | 学什么 |
| --- | --- |
| fluxDB `.github/workflows/ci.yml` | 三平台原生 runner（macos-14 / windows-2022 / ubuntu-22.04）+ **GPUI/Linux 依赖的完整 apt 清单** + release 构建排在测试之前 + `--locked` + `Swatinem/rust-cache`；**按 crate 分层跑测试**（业务 crate 不依赖 gpui，所以能在 CI 跑） |
| dbflux `tests/driver-live/README.md` | **testcontainers**：每个测试起独立容器、动态端口（PG/MySQL/Mongo/Redis/DynamoDB Local/MSSQL/ClickHouse），live 用例 `#[ignore]` + `--ignored`；nextest；`deny.toml`（依赖审计）；`cliff.toml`（changelog） |
| dbui `docker-compose.yml` + README | `postgres:17-alpine` → **55432**、`mysql:8` → **53306**（非默认端口不撞车）+ healthcheck，门控 `DBUI_LIVE_TESTS=1`，参数 `DBUI_PG_*` 可覆盖 —— **可以直接抄** |
| sqlab `.githooks/pre-commit` + `AGENTS.md` | 收尾一把跑 clippy / test / fmt 的钩子 |

### 1.12 文档与 agent 工作流（`docs/`、`.agents/skills`）

| 看哪里 | 学什么 |
| --- | --- |
| zqlz `ARCHITECTURE.md`（§Crate Reference 逐 crate、§Data Flow Patterns、**§Where to Edit What 快速表**、§Common Development Tasks）+ `.claude/skills/`（15 个：`gpui-action` `gpui-async` `gpui-context` `gpui-element` `gpui-entity` `gpui-event` `gpui-global` `gpui-focus-handle` `gpui-layout-and-style` `gpui-style-guide` `gpui-test` `new-component` …）+ `.claude/COMPONENT_TEST_RULES.md` | **"改哪里"的索引入库**；GPUI 技能拆得比我们细（我们只有一个 `gpui-kit-dev`，其中"测试"与"焦点"若独立成技能会更可检索） |
| dbflux `skills/*/SKILL.md` | 技能四段式：何时用 / 事实来源（列文件路径）/ 关键口径 / 检查清单 —— 我们的 `.agents/skills` 可直接采用这个骨架 |
| sqlab `AGENTS.md`（3 条：收尾必跑 clippy+test+fmt / 本地有 `../gpui-component` fork 可改 / 新快捷键要进系统菜单）+ `docs/llms.txt` | 极简仓库规则 + 面向 agent 的 `llms.txt` |
| dbui `ARCHITECTURE.md` 的 §Decisions worth knowing / §What the live tests caught / §Known limits | 三节结构：**决策理由 / 实机测试抓到了什么 / 已知边界**（我们的"已知缺口"可以提成同款三节） |

### 1.13 演示与手工验收资产

| 看哪里 | 学什么 |
| --- | --- |
| dbui `docs/demo.sql` + README 的"storefront"剧本 | 一份**像真业务的演示数据集**（截图、手工验收、录屏都用它）；我们的 mock 生成器是造数，这个是"业务剧本" |
| sqlab `docs/`（GitHub Pages + `llms.txt` + `sitemap.xml`） | 文档站点与 agent 入口一起发 |
| navop `docs/` + `docs-site/` | 文档站与产品文档分离 |

## 2. 跨项目的共同结论

1. **GPUI 是可行路线**：七仓里六个 GPUI（含一个 776k 行、一个 413k 行的），都在 macOS/Windows/Linux 出货或接近出货。
2. **只有我们做 Arrow 数据面**：dbflux 用 bincode、navop/sqlab/zqlz 用 JSON（dbflux 的 bincode 只在"两端都是 Rust"时成立）。我们的"sidecar 可以是任何语言"这条主张，正对应 Arrow 的价值。
3. **"能力缺失必须有处表达"是共同做法**：dbflux 握手期能力位集 / navop 扩展清单 / sqlab 功能状态表 / 我们 `-32006` + 宿主侧门控。
4. **测试分层是共同做法**：业务 crate 不依赖 UI → 能在 CI 跑（fluxDB 的 `cargo test -p …` 分列）；实机/容器测试单独门控（dbui `DBUI_LIVE_TESTS`、dbflux `--ignored` + testcontainers、我们 `RDS_SIDECAR_*`）。
5. **分帧不约而同**：dbflux 与 navop 都是 **4 字节小端长度 + 载荷**、上限 16 MiB；我们是 **4 字节大端（含头）+ kind 字节**、64 MiB。差在字节序与数据面，跨实现互通时要明说（我们 §4.2.1 已写死）。

## 3. 从他们身上看出来的坑（别踩）

| 坑 | 依据 |
| --- | --- |
| **宿主崩溃时远程进程谁收摊**：dbflux 只管"优雅退出时 kill 自己拉起的"，崩溃这条没写 | `docs/DRIVER_RPC_PROTOCOL.md` §Process lifecycle and cleanup |
| **socket 传输没有 stdin EOF 这类天然心跳**：服务端不容易知道宿主已死 | 同上（我们的 stdin EOF 约定正好补这一格） |
| **GPL 血统会传染**：zqlz 的编辑器疑似 Zed 移植（GPL），抄结构可以、抄代码会把我们拖成 GPL | zqlz `LICENSE` = GPL-3.0-or-later + `zqlz-text-editor` 目录结构 |
| **本地 fork 组件库是双刃剑**：sqlab 直接 fork `gpui-component` 以便随手改（`AGENTS.md` 明说）；我们锁 0.6.1 不动 —— 升级干净但改不动 | sqlab `AGENTS.md` |
| **"极简设置"也是产品选择**：sqlab 明确不做 200 项设置菜单；我们设置页有"准入五条 + 退役清单"，方向一致 | sqlab README |

## 4. 从这些参考里挑出的可落地项

| # | 事项 | 依据 | 成本 | 依赖 |
| --- | --- | --- | --- | --- |
| **R1** | 拿 dbflux `DRIVER_RPC_PROTOCOL.md` **逐条对照**我们 `plugin-dev-plan.md` §4.2，补三处：能力协商提前到握手、连接表单由驱动下发、一请求多帧 `done` 标记 | §1.5 | 低（写文档 + 改口径） | 无 |
| **R2** | 加"**填一条命令就能接**"的轻入口（对照 Settings → RPC Services），让用户不必等 P5 包格式 | §1.5 | 中 | 方向待定（是否保留 P5 签名/包格式） |
| **R3** | 把插件作者指南做成 `.agents/skills/rds-plugin-author/SKILL.md`（四段式：何时用 / 事实来源 / 关键口径 / 检查清单） | §1.5、§1.12 | 低 | 无 |
| **R4** | 抄 dbui 的 `docker-compose.yml`（PG 55432 / MySQL 53306）+ 门控环境变量，把 P1/P2 的实机验收变成"有 Docker 就能跑" | §1.11 | 低 | 需要 Docker（本机暂无） |
| **R5** | CI 里按 crate 分层跑**无 UI 依赖**的测试（`rds-plugin` / `rds-engine` / `rds-shared` / `rds-paths` / `rds-mock` / `rds-insight`） | §1.11 | 低 | 无（本机可先跑通命令） |
| **R6** | P4 动网格前，把 dbui 的交互清单落成我们的验收表；"只读连接服务端强制"纳入设计 | §1.2、§1.8 | 低（文档） | P4 |
| **R7** | 类型映射三层（canonical → 逐类型映射 → 值编码）参照 zqlz `zqlz-interchange`，把 `infer_type` 四档扩成真映射（含 Arrow schema 那一份） | §1.6 | 中 | P2.5 接线 |
| **R8** | `docs/architecture/README.md` 补一节"改哪里"的快速表（照 zqlz `ARCHITECTURE.md` §Where to Edit What） | §1.12 | 低 | 无 |
| **R9** | 评估用 **cargo-dist** 生成安装包与自更新器（对照 sqlab `dist-workspace.toml`），替代/补充手写 `.github/workflows/release.yml` | §1.10 | 中 | 发布策略待定 |
| **R10** | 加一份**演示数据集 + 业务剧本**（对照 dbui `docs/demo.sql`），给截图、手工验收与录屏用 | §1.13 | 低 | 无 |

## 5. 逐模块对比（我们 vs 他们）

判定口径：**领先** = 我们有他们普遍没有的；**持平** = 都有，做法不同；**落后** = 他们普遍有而我们没有。
我们的现状都标了依据（本轮实测的 grep / 文件），不是印象。

| 模块 | 他们 | 我们现状（依据） | 判定 |
| --- | --- | --- | --- |
| 编辑器内核与文本层 | dbflux `fluxdb-editor-core`（SumTree + 分层 map + `perf.rs`）；zqlz `zqlz-text-editor`（59.8k 行，Zed 移植结构）；sqlab 用 tree-sitter-sql 高亮 | `crates/editor` 24.5k 行，**无 fold / inlay / display-map 分层**；高亮走语义着色 provider（`lsp-types` 的 `proposed`），补全自研 | **落后**（折叠/内联/坐标分层）；高亮路线不同（tree-sitter vs 语义 token） |
| 结果网格（只读） | dbui `grid.rs` 766 行；dbflux 称 virtualized data table | `crates/editor/src/view/results/grid.rs` 已有；**1000 行 × N 列全量 `to_string()` 仍是待改项**（P4/Q6） | **落后**（虚拟化与惰性格式化没做） |
| 结果网格（编辑回写） | **六仓全有**：dbui 暂存→一个事务提交、`MIXED` 批量编辑、外键跟随；fluxDB 数据编辑回写；sqlab in-place editor | 全仓搜不到编辑回写（`写回` 的命中都是 `analytics_resource` 的**文件版本**语义，不是写库）；导出的 `INSERT` 是只读产物 | **落后**（这一块我们等于没开） |
| 执行 / 取消 / 事务 | dbflux 有能力位 `Cancellation` + `Cancelled`，但**没有连接失败码、没有结构化 SQL 错**；dbui 整批提交在一个事务里、**只读由服务端强制** | 真取消（令牌递到驱动进程，等它以 `-32004` 收场）+ 错误按域分流 + `-32009 connect_failed` + SQL 错带 `sqlstate` 与字段位置（`sidecar/driver.rs`） | **取消与错误分类领先**；**显式事务 UI 与只读强制待补** |
| 导航与元数据 | sqlab live schema tree + go-to-definition（表/外键/函数）；dbui 物化视图也进树；zqlz `zqlz-schema-engine` + `zqlz-schema-tools`（schema 比较）；dbflux `docs/DRIVERS.md` 矩阵 | 三层缓存（L1 内存 / L2 连接级 SQLite / L3 词表）+ 内省级别自适应 + 五文件夹 + 属性面板 + `#` 全文档档（`docs/architecture/data-layer-wiring-matrix.md`） | **持平偏领先**（缓存与台账）；**缺 schema 比较、缺树上跳转** |
| 驱动接入 | 六仓各有一套 in-process trait + 能力声明；dbflux 另有外部驱动 RPC；navop 有扩展市场 | 原生 6 驱动 + 联邦 scanner + `Database` trait；插件（sidecar/wasm）在建（M9） | 驱动**广度落后**（他们有 Redis/Mongo/MSSQL/Oracle/InfluxDB/S3…，我们 SQL 为主）；**扩展机制领先**（Arrow 数据面 + 版本闸 + 宿主门控 + 进程边界，七仓里唯一） |
| 类型映射 | zqlz `zqlz-interchange`（canonical → type_mapping → value_encoding，21.9k 行）；sqlab 导出按类型 | `infer_type` 只产 4 档（BIGINT/DOUBLE/BOOLEAN/VARCHAR）；`rds.canonical` 口径已在 dev-plan §4.5.1 定，**读侧未接** | **落后**（P2.5/P4 已排期） |
| 查询分析与计划 | zqlz `zqlz-analyzer`（6.1k，含 suggestions）+ `zqlz-explain-visual`（1.4k）；dbflux CHARTS/DASHBOARDS；navop 监控面板 | `crates/insight`（质量分/建议）+ `engine/src/duckdb/explain.rs` 的 `ExplainAnalyzer`（结构化计划树，含节点类型/深度） | 分析**领先**（质量与建议）；**计划可视化落后**；**图表/仪表盘我们没有**（也不是我们的方向） |
| 导入 / 导出 | sqlab CSV/JSON/Excel/SQL Inserts/**Updates**/WHERE 子句；zqlz interchange 含 csv import；dbflux shape-based export | `editor/src/export.rs`：CSV / JSON / INSERT / **Parquet** / **Xlsx**（后两者走 DuckDB `COPY`）；`analytics_resource` 是**文件型数据源 + 版本历史** | 导出**持平偏领先**（Parquet 七仓里只有我们）；**导入落后**（没有导入管线）；数据资源带版本是差异点 |
| 连接 / 凭据 / 隧道 | dbflux SSH/SOCKS5/HTTP CONNECT/**AWS SSM** + 凭据 provider + 隐私文档；navop 端口转发/X11/**Known Hosts 页**；dbui 密码只留内存 | `crates/connection` 4.7k + `TunnelRegistry` + 凭据 Secret 引用 + 项目/全局两层作用域 | **持平**（隧道已有）；缺代理/SSM 类与企业级 provider 抽象 |
| 设置 / 主题 / i18n | navop `themes/` + `.theme-schema.json`（**可导入主题**）+ 三语；dbflux `dbflux_i18n/locales/*.yaml` + Weblate；sqlab 明确不做 200 项设置 | `rds-theme` token 注册表 + 明暗两套 + 设置准入五条/退役清单（`settings-architecture.md`） | 主题**持平**（我们语义角色更严格）；**i18n 完全没有 → 落后** |
| AI / MCP | dbflux 内置 MCP server + 治理（操作分级/策略/人工审批）；navop MCP + Agent Hub + ACP 接 Codex/Claude/OpenCode + `@navop/cli`；sqlab 终端面板支持编码 agent | 全仓搜不到 `mcp` / `llm` / `openai` 任何实现 | **落后**（我们自身是 AI 开发的项目，这条缺得显眼） |
| 打包与发布 | sqlab **cargo-dist**（shell/powershell/msi + 自更新器）；dbui 公证 DMG + 自更新；fluxDB Inno + 手工 deb（含包内容断言）；navop 全平台 + Homebrew/Scoop/Flatpak | `.github/workflows/release.yml`：四平台 `dist/*.zip` + 校验和 → Release Assets（**无安装器、无自更新、无包管理器**） | **落后**（能用但不成话） |
| CI 与测试 | fluxDB 三平台按 crate 分层**跑测试** + Linux 依赖清单；dbflux nextest + **testcontainers** 实机 + `deny.toml`；dbui 真窗口 UI 测试 + docker-compose；sqlab pre-commit 钩子 | `ci.yml` 只跑 `clippy-all`（**有意不跑测试**，理由写在文件头）；2004 测试全在本机（`module-status.md`） | **落后**（测试没进门禁；无容器化实机测试） |
| 文档与 agent 工作流 | zqlz `ARCHITECTURE.md` 的 **Where to Edit What** + **15 个细粒度技能**（`gpui-test`/`gpui-focus-handle`/`gpui-global`/`gpui-style-guide`…）；dbflux `skills/*/SKILL.md` 四段式（含**扩展作者**技能）；dbui 三节（决策/实机测试抓到什么/已知边界） | 台账三件（`module-status` / `data-layer-wiring-matrix` / `driver-capability-matrix`）+ 逐模块 dev-plan/映射表 + 5 个技能（架构/布局/主题/UI 规格/GPUI 开发） | 文档纪律**领先**；**技能粒度落后**（无测试技能、无扩展作者技能） |
| 演示与手工验收资产 | dbui `docs/demo.sql`（storefront 剧本） | `mock` 造数 + `scratchpad`；**无业务演示数据集** | 持平偏落后（缺剧本） |

### 5.1 结论：我们领先的四块与落后的六块

**领先（守住）**：① **扩展机制**（Arrow 数据面 + 版本闸 + 宿主门控 + 进程边界，七仓唯一）；② **联邦**（DuckDB + sqlglot 方言转译，七仓都没有）；③ **分析与造数**（`insight` 质量分/建议 + `mock` + 文件型数据源带版本）；④ **文档纪律**（台账 + 映射表 + 实机/未核实分标）。

**落后（按性价比排追赶序）**：① **测试进 CI**（按 crate 分层即可，零新依赖）；② **打包装包器与自更新**（cargo-dist 一条路）；③ **编辑回写**（体量最大，且要先有 P4 的网格改造）；④ **类型映射**（P2.5 已排）；⑤ **i18n**（可以晚）；⑥ **AI/MCP**（要产品决策，不是补课）。

> 注：**六仓全都有编辑回写**，而我们是七仓里唯一没有的 —— 这不是「待优化」，是产品能力缺口，值不值得补要先定。
## 6. 维护约定

- 新增参考仓时：**拷到项目外**、在 §0 表里加一行（含许可与"能不能抄"）、在 §1 里给出**至少一个具体文件路径**。
- 只写**看过的**；推断要标"疑似/未核实"。
- 结论要能被引用：写清 `仓/文件` 或 `仓/文件:行`，别写"某项目据说……"。
