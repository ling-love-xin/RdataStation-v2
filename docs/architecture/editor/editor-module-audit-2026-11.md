# 编辑器模块审计与设计主张（2026-11-27）

> 范围：`crates/editor`（含其对外端口与 `crates/result_panel` 的边界）。
> 性质：**只读审计 + 设计主张**，本轮未改任何代码。
> 口径：全部结论来自本地源码/文档实测（命令与产物见 §8）；无法实测的部分在 §7 明确标为未验证，不做推测。

---

## 0. 结论先说

1. **功能完成度高于文档所述**。`crates/editor/README.md:29` 仍写「2b / 1c / 2d 未开工、`notebook.rs` 尚不存在」，而 `lib.rs:21/24/105` 与代码目录（`notebook/` 7 文件 2,366 行 + `view/host/notebook.rs` 1,157 行）都表明 2b/2d 已全量落地。当前真实形态是：**三大模式的功能骨架都齐了，缺的是边角功能与验收**，不是"没开工"。
2. **工程质量纪律属优良档**：`unsafe` 0、`TODO/FIXME/todo!()` 0、生产 `panic!` 0、生产 `unwrap/expect` 仅 19 处；**本轮实测门禁 886 passed / 0 failed / 4 ignored**（HEAD `70e6bdfd`，68.15s）。
3. **真正的问题不在代码质量，在三个结构维度**：
   - **状态所有权**：`EditorHostPanel` 一个结构体 76 个 `pub(crate)` 字段、跨 38 个文件 458 个 `fn`，任何域都能写任何域的字段；
   - **编译边界**：`editor` 已膨胀成一个"迷你应用"（23 个直接依赖，含 `duckdb` / `rfd` / `scratchpad` / `file_meta`），`ports/` 6,927 行是从 workbench 下沉进来换编译时间的应用服务；
   - **模式语义唯一性**：文档说「能力表是唯一分叉点」，实测 `EditorMode` 判定散在 **10 处**，其中 `view::fold::is_enabled` 与 `view::highlight::is_enabled` 是对 `Capabilities::folding` 的重复实现——这套重复已经真实引发过一次三处口径不一致（§6 P0-2）。
4. **继续拆文件已到边际**：`view/host/` 从 1 个大文件拆成 38 个，`mod.rs` 仍有 717 行 + 76 字段的巨型结构体定义。**下一刀不该再按"文件"切，该按"状态所有权"和"编译边界"切。**
5. 我的设计主张排序：先做**便宜且能自证**的两件（模式单一分叉点 + 文档漂移守护测试），再做编译边界三段拆分，最后才动 `EditorHostPanel` 的状态分组与结果域收敛。**不建议**一步到位把 76 字段拆成 4 个 Entity。

---

## 1. 规模与分布（实测）

`crates/editor/src` 共 **206 个 `.rs` / 67,583 行**；其中生产代码（首个 `#[cfg(test)]` 之前）**42,794 行**，测试代码约 **24,789 行（36.7%）**。

| 域 | 文件 | 行数 | 占比 | 职责（据各 mod.rs 文档注释） |
|---|---:|---:|---:|---|
| `view` | 92 | 36,127 | 53.5% | 面板渲染 / 组件 / 测试（其中 `view/host` 14,093、`view/tests` 13,940 / 20 文件） |
| `ports` | 25 | 6,927 | 10.3% | 宿主端口实现（连接/执行/补全/schema/会话/kernels/LSP/文件对话框），P2-a 从 workbench 下沉 |
| `support` | 19 | 6,350 | 9.4% | 书签/折叠/编码/格式化/历史/持久化/工具栏/转译…（杂项集合） |
| `exec` | 19 | 4,744 | 7.0% | 目标解析 / 队列 / 通道 / 预检 / 回执 |
| `lsp` | 11 | 3,141 | 4.6% | LSP 客户端 / 桥接 / hover / definition / code action / frame |
| `notebook` | 7 | 2,366 | 3.5% | 分析模式数据层（cell / controller / kernel / media / orchestrate / session），**零 GPUI** |
| `core` | 5 | 2,210 | 3.3% | 文档模型 / 模式 / 能力表 / 服务层 / 档位 |
| `language` | 7 | 2,107 | 3.1% | 补全管线 / 语言服务 |
| `shared` | 8 | 1,918 | 2.8% | `EditorShared` 端口槽位 / 跨面板共享状态 |
| `quick_open` | 11 | 1,418 | 2.1% | 纯逻辑 + 模型 |
| 根（`lib.rs` / `host.rs`） | 2 | 275 | 0.4% | 入口 / 端口 trait |

`view/host` 明细（14,093 行，占全 crate 20.8%）：

| 子域 | 文件 | 行数 |
|---|---:|---:|
| `results/` | 12 | 3,634 |
| `exec/` | 8 | 2,128 |
| `toolbar/` | 7 | 1,520 |
| 根文件（`notebook.rs` 1,157 / `render.rs` 903 / `construct.rs` 803 / `session.rs` 793 / `mod.rs` 717 / `format.rs` 591 / `ops.rs` 411 / `editor_body.rs` 405 / `bookmarks.rs` 393 / `panel_traits.rs` 240 / `env.rs` 225 / `settings_defaults.rs` 173） | 12 | 6,811 |

**超 800 行的文件 21 个**：生产 12 个（`ports/editor_completion.rs` 1,250、`view/host/notebook.rs` 1,157、`core/service.rs` 1,023、`view/history.rs` 1,004、`view/analysis.rs` 999、`view/host/results/dispatch.rs` 967、`view/widgets/status_bar.rs` 916、`view/host/render.rs` 903、`view/manage_env.rs` 900、`support/toolbar.rs` 860、`language/completion.rs` 837、`view/host/construct.rs` 803）；测试 9 个（`view/tests/results_a.rs` 2,056、`view/tests/analysis.rs` 2,037、`view/tests/notebook_win.rs` 1,138、`view/tests/channels.rs` 1,135、`view/tests/dialogs.rs` 1,113、`view/tests/results_b.rs` 985、`view/tests/exec.rs` 945、`exec/execution/tests/tx.rs` 930、`view/tests/mod.rs` 914）。

---

## 2. 依赖与边界

【事实】`crates/editor/Cargo.toml:7` 声明硬约束 `workbench → editor → engine / database / shared / gpui-kit`；实际直接依赖 **23 个**（`:10-58`）：`gpui-kit, engine, database, shared, lsp-types, anyhow, encoding_rs, chrono, serde, serde_json, jupyter-protocol, base64, rds-kernel, tokio, tracing, workbench_shell, result_panel, settings, file_meta, duckdb, rfd, uuid, scratchpad`。

【事实】`crates/result_panel/Cargo.toml:10-11` 写死「本 crate 绝不反向依赖 editor」，其依赖表（`:13-35`）确实没有 editor —— 方向成立、无环。

【事实】`crates/editor/src/ports/mod.rs:1-10` 自己写清了 `ports/` 的存在理由：

> 「这些文件是 `EditorShared` 各 `attach_*` 槽位的宿主实现……此前放在 `workbench::services` 下——改任何一个都会触发 workbench 全量重编（26k 行，`-j 2` 下每次 2–4 分钟）。下沉后……只重编 `rds-editor`。」

【推断（有据）】也就是说，`ports/` 不是按领域边界划出来的，而是**按编译时间划出来的**：为了让补全/schema/环境这类改动不触发 workbench 全量重编，把 6,927 行应用服务放进 editor。代价是 editor 的职责语义被稀释——它现在同时是"文档编辑器""执行编排器""DB 元数据访问层""文件对话框调用方""内核管理器"。

【事实】结果域是三层结构：
1. 转发壳：`exec/{analysis,changes,export,store}.rs` 与 `view/results/{grid,error_card,script_card,sets}.rs` 全是对 `result_panel::*` 的 `pub use`（各 6–8 行）；
2. 桥接层：`view/host/results/*`（3,634 行）做 `ResultRequest` 派发、`GridPayload`/`ResultSyncInput` 组装、筛选存档编解码、导出落地；
3. 面板本体：`crates/result_panel`（独立 crate）。

三层的边界目前靠注释维持（例如 `view/host/results/filter.rs:4-5` 明写"按能力域切分 `impl EditorHostPanel` 方法块，字段在 `mod.rs` 统一 `pub(crate)`"）。

【事实】另一个容易被忽略的规模事实：`crates/result_panel/src` 自身 **50 文件 / 44,513 行**，比 `crates/editor` 的三分之一还多。也就是说"编辑器模块"的真实体量是 `editor 67,583 + result_panel 44,513 ≈ 112k 行`，其中近四成在兄弟 crate 里。本文件的主体只审 `crates/editor`，与 `result_panel` 的交界在 §5 P1-2 / §6 D5 单独说明；`result_panel` 内部结构本轮未展开。

---

## 3. 质量纪律（可量化）

| 指标 | 实测 | 说明 |
|---|---:|---|
| 测试属性（`#[test]` / `#[gpui::test]`） | **614** | `src` 内合计；另有 `tests/offline_baseline.rs` 5 例 |
| 运行时用例数（`--list`） | **890** | 属性数小于用例数，因部分用例由宏/循环生成（`--list` 是准数） |
| 本轮实测门禁 | **886 passed / 0 failed / 4 ignored** | HEAD `70e6bdfd`，`cargo test -p rds-editor --lib -j 1 -- --test-threads=1`，68.15s |
| 上一次基线（留档对比） | 864 passed / 0 failed / 4 ignored | 2026-11-23（`docs/analysis/分析模式目录架构与修复清单.md:179`）；本轮比它多 22 例全绿 |
| 内核门禁 | 14 passed / 0 failed / 6 ignored | 引自 2026-11-23 记录，本轮未复测 |
| `unsafe` | **0** | 全 crate |
| `TODO` / `FIXME` / `todo!()` / `unimplemented!()` | **0** | 全 crate（唯一 `XXX` 命中是 `lib.rs:131` 注释里的旧路径字样） |
| 生产 `panic!` | **0** | 28 处命中全部落在 `#[cfg(test)]` 段或 `tests/` |
| 生产 `unwrap/expect` | **19** | `support/toolbar.rs` 4、`exec/queue.rs` 4、`ports/editor_exec/runner.rs` 3、`status_bar.rs` 2、其余 6 处各 1 |
| render 纯读：`block_on` | **0** | `view/` 全目录 |
| render 纯读：`std::fs::` | 生产 **5 处** | `view/host/results/export.rs`:193/221/226/333（导出落盘，事件路径）+ `view/highlight.rs`:671（主题资产，测试段）；其余 56 处在 `view/tests/` |
| gpui-kit 组件复用 | `input` 41、`dock` 13、`Theme` 10、`button` 7、`table` 7、`list` 7 | 符合"禁手搓"基调 |

【事实】最新的分析模式子系统（`notebook/`）是**零 GPUI 的纯逻辑层**：`controller.rs`（编排账本，400 行）、`orchestrate.rs`（事件收口规则，337 行）都可独立单测。这是全 crate 里结构最健康的一块，也是下面设计主张的现成样板。

---

## 4. 完成度评分

评级口径：**A** 已落地且有自动化门禁；**B** 已落地但缺视觉/人工验收；**C** 主体完成、有明确缺口；**D** 局部完成；**E** 未做。

| # | 领域 | 评级 | 证据与缺口 |
|---|---|---|---|
| 1 | 文档模型（打开/保存/编码/换行/脏标记/会话恢复/隐藏文件） | A | `support/{encoding,persist,hidden,session}.rs` + 对应测试；`reload_from_document` 已处理 `set_value` 不发 `Change` |
| 2 | 三模式内核 + 能力表 | A | `core/mode.rs`（24 测试）、`core/model.rs:190` `Capabilities::for_mode` |
| 3 | SQL 执行闭环（目标解析/预检/队列/执行族 5 项/分段抓取/超时/取消/事务/历史） | A | `exec/{target,preflight,queue,notes}.rs`；门禁覆盖 `exec/execution/tests/tx.rs` 930 行 |
| 4 | 三档执行通道（源库/本地加速/联邦） | C | `exec/channel.rs`；缺口：联邦"用作源"标记存储、扫描量可见、只读锁 ⓘ（连接侧写策略） |
| 5 | 结果集（就地编辑 2a、筛选/排序、导出五档、网格 P0–P2、列统计、转置） | C | 六道防线 + 只 UPDATE 已落地；缺口：新增/删除行、列类型化编辑器、结果会话恢复、独立表浏览（`table_editor` 仍 `NotBuilt`） |
| 6 | 补全 / 格式化 / 转译 / 执行计划 | A | 打字 + `Ctrl+Space` + 模板片段；缓存未命中回落实时内省已落地（`ports/editor_completion.rs:233/245/583` cache-aside + 按连接节流，测试 `:2294` `an_empty_cache_submits_one_warm_introspection`），README 的"⬜ 余"已过期；格式化改 `lax-sql` lossless（2026-11-22） |
| 7 | LSP（诊断/hover/签名/函数手册/跳转/代码动作） | A | `lsp/{client,bridge,host,sql_hover,sql_definition,sql_code_action}.rs`；缺口在内联提示/display-map「明确不做」 |
| 8 | 通用模式脚本执行 + 环境管理 | A | `exec/env.rs` + `ports/editor_env.rs` 747 + `view/manage_env.rs` 900 |
| 9 | 分析模式（Cell/Output/Session/内核/mime/补全/诊断/Markdown） | C | `notebook/*` + `view/analysis.rs`；缺口：多内核并发的输入单槽（§5 P1-4）、`.ipynb` 互操作、变量浏览器、Windows 中断＝关内核 |
| 10 | 书签 / 折叠 | B | 折叠 2026-10-27 恢复（`view/fold.rs:44-46`），候选纯函数 + 重喂；**视觉验收未做**（`ui-todo.md` V-6/V-7、W-1/W-2） |
| 11 | Quick Open | A | `quick_open/model` 纯逻辑 + 测试 |
| 12 | 视觉/交互验收（V-1…V-8 共 8 项 + W-1…W-5 共 5 项） | E | `docs/architecture/ui/ui-todo.md` §五/§六 全部 ⬜；本轮未做（用户明示不便点验 GUI） |

统计：**A 7 / B 1 / C 3 / E 1**。"未开工"的表述与第 9 项（分析模式已全量落地）直接冲突，与第 8 项（通用模式）也冲突；它只在第 4、5 项的**边角缺口**上勉强成立。

---

## 5. 结构性问题

### P0-1 `EditorHostPanel` 是未拆解的 god object（强度：高）

【事实】`view/host/mod.rs:277-500` 是 `pub struct EditorHostPanel` 定义，**76 个 `pub(crate)` 字段**；`view/host/` 38 个文件里共 **458 个 `fn`**，绝大多数是 `impl EditorHostPanel` 的方法块（`view/host/results/filter.rs:4-5` 自述"按能力域切分 `impl EditorHostPanel` 方法块，字段在 `mod.rs` 统一 `pub(crate)`，跨域互调经 `pub(crate)` 方法"）。

【影响】目录化解决了"一个文件一万行"，但没解决三件事：
- **写权限无约束**：没有任何机制阻止工具栏代码改执行队列字段、或结果导出代码改文档脏标记，编译器只认 `pub(crate)`；
- **状态生命周期不可推理**：76 个字段里有会话态、执行态、UI 偏好缓存、面板布局、悬浮态（`title_hovered: Cell<bool>`）、弱引用（`group: Option<WeakEntity<TabGroup>>`）混在一起，没有"谁能构造/谁能清空"的边界；
- **测试只能整机起**：想单独测"事务状态机"也得构造整个面板。

【为什么不能靠"再拆文件"解决】`editor-backlog.md §16.2 B4` 已把这条列为待拍板："`view/host/` 是否继续压到 500 行——作者认为无损搬运空间已用尽"。我同意这个判断：剩下的都是**职责边界**问题，不是行数问题。

### P0-2 模式语义分叉点实测 10 处，且已发生过一次口径事故（强度：高）

【事实】`EditorMode` 相关判定分布：

| 位置 | 内容 | 是否与能力表重复 |
|---|---|---|
| `core/model.rs:190` | `Capabilities::for_mode`（唯一权威表） | 权威 |
| `view/fold.rs:46` | `matches!(mode, General \| Sql)` | **重复** `caps.folding` |
| `view/highlight.rs:142` | `matches!(mode, Sql \| General)` | **重复**（且语义 = folding，不是因为高亮真需要判断） |
| `support/toolbar.rs:31-34` | 工具栏图标按模式 | 与 `caps.toolbar` 并行 |
| `support/edit.rs:19-20` | 注释符号按模式 | 与 `caps.kind` 并行 |
| `view/host/render.rs:400` | tooltip 按模式 | 与 `caps` 并行 |
| `lsp/host.rs:110` | 语言 id 按模式 | 合理（协议层） |
| `ports/editor_exec/runner.rs:649` | 执行统计按模式 | 合理（数据层） |
| `quick_open/model/types.rs:49` | quick open 自己的模式枚举 | 独立域，可接受 |
| `core/limits.rs:52` | 大文件档位 `folding: true` | 与能力表交叉校验 |

【已发生的事故】`docs/architecture/editor/editor-architecture.md:717` 记录 2026-10-22 随 #26「行号槽收窄」把折叠关掉：「`Capabilities::folding` 三模式均 `false`、`view/fold.rs::is_enabled` 恒 `false`」；2026-10-27 恢复时（`core/model.rs:198/210` 与 `view/fold.rs:44-46`）**代码同时改回两处**，但**文档没改**，于是今天存在三份互相矛盾的"事实"：
- 代码：General/Sql `folding: true`；
- `editor-architecture.md:717`：三模式均 false；
- `crates/editor/README.md:29` / `docs/architecture/editor/README.md:78,175`：`notebook.rs` 未建。

这次能对上，是因为改的人记得有两处；下一次换人就会漏。**这是结构性缺陷，不是笔误。**

### P1-1 `editor` 已承担应用服务职责（强度：中高）

【事实】`ports/` 6,927 + `lsp/` 3,141 + `language/` 2,107 = **12,175 行（18%）**，内容全是"与外部世界打交道"：DB 元数据、驱动执行、补全 provider、环境探测、文件对话框、内核注册。23 个直接依赖里 `duckdb` / `rfd` / `scratchpad` / `file_meta` / `rds-kernel` / `jupyter-protocol` 都不是"编辑器"该关心的东西。

【影响】
- `cargo check -p rds-editor` 的口径等于"整个应用"，UI 改一行与 DB 访问改一行共用同一张依赖图；
- 语义上说不清"什么属于编辑器"：新功能（比如"扫描量可见"）会自然落进 editor，因为数据就在这儿；
- 与 `ports/mod.rs:1-10` 的初衷相悖：原意是"避免 workbench 全量重编"，结果换成了"editor 全量重编"（editor 现在 67k 行，比 workbench 26k 还大）。

### P1-2 结果域的"三层结构"缺少运行时边界（强度：中）

【事实】见 §2。`view/host/results/` 3,634 行里，`dispatch.rs` 967 行做 `ResultRequest` → editor 状态的双向翻译；`export.rs` 447 行把导出上下文从 editor 搬到 `result_panel`。

【影响】`result_panel` 想加一个结果集能力（比如"新增行"），需要先在 editor 的 `dispatch.rs` 里加一条派发分支、在 `grid.rs` 里加 payload 组装、在 `construct.rs` 里加初始化——**三个 crate 的改动靠同一份口头约定协调**。这类"跨 crate 的隐式契约"是后续最容易漏改的地方。

### P1-3 `support/` 是杂物抽屉（强度：中）

19 个文件 6,350 行，装书签、折叠、编码、格式化、历史、持久化、工具栏、转译、连接、诊断、扫描、项目、缩进、隐藏、命令、UI 常量。命名不表达领域（"support" 等于"其余"），新人无法从目录名判断"改编码逻辑去哪"。

### P1-4 分析模式的内核输入是单槽（强度：中）

【事实】`view/host/mod.rs:455` `pub(crate) nb_pending: Option<NotebookInputRequest>` —— 全文档**只有一个**待输入槽位（`view/host/notebook.rs:1001/1012/1086` 用它做"等待输入"的全局状态）。

【影响】两个单元同时 `input()`（或两个内核并行跑）时只有一条请求能进状态机，其余要靠测试里构造才能暴露；`view/tests/analysis.rs` 已有针对"输入行是否真画出来"的断言（`:2045-2234`），但并发场景没有对应测试。

### P2-1 文档漂移四处，其中一处是同一 crate 自相矛盾（强度：中）

| 文档 | 声称 | 代码 |
|---|---|---|
| `crates/editor/README.md:29` | 「2b / 1c/2d 未开工，`notebook.rs` 尚不存在」 | `lib.rs:21/24/105` 全部 ✅；`notebook/` 7 文件、`view/host/notebook.rs` 1,157 行 |
| `docs/architecture/editor/README.md:8,78,175` | 「分析模式笔记本视图属 1c/2d，尚未开工」 | 同上 |
| `crates/editor/README.md:172` | 「⬜ 余：缓存未命中时回落实时内省」 | `ports/editor_completion.rs:233/245/583` 已实现 cache-aside 写回 + 按连接节流；测试 `:2294` |
| `editor-architecture.md:717` | 「`Capabilities::folding` 三模式均 false、`is_enabled` 恒 false」 | `core/model.rs:198/210` true；`view/fold.rs:46` General/Sql true；`limits.rs:52` true |

折叠那条危害最大：§12 是官方声明的"已知问题与排期唯一权威"，现在它给出的完全是不存在的事实。四条里有三条是"更低一级的文档没跟上代码"，一条（README:172）是"代码已补齐、余项没划掉"——两个方向都会让读者做出错误判断。

### P2-2 大文件与测试文件按"批次"分裂（强度：低）

`view/tests/results_a.rs` 2,056 + `results_b.rs` 985、`view/tests/analysis.rs` 2,037 —— 按"a/b"或"一次迁移一坨"分文件，而不是按被测能力分。找"保存编码"的测试要在 13,940 行测试里翻。

### P2-3 视觉验收长期挂账（强度：低但持续）

`ui-todo.md` §五 V-1…V-8、§六 W-1…W-5 全部 ⬜。这类"必须人眼"的项没有自动化替代，也没有"过期即视为风险"的机制，会一直停留。

---

## 6. 如果让我重新设计

先说设计原则：**这套代码的可读性已经靠"目录化"做到头了，继续拆文件收益趋零。下一步应该换三个维度切：谁能写这份状态、谁依赖谁、模式语义从哪来。**

### D1（第一优先，半天）模式语义收敛为一张表 + 一条守护测试

**问题**：§5 P0-2，10 处分叉，且已经出过一次口径事故（10-22 关闭时两侧同步、10-27 恢复时又同步，但文档三次都没跟上）。
**方案**：
1. 把 `Capabilities` 扩成 `ModeSpec`（加 `fold: bool`、`highlight: bool`、`comment_token: &'static str`、`icon: IconName`、`tooltip: &'static str`），`view::fold::is_enabled` / `view::highlight::is_enabled` / `support::toolbar::icon` / `support::edit::comment_token` / `render::tooltip` 全部改成读表；
2. 加一条守护测试：`for mode in EditorMode::ALL { assert_eq!(fold::is_enabled(mode), spec.folding); ... }`，或者更彻底——**删掉中间函数**，让调用方直接 `spec.fold`；
3. 验收：`grep -n 'matches!(mode' crates/editor/src` 返回 0 行；`match mode {` 只剩 `core/model.rs`（能力表自身）与 `lsp/host.rs`（语言 id）两处。

**收益**：新增第四种模式只改一行表；两侧口径再也不会分家。
**成本**：低（纯函数改动 + 1 条测试）。**风险**：极低（行为不变，测试锁住）。
**这条能自证**：不需要 GUI，纯 `cargo test -p rds-editor`（本轮基线 886/0/4，加上新用例后应变为 887/0/4）。

### D2（第二优先，半天）文档漂移守护测试

**问题**：§5 P2-1，其中一处是"唯一权威"文档给出了不存在的结论。
**方案**：在 `crates/editor/tests/` 加一条 `docs_claims_match_code()`：
- 断言 README/架构文档里出现"未建 / 未开工 / 尚未开工"的路径**确实不存在**；
- 断言 `Capabilities::for_mode` 的 `folding` 三元组与架构 §12 表格里写的三元组一致。

**收益**：把"人记得改文档"变成"改漏就红"。
**成本**：低。**风险**：文档措辞稍有变化就可能误报，需限定匹配范围（只匹配带反引号的路径与带布尔值的表格行）。

### D3（第三优先，1–2 天）按编译边界拆三段 crate

**问题**：§5 P1-1，`ports/` 下沉是为编译时间，代价是职责混淆。
**方案**（不搬回 workbench，而是加一层）：

```
engine / database / shared / gpui-kit / result_panel
        ↑                ↑
 rds-editor-core   ←  rds-editor-services（ports/ + lsp/ + language/，无 view）
        ↑                ↑
        └── rds-editor-ui（view/ + shared/ + notebook 视图）
                    ↑
              workbench（组合注入，保持现有 EditorHost 注入方式）
```

- `rds-editor-core`：`core` + `exec`（目标/队列/通道/预检）+ `host.rs` trait + `notebook` 纯逻辑。**零 gpui**（`exec/queue.rs` 用 tokio 可以接受）。
- `rds-editor-services`：`ports` + `lsp` + `language`。依赖 core，不依赖 ui。
- `rds-editor-ui`：`view` + `shared` + `support`（UI 部分）。依赖 core，经 `EditorShared` 注入 services。
- 收益：改 UI 只重编 ui（~34k）；改补全/DB 适配只重编 services（~12k）；**这正是 `ports/mod.rs` 想解决但解决错了的问题**。
- 成本：中（Cargo 拆分 + `pub use` 兼容层沿用现有套路，`lib.rs:130-141` 已经证明这个套路可行）。**风险**：首次全量编译变长；`test-support` feature 要沿三条链传递；三个 crate 都带 `#[cfg(test)]` 段，需按归属切。

### D4（第四优先，2–4 天）状态所有权显性化：先分组，后抽 Entity

**问题**：§5 P0-1，76 个 `pub(crate)` 字段谁都能写。
**方案分两阶段，不要一步到位**：

- **阶段 A（机械，低风险）**：把 76 个字段按域收进 4 个内嵌结构体：
  - `DocState`（`document` / `edit_mode` / `show_*` / `soft_wrap` / `auto_close` / `smart_indent` / `user_ro_lock` …）
  - `RunState`（`pending` / `running_since` / `autocommit` / `tx_open` / `tx_since` / `tx_pending` / `refresh_pending` / `export_pending` / `nb_pending` …）
  - `ResultsState`（`grid` / `result_panel` / `export_pending` 的结果侧 / 筛选存档 …）
  - `ChromeState`（面板布局 / 工具栏隐藏项 / `title_hovered` / `closed` / `group` / `area` …）
  字段从 `pub(crate) x` 变成 `pub(crate) doc: DocState`，访问变 `self.doc.x`。
  **收益立刻可见**：`grep 'self\.run\.' crates/editor/src/view/host/results/` 一查就知道结果域在偷改执行态——跨域写从"编译器看不见"变成"grep 一行可见"。
- **阶段 B（仅两处，中风险）**：把 `RunState` 与 `ResultsState` 提成真正的 `Entity`（有自己的 `cx`），因为这两个域有异步回调（队列轮询泵、导出线程回执、内核事件），最值得独立生命周期。UI 侧留 `Entity<T>` 引用。
  **不要**把 4 个全提成 Entity：`DocState` / `ChromeState` 与渲染强耦合，独立出去只会增加 `read(cx)` 噪音。

**验收**：阶段 A 后 `view/host/mod.rs` 的 `pub(crate)` 字段数 = 0（分组结构体内部私有）；阶段 B 后 `view/host/results/*` 不再出现 `self.run.`。

### D5（第五优先，3–5 天）结果域收敛：给 `result_panel` 一个 Host trait

**问题**：§5 P1-2，3,634 行桥接跨越 crate 边界靠约定维持。
**方案**：在 `result_panel` 定义 `trait ResultHost`（预估 20 个方法上下：取当前连接、取文档 id、发执行请求、报状态文案、读写面板设置…），把 `dispatch.rs` 里"翻译"逻辑搬进 `result_panel::panel`，editor 只留 `impl ResultHost for EditorHostPanel`（预估 300 行以内——**这是估算，不是实测**）。
**收益**：结果域的新能力（新增行、列类型化编辑、会话恢复）在 `result_panel` 内闭环，editor 只需实现 trait 方法；跨 crate 契约从"口头"变成"trait 签名"。
**风险**：`ResultRequest` 语义宽（`BeginEditCellAt` / `ResetCell` / `FetchAll` / `RequestInsightColumn` …），一次搬完容易踩测试；建议按 `dispatch.rs` 的 match 分支分批搬，每批跑一次 `view/tests/results_a|b`。
**成本**：中高，是本次主张里最贵的一项，建议排在 D1–D4 之后。

### D6（第六优先，3–5 天）分析模式改为每内核 actor + 输入队列

**问题**：§5 P1-4 单槽输入。
**方案**：`notebook/controller.rs` 已有纯账本（400 行，零 GPUI），只需在其上加：
- `KernelRuntime`（每 `(文档, 语言)` 一个），持有自己的 `pending_inputs: VecDeque<(CellId, InputRequest)>`；
- 视图按 `CellId` 顺序渲染输入行，而不是读面板级 `nb_pending`；
- 顺带补 `.ipynb` 导入/导出（纯映射：`cell.rs` 的 v2 JSON ↔ nbformat，无 UI 改动），这是低成本高价值的生态互通。
**验收**：一个 headless 测试里让两个单元先后 `input()`，断言两条请求各自归属正确、都出现在渲染树里（沿用 `view/tests/analysis.rs:2045` 起的"行有没有被画出来"手法）。
**风险**：`orchestrate.rs` 的事件归属规则已存在，扩展要严格复用，不要另起一套归属判断。

### 我不会做的事

- **不会**继续把 `view/host/` 往 500 行压（无损搬运已尽，边际收益趋零，`§16.2 B4` 的判断是对的）；
- **不会**一步把 76 字段拆成 4 个 Entity（阶段 A 的分组就能拿到 80% 收益，成本 1/5）；
- **不会**为三种模式各写一套工具栏/渲染分支（现状"一个内核 + 三档能力"是对的，只需把分叉点收到一张表）；
- **不会**在 0.11 升级前动 gutter/折叠/书签的视觉收尾：0.11 的 `GutterStyle` **有可能**替掉行号槽的私有补丁（上一轮评估未实测能否覆盖"左内边距 0"那条，见 `gpui-kit-0.11.0-impact-assessment.md` §4/§8），先升级再动 UI 可避免两次返工。

---

## 7. 本轮边界（未验证 / 未做）

1. **门禁**：本轮已在 **HEAD `70e6bdfd`** 实测通过（886/0/4，68.15s，日志 `.rds/tmp/audit_editor_gate2.txt`）。审计取证的前半段（HEAD `2f44820d`）一度不可编译——原因是并行会话在 `crates/engine/src/services/duckdb_service.rs:208` 留下的拼写错误 `dowgrades`，该改动随后被其作者提交为 `70e6bdfd`，错误随之消失。这段插曲对结论无影响，但说明**跨 crate 的并行改动会让整仓门禁随时失效**，审计类工作应记录"取证时的 HEAD"。
2. **未做 GUI 点验**：`ui-todo.md` V-1…V-8、W-1…W-5 共 13 项仍为 ⬜，本文件对它们的表述一律是"未验收"，不是"无问题"。
3. **`result_panel` 内部未审计**：只核了它与 editor 的依赖方向、`Cargo.toml` 硬约束与交界文件；44,513 行内部结构（含网格渲染路径的格式化开销）本轮未评，因此本文件不对其性能下任何结论。
4. **设计主张未实施**：D1–D6 全部是提案，未改一行代码、未建新 crate、未提 commit。
5. **0.11 未纳入本轮**：其评估见 `docs/architecture/dependencies/gpui-kit-0.11.0-impact-assessment.md`（`7e2ff19d`），本文件只在 §6 末尾引用了它对 UI 收尾的排序建议。

---

## 8. 复现命令（本轮取证口径）

全部为 PowerShell 7，在本仓库根执行：

```powershell
# A. 分域文件数 / 行数（§1 两张表的来源）
$base = 'crates\editor\src'
Get-ChildItem -LiteralPath $base -Recurse -Filter *.rs -File | Group-Object {
  ($_.FullName.Substring((Resolve-Path $base).Path.Length + 1) -split '\\')[0]
} | ForEach-Object {
  $n = ($_.Group | ForEach-Object { (Get-Content -LiteralPath $_.FullName -Encoding utf8 |
        Measure-Object -Line).Lines } | Measure-Object -Sum).Sum
  '{0} files={1} lines={2}' -f $_.Name, $_.Count, $n
}

# B. god object：字段数与 fn 数（§5 P0-1）
(Get-Content -LiteralPath 'crates\editor\src\view\host\mod.rs' -Encoding utf8)[276..499] |
  Select-String -Pattern '^\s+pub\(crate\) ' | Measure-Object | Select-Object -ExpandProperty Count
(Get-ChildItem -LiteralPath 'crates\editor\src\view\host' -Recurse -Filter *.rs -File |
  Select-String -Pattern '\bfn ').Count

# C. 模式分叉点（§5 P0-2）
Get-ChildItem -LiteralPath 'crates\editor\src' -Recurse -Filter *.rs -File |
  Select-String -Pattern 'matches!\(.*mode|match mode \{|match self\.mode|EditorMode::Analysis =>'

# D. 纪律：TODO 族与 unsafe（§3）
(Get-ChildItem -LiteralPath 'crates\editor\src' -Recurse -Filter *.rs -File |
  Select-String -Pattern 'TODO|FIXME|todo!\(|unimplemented!\(').Count
(Get-ChildItem -LiteralPath 'crates\editor\src' -Recurse -Filter *.rs -File |
  Select-String -Pattern 'unsafe').Count

# E. 生产期 unwrap/expect 与 panic（以各文件首个 #[cfg(test)] 为界；§3 的 19 / 0）
Get-ChildItem -LiteralPath 'crates\editor\src' -Recurse -Filter *.rs -File |
  Where-Object { $_.FullName -notmatch '\\(tests|test_)[\\\.]' -and $_.Name -notmatch 'tests?\.rs$' } |
  ForEach-Object {
    $ls = Get-Content -LiteralPath $_.FullName -Encoding utf8
    $idx = $ls.Count
    for ($i = 0; $i -lt $ls.Count; $i++) { if ($ls[$i] -match '^\s*#\[cfg\(test\)\]') { $idx = $i; break } }
    $seg = $ls[0..([math]::Max($idx - 1, 0))] -join "`n"
    '{0}: unwrap/expect={1} panic={2}' -f $_.Name,
      ([regex]::Matches($seg, '\.unwrap\(\)|\.expect\(')).Count,
      ([regex]::Matches($seg, 'panic!\(')).Count
  } | Select-String -Pattern 'unwrap/expect=(?!0 )|panic=(?!0)'

# F. 门禁（本轮在 HEAD 70e6bdfd 实测：886 passed / 0 failed / 4 ignored，68.15s）
cargo test -p rds-editor --lib -j 1 -- --test-threads=1
```

过程产物均落在 `.rds/tmp/audit_editor_*.txt`（gitignore，未入库）。
