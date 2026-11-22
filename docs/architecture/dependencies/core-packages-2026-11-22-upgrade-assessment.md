# 核心包版本复查与升级必要性判断（2026-11-22）

- 复查日期：2026-11-22
- 复查对象：`gpui-kit` / `gpui-fast` / `duckdb` / `sqlglot-rust`（及其绑定家族 `gpui-base` / `gpui-component` / `gpui-pre` / `libduckdb-sys`）
- 项目基线：本地 `main`（2026-11-08 并入的 `gpui-kit 0.9.0 + gpui-fast 0.1.13`、`duckdb 1.10506.1`（DuckDB v1.5.6）、`sqlglot-rust 0.11.1`）
- 取证口径：**crates.io API 的 `max_version` / `versions`**、**GitHub tag 与 compare**、**从 `static.crates.io` 下载发布包源码后本地实测**。**没有使用本地 `cargo info`**——本机 `CARGO_HOME` 走 tuna 镜像，索引滞后（见 `dependency-strategy.md` R3）。
- 本文只做判断与计划，**未改动任何依赖、`Cargo.lock`、`vendor/`**；因此"能否编过"仍未实证，见 §7。

## 1. 结论摘要

| 包 | 当前 | 上游最新 | 建议 | 一句话依据 |
| --- | --- | --- | --- | --- |
| `sqlglot-rust` | `0.11.1` | `0.11.2`（2026-11-13） | ✅ **升，值得** | 9 个提交、两个真 bug（PG 数组维度/基数保真、`UPDATE FROM` join 保持）；`CreateTableStatement` 结构未变，我们 2 处填空不用改 |
| `gpui-fast` | `0.1.13` | `0.1.17`（2026-11-17） | ✅ **升，值得** | 77 个提交里有 6 条直接命中我们已知失败机制（保留绘制的观察层），这是那批 UI 红的**上游修复路径**，不是要长期忍的已知问题 |
| `gpui-kit` | `0.9.0` | `0.9.1` / `0.10.0`（2026-11-09 / 11-16） | ⚠️ **升级有必要，但不是必须；要做就一步到 0.10.0** | 0.10.0 = 167 提交 / 300 文件，公开 API 对我们**零断裂**（删除的 3 个符号我们 0 引用、`element.rs` 117 个 fn 签名 0 变化）；但有实质收益（`GridLayout`、表格右固定列、折叠行占位符），代价是 vendor 私有 patch 要在被重做的 gutter 区重打一次 |
| `duckdb` | `1.10506.1`（v1.5.6） | `1.20001.0`（DuckDB v2.0.1，2026-11-20） | ⛔ **不升** | 1.5 线已无新补丁（`1.10506.1` 是最后一版）；2.0 线仍要 `arrow ^60`（我们 58.4.0），而 1.5 是 LTS 支持到 2027-03，没有紧急性 |
| `rustls-webpki`（顺带发现） | `0.103.15` | `0.103.17`（2026-11-12） | ✅ **单独升，与本轮无关** | 上游在 gpui-kit 0.9.1 周期专门提交了 `deps: Update rustls-webpki … for CVE-2026-111943`；我们锁在修复前两版，且依赖图里确实经 `rustls 0.23.44` 用它 |

## 2. `gpui-kit`：0.9.1 与 0.10.0 的性质

时间线与包含关系（GitHub tag 实测）：

| tag | 发布日期 | 相对前一个 | 相对 `v0.9.0` |
| --- | --- | --- | --- |
| `v0.9.0` | 2026-11-02 | — | — |
| `v0.9.1` | 2026-11-09 | +74 提交 / 228 个文件 | +74 |
| `v0.10.0` | 2026-11-16 | +93 提交 | **+167 提交 / 300 文件** |

**0.9.1 已被 0.10.0 完全包含**（74 + 93 = 167，与 `v0.9.0...v0.10.0` 的提交数逐一对上）。所以不存在"先跳 0.9.1 更稳"的路线——同一段 vendor patch 会被打两遍。要么升到 0.10.0，要么留在 0.9.0。

### 2.1 对我们的断裂面：实测几乎为零

从 `static.crates.io` 取得 `gpui-base` / `gpui-component` / `gpui-kit` 的 0.9.0 与 0.10.0 发布包源码，扫描 `src/**/*.rs` 里的 `pub` 条目（fn / struct / enum / trait / const / static / type / mod / use）并做集合差：

| 包 | 0.9.0 | 0.10.0 | 被删除 | 新增 |
| --- | --- | --- | --- | --- |
| `gpui-base` | 915 | 959 | **1**（`dock_extent`） | 45 |
| `gpui-component` | 768 | 777 | **2**（`BORDER_RADIUS`、`render_tooltip`） | 11 |
| `gpui-kit` | 18 | 18 | **0** | **0**（门面 re-export 面未变） |

这三个被删符号在 `crates/**` 里**各 0 处引用**（grep 实测）→ 项目代码层面没有 API 硬断。

更进一步，`vendor/gpui-base/src/input/base/element.rs`（我们 464 行私有 patch 的宿主）做函数签名对比：0.9.0 有 117 个 fn，0.10.0 有 136 个，**删除 0 个、签名变更 0 个**，新增的 19 个全是新函数。也就是说我们 patch 要插进去的锚点函数都还在，且形参没变。

### 2.2 但 gutter / 折叠区被上游重做了 —— 这是真正的成本

`element.rs` 原始文件：0.9.0 **6157 行** → 0.10.0 **6952 行**（+845/−50，其中 +550/−36 落在 0.9.1 这一段）。同时这条线上游自己动了我们 patch 所在的地盘：

- `e7ee9402 input: Draw a placeholder for a folded line`（折叠行占位符；新增 `FoldPlaceholderRenderer`）
- `2905983d input: Show the arrow over the editor gutter`（折叠箭头画到 gutter 上）
- `5b72161e input: Publish the input's layout at the end of prepaint`（在 prepaint 末尾发布输入区布局）
- 新增测试 `text_origin_offset_tracks_rendered_gutter_and_scrolling`、`a_folded_line_shows_its_placeholder_right_after_the_text_and_unfolds_on_click`
- 新增 fn `layout_folded_lines`、`textarea_layout`、`fit_content_height`

含义有两层：
1. **不能复用旧补丁位置**：`element.rs` 变大 795 行、gutter 区被改写，`tools/_archive/port_gutter.py` 的 13 处锚点必须按新文件重跑一遍（脚本自带 count 断言，跑一次就能知道哪几处漂了）。
2. **我们的 patch 可能变薄**：上游现在自己会在 gutter 上画折叠箭头、有折叠行占位与 gutter 偏移概念，我们 patch 里同类的部分也许可以直接让给上游；但**必须先读上游实现再决定**——0.9.0 那次的教训是"上游 `GutterDecoration` 只画不点"，所以"上游有类似名字"不等于"能替代"（书签要点击切换 + 悬停说明）。

### 2.3 实质收益（不是"为了新版本号"）

| 能力 | 位置 | 对项目的价值 |
| --- | --- | --- |
| `GridLayout` 一族（`GridLayout` / `GridLayoutState` / `DragGridItem` / `GridGesture` / `GridMetrics`，base +1040/−0 等四个文件，component 侧 +105） | 新组件 | EDA / 洞察 / 报表这类"用户可拖拽改尺寸的网格面板"，以及未来 dashboard 化，不必手搓 |
| `table: Add ColumnFixed::Right` | `component/table/column.rs` | 结果集表格**右固定列**（行号/操作列钉在右侧），direct 命中 `result_panel` |
| `FoldPlaceholderRenderer` + 折叠行占位 | `base/input/base` | 折叠行的视觉表达由内核统一，减少我们自己 patch 的面积 |
| `component: Redraw fully in GPUI Fast lib tests`（`d86516d1`，在 0.9.1 段） | 组件测试 | 直接对应我们压着的"gpui-fast 观察层口径"问题 |
| `A11yTextRuns`（`input/base/a11y_text.rs` +368） | 编辑器 a11y | 文本输入的可访问性重做，桌面工具长期收益 |
| `text: Add on_task_toggle`、`LinkTooltipFn`、`highlight_matches` | 文本 | Markdown 任务项可点、链接悬停、搜索高亮，本仓有 Markdown/文档预览场景 |

## 3. `gpui-fast`：0.1.13 → 0.1.17 是"修我们已知失败"的版本

`v0.1.13...v0.1.17`：**77 个提交 / 184 个文件**。其中与我们 2026-11-08 定性的失败机制（保留绘制 + `Weak` 元素注册 + 绘制记录复用）同类或直接相关的至少 6 条：

| 提交 | 说明 | 对应我们的现象 |
| --- | --- | --- |
| `#91` | `fast: avoid redundant ancestor renders after child splices` | 子树未变不重建 → `missing ElementId` |
| `#106` | `fast: forget released entities in a spliced view's dependencies` | 实体释放后仍被依赖表持有 |
| `#114` | `fast: record a draw's entities by what entered and left the window's set` | 绘制记录口径（`debug_bounds` 读到旧记录） |
| `#117` | `fast: draw a cached view around its notified cached children` | 缓存视图与子节点通知 |
| `#122` / `#126` | retained a11y 重放把合成子层级压平；该修复先放到 hooks 后面 | a11y 树与元素 id 路径 |
| `#129` | `fast: measure a measure_all list over later frames after a width change` | 列表/虚拟列表行"有高度但没画" |

另有 `#99` / `#131` / `#153` 等渲染与性能项（WGPU 动态纹理、Windows 动态纹理后端、用哈希定位重复的状态与偏移读取）。

**判断：升。** 理由不是"版本新"，而是我们报告 §7/§10 里那 6~9 条红的机制，上游在 11-09 ~ 11-17 这一波里逐条修。继续留在 0.1.13 等于自愿保留已知缺陷，然后在测试里绕。

版本对齐要求（发布包 manifest 实测）：`gpui-fast 0.1.17` 钉 `gpui-pre =0.3.12`；`gpui-kit 0.10.0` 钉 `gpui-pre =0.3.12`、要求 `gpui-fast ^0.1.16`。所以 kit 与 fast 必须同批升，`gpui-pre` 从 0.3.11 → 0.3.12。

## 4. `duckdb`：1.5 线已收尾，2.0 线仍不该碰

| 事实 | 数据 |
| --- | --- |
| 1.5 线最新 crate | `1.10506.1`（2026-10-16），**之后再无 1.5 patch** |
| 2.0 线 | `1.20000.0`(10-22) → `.1`(10-28) → `.2`(11-13) → **`1.20001.0`(11-20)** |
| 2.0 线硬依赖 | `duckdb 1.20001.0` 要求 **`arrow ^60`**（我们 `58.4.0`），且 `libduckdb-sys =1.20001.0` 精确钉同版 |
| 支持窗口 | DuckDB 1.5 为 LTS，支持至 **2027-03** |

结论：**现在不升**。理由是"升级换不到本仓需要的东西"：我们不用 Server 模式（Quack）、不直接用 DuckDB 的 PEG parser（解析走 `sqlglot-rust`），而代价是 `arrow` 跨大版本 + 存储格式进入 2.0 线 + 手工替换 `third_party/duckdb/` 库 + 重新验证 `appender-arrow`/vtab 面。

若将来要升：目标是 **`1.20001.0`**（不是 `1.20000.x`），并连带 `arrow 60.x`；评估见 `duckdb-2.0-gpui-kit-0.8.1-upgrade-assessment.md`（该文写于 0.8.1 时代，其中 gpui 部分已过时，DuckDB 部分仍适用）。

## 5. `sqlglot-rust`：0.11.2 值得，成本最低

`v0.11.1...v0.11.2`：**9 个提交**，实质两条修复 + 一条 CI 整理：

- `PSQ-5302 preserve UPDATE FROM joins throughout SQL parsing`
- `PSQ-5486 preserve PostgreSQL array dimensions and cardinality`（新增回归测试 `tests/test_psq5486_array_dimensions.rs`）
- 其余为 release 准备与 CI（`0bd49757`、`4aadf790`、`8f50347d`、`ffee7345`）

对我们最关键的是**依赖面没有变化**：下载 `sqlglot-rust-0.11.2.crate` 实测 `CreateTableStatement` 字段序列仍是
`comments → if_not_exists → temporary → table → columns → constraints → with_properties → as_select → with_data`
——即 2026-11-08 为 CTAS 补的 `with_properties` / `with_data` 两处填空**不需要再动**，也不会再出现 E0063。

价值落在产品面：本仓的 SQL 分析/转译面向 MySQL / PG / DuckDB 多源，PG 数组维度与基数保真、`UPDATE FROM` join 保持，直接减少"解析一趟就变形"的误判（语义诊断、谓词下推、转译提示都吃这个）。

**判断：升。** 成本 ≈ 改一处版本号 + `cargo update -p sqlglot-rust --precise 0.11.2` + `rds-sql` 一轮。

## 6. 顺带发现：`rustls-webpki` 落后于 CVE 修复

| 事实 | 证据 |
| --- | --- |
| 我们 lock 里 `rustls-webpki 0.103.15`（2026-08-21 版） | `Cargo.lock` |
| 上游已有 `0.103.16`（2026-10-20）、`0.103.17`（2026-11-12） | crates.io `versions` |
| gpui-kit 在 0.9.1 周期提交 `675965a7 deps: Update rustls-webpki and webpki-root-certs for CVE-2026-111943` | GitHub compare `v0.9.0...v0.9.1` |
| 该库在我们依赖图里是活的：`rustls 0.23.44 → rustls-webpki`，`rustls` 被 quinn / hyper / 多个 TLS 客户端依赖 | `Cargo.lock` 依赖边 |

**这条不必等 gpui-kit 升级**：`cargo update -p rustls-webpki` 即可拿到 0.103.17。是否真被我们触发（MySQL/PG TLS、S3/httpfs、SSH 通道用到证书校验）需在升级时用一次真连接复核；未复核前只能算"已消除版本落后"，不能宣称"漏洞已消除"。

## 7. 建议的执行顺序与前置条件

| 优先级 | 内容 | 预估成本 | 前置条件 |
| --- | --- | --- | --- |
| **P0** | `sqlglot-rust 0.11.2` + `rustls-webpki 0.103.17`（两条都是 patch，不加 feature、不动 vendor） | 约 0.5 h：改版本 + `rds-sql` / `rds-engine` 各一轮 | 工作树干净（当前有 12 项未提交改动，需先确认是否影响） |
| **P1** | `gpui-kit 0.10.0 + gpui-base/component 0.10.0 + gpui-kit-assets 0.10.0 + gpui-pre 0.3.12 + gpui-fast 0.1.17`，vendor 私有 patch 重打 | 半天到一天（vendor 换包 + 13 处锚点重跑 + 编译收敛 + `gpui/` 全链重编数轮 + 全量） | ① 工作树干净；② **V-1~V-8 目视验收先做完**（否则换了内核就失去现有视觉基线，出问题分不清是谁的） |
| **P2** | `duckdb 1.20001.0` + `arrow 60.x` | — | 明确要做 2.0 能力（Server 模式 / VARIANT / Triggers）时再启动；1.5 LTS 到 2027-03 |

P1 的验收口径（不可省略）：
1. `cargo check -p gpui-base`（vendor 侧）0 error 0 warning；
2. `-p gpui-component -p rds-editor -p rds-app` 0 error（warning 只允许既有 `rds-mock` 那 3 条）；
3. 书签三条行为用例 + `folding_candidates_are_fed_after_the_gutter_restored` 绿；
4. `rds-editor --lib` 与 `rds-engine --lib` 对基线；
5. 全量 `cargo test --workspace -j 1 --no-fail-fast` 的失败**逐条归因**，不许"总数没涨就算过"；
6. 冷编 + 20 秒冒烟（0 panic / 0 ERROR）；
7. 目视项重新过一遍（V-1~V-8），因为内核换了。

## 8. 本次未做的（明确记账）

- **没有**做 `cargo check` 探针：要证明"0.10.0 能编过"必须换 `vendor/` 与 `Cargo.lock`，属于未获授权的代码改动。因此本文的"编译可行性"只到"公开 API 与 patch 锚点都在"，**不等于编过**。
- **没有**核实 CVE-2026-111943 的具体影响面（没有 RustSec 库在线核查，`cargo-audit` 未安装），只核到"版本落后"这一层。
- **没有**评估 `GridLayout` 与本仓面板体系的复用可行性（要先读上游实现，属 P1 的工作内容）。
- **没有**动 0.9.0 上那 8 项目视验收（`ui-todo.md` §五），与本文判断无冲突。

## 9. 取证命令（可复跑）

```powershell
cd D:\RdataStation\RDS\RdataStation-v2
python .rds\tmp\probe_2026_1122.py    # crates.io max_version / versions
python .rds\tmp\probe_2026_1122c.py   # lock 当前版本 + 上游 pin + fast/sqlglot compare
python .rds\tmp\probe_2026_1122d.py   # 下载 0.10.0 / 0.11.2 发布包，查 patch 锚点与 CTAS 字段
python .rds\tmp\probe_2026_1122f.py   # v0.9.0..v0.9.1 compare + element.rs 函数签名对比
python .rds\tmp\probe_2026_1122g.py   # 0.9.0 vs 0.10.0 公开符号集合差
```

产物落在 `.rds/tmp/`（gitignore，不入库）。发布包解包在 `.rds/tmp/crates1011/`。

## 10. 续篇：对「结果集面板对齐 DBeaver」这条线的具体影响

> 口径：本节只回答一个问题——**升级会不会让结果集面板这条线更好做**。取证件与 §1-§9 同源（`.rds/tmp/crates1011/` 里已解包的 0.9.0 / 0.10.0 发布包 + 本地 `vendor/` 0.9.0），逐条带源码位置。

### 10.1 结论

**直接帮助接近于零。** 这条线上正在欠的三个缺口里，两个是「本仓没接线」而不是「组件没能力」，第三个（输入框全选）两版都不给。升级的真正价值只有三处：`gpui-fast` 的观察层修复（预防性，不修红用例）、`ColumnFixed::Right`（本线当前无对应需求）、`DataTable::scrollbar_mode` 与 `Table::hover`（都属目视类，已统一后处理）。

### 10.2 0.10.0 的公开 API 增量（逐模块实测）

按 `pub fn` 做集合差（`gpui-component` 0.9.0 vs 0.10.0）：

| 模块 | 增/删 | 具体符号 |
| --- | --- | --- |
| `table` | **+4 / −0** | `column.rs::fixed_right`、`data_table.rs::scrollbar_mode`、`table.rs::hover`、`table.rs::id` |
| `dialog` | **+0 / −0** | `AlertDialog` / `Dialog` 全套 0.9.0 即有（`alert_dialog.rs` 的 `new:80` / `confirm:96` / `trigger:109` / `on_ok:303` / `on_cancel:314`） |
| `menu` | **+0 / −0** | 右键菜单能力无变化 |
| `input` | **+1 / −0** | 仅 `number_input.rs::stepper` |

表格语义层面完全没动：`TableEvent` 枚举、`SelectionMode` 枚举、`set_selected_row` 函数体**两版逐字一致**（`table/state.rs`）。

### 10.3 三个在跑的缺口 ↔ 升级是否解决

| 缺口 | 升级能解决吗 | 实证 |
| --- | --- | --- |
| `FetchAll` 确认弹窗（对齐 `CONFIRM_RS_FETCH_ALL`） | **不能** | 组件侧 0.9.0 已有 `dialog/alert_dialog.rs`；项目另有 8 处在用 `window.open_alert_dialog`（`database/src/nav_view/rows.rs:1066`、`analytics_resource/src/dialogs/*`）。结果集面板没用它，是因为本仓选了**面板内浮层**范式——`result_panel/src/panel/dialog.rs:149` 明写「不是窗口级 dialog」 |
| `gotoColumn`（`CTRL+SHIFT+G`） | **不能** | `vendor/gpui-component/src/table/state.rs` 0.9.0 已有 `scroll_to_col`(:454) / `selected_col`(:583) / `set_selected_col`(:589) / `set_selected_cell`(:629) / `set_selection`(:647) / `clear_selection`(:657)；且本仓**已在用**——`result_panel/src/panel/events.rs:257` 的「定位」就是 `set_selected_cell` + `scroll_to_row/col`。缺的只是「列名/索引 → 列序」的输入层 |
| `CTRL+G` 的「预填 + 全选」 | **不能** | 0.10.0 的 `InputState` 公开面与 0.9.0 完全相同（`as_input`/`as_textarea`/`as_editor`/`as_otp`/`value`/`focus_handle`），`select_all_from_edit_menu` 仍是 `pub(crate)`；`as_input()` 返回 `Option<&Entity<InputState>>`，拿不到 base state 的 `select_all` |
| `sync_result_view` 清 `selected_row` | **不能** | 本仓 `editor/src/view/host/results/grid.rs:169` 自己的写法（`selected_row: None`），组件不涉及 |

**这条对上一批的更正**：第 70 批遗留里写的「本仓无『选中某列并滚动』的现成入口」不准确。组件有、本仓也已在用（`events.rs:257`）。`gotoColumn` 不需要等升级，缺的是输入交互层。

### 10.4 升级真正新增的三样东西（对本线的定位）

| 能力 | 内容 | 对本线 |
| --- | --- | --- |
| `ColumnFixed::Right` / `fixed_right()` | 0.9.0 的 `ColumnFixed` 只有 `Left`；0.10.0 加 `Right`，渲染实现完整（`table/state.rs:823/833/1456/1469/1672/1849/2262/2325/2533` 十处：右固定列计数、表头 bounds、滚动区切分、内边缘边框） | **当前无对应需求**：DBeaver 结果集没有右固定列。本仓只用 `fixed_left`（`grid/table_impl.rs:314/341` 的 B15 冻结列）。只有将来做「行操作列钉右侧」才用得上 |
| `DataTable::scrollbar_mode(ScrollbarMode)` | Windows 默认 `Hover`（悬停才显） | **目视类**（滚动条显隐策略），并进目视统一那一轮，不在对齐批次里做 |
| `Table::hover(f)` / `Table::id(ElementId)` | 行悬停样式 / 行带稳定身份（重排时行内元素状态跟行走） | `hover` 属目视类。`id` 是**潜在**能力：排序或行重排后行内编辑框/tooltip 位置漂移理论上能修，但本仓插入固定落末尾、排序时清编辑态，**当前没有实证红点**，不算已证收益 |

### 10.5 `gpui-fast` 0.1.17：本线唯一「真」收益，但是预防性

§7 第 5 条已定性：开 fast 后的失败**全部落在观察层**（`Weak` 元素注册 + 绘制记录复用），没有一条是画面不对。0.1.17 的修复正打这些机制（`#91` 子树未变不重建、`#106` 释放实体仍被依赖表持有、`#114` 绘制记录按进出集合记账、`#117`、`#122`/`#126`、`#129` 宽度变化后跨帧 measure）。

本仓结果集面板侧的观察层断言分布（`debug_bounds` / `try_find` 命中数）：

| 文件 | 处数 |
| --- | --- |
| `crates/result_panel/src/panel/tests.rs` | 85 |
| `crates/result_panel/src/grid/tests.rs` | 19 |
| `crates/editor/src/view/tests/results_a.rs` | 12 |
| `crates/editor/src/view/tests/analysis.rs` | 13 |

这些断言**现在全绿**（panel 408 / editor 812），所以升级的意义是「降低对渲染帧的偶然依赖、把绕的部分收回」，不是「修某个红用例」。这条线不会因为升级变绿或变红。

### 10.6 代价与并行冲突

- **本线代码零返工**：0.9.0→0.10.0 删除的 3 个符号（`dock_extent` / `BORDER_RADIUS` / `render_tooltip`）在 `crates/**` 各 0 引用（§2.1 实测），本线不需要改代码。
- **门禁要重跑**：editor 812 条里含 fold / bookmark（吃 gutter 重做）。第 70 批已踩过一次 rustc 编译期 OOM（物理内存余 ~3.8 GB），升级那轮会再碰一次，`CARGO_INCREMENTAL='0'` 要备着。
- **文件冲突面**：升级分支占 `Cargo.toml` / `Cargo.lock` / `vendor/`；本线只碰 `crates/**` 与 `docs/**`。**唯一共享的是 `crates/app/src/main.rs`**（键位注册表）——通知并行线与升级分支都会碰它，三线同改，提交必须继续用 `.rds/tmp/stage-main-hunk-*.py` 只摘自家 hunk。
- 升级验证期整仓可能一段时间编不过，**本线必须停在干净 HEAD**，不要在同一时间往 `crates/**` 叠新批。

### 10.7 建议

1. **升级不改变本线节奏**：继续「一小批 + 四门禁 + 代码/文档两刀」。升级合入前，本线只做不碰 `Cargo.*` / `vendor/` / `main.rs` 的批（`result_panel` / `editor` 内部逻辑 + 文档）。
2. 升级合入后可顺手接的三件，按价值排：`Table::id`（先写「排序后行内编辑态错位」探测用例，证实了才改）→ `scrollbar_mode`（并入目视统一那轮）→ `fixed_right`（无需求，不动）。
3. 真正该先做的是两件**不用升级就能做**的：`FetchAll` 真循环 + 确认浮层（修既有 bug，顺带解锁 `CTRL+SHIFT+=`）、`gotoColumn`（组件能力已在手）。

### 10.8 取证命令（可复跑）

```powershell
# 0.10.0 / 0.9.0 发布包已在本地
ls .rds/tmp/crates1011
# 公开 API 集合差（table / dialog / menu / input）
python -c "..."   # 见本节表格的实现方式：遍历 src/<module>/*.rs 抽 `pub fn`
# 表格语义是否变动
python -c "..."   # 对比两版 table/state.rs 的 set_selected_row 函数体
```

---

---

## 11. 隔离分支实测记录（2026-11-22，`upgrade/core-2026-11-22`）

本节把「§8 本次未做」里的 `cargo check` 探针补上了，并记录实测口径。

### 11.1 分支与工作区

| 项 | 值 |
| --- | --- |
| 主工作区 | `D:\RdataStation\RDS\RdataStation-v2`（分支拉出时 `main` = `f7e2946a`；验证期间 main 前进到 `96a6893f`，已并入分支，见 §11.5） |
| 隔离 worktree | `D:\RdataStation\RDS\RdataStation-v2-core-1122` |
| 分支 | `upgrade/core-2026-11-22`（从 `f7e2946a` 拉出，后续 `git merge main`） |
| 隔离手段 | `git worktree add`：物理两个工作目录，`target/`、`third_party/duckdb/` 各一份，互不覆盖 |

### 11.2 版本落点（实测 `Cargo.lock`）

`gpui-kit 0.10.0` / `gpui-base`·`gpui-component`·`gpui-kit-assets`·`gpui-component-macros` **0.10.0** /
`gpui-fast 0.1.17` / `gpui-pre 0.3.12` / `sqlglot-rust 0.11.2` / `rustls-webpki 0.103.17`；
`duckdb`·`libduckdb-sys` 仍 `1.10506.1`、`arrow` 仍 `58.4.0`（按 §4 结论不升 2.0 线）。
相对 `main` 共 37 个包版本变化。

### 11.3 私有 patch 的跨大版本重打

发布包源码替换 `vendor/gpui-base`、`vendor/gpui-component` 后，私有 patch 面（12 文件 / 67 hunk / +1166 −49）**不能走锚点法**：
锚点自动插入只成 22/67，失败区全是上游重写区（`element.rs` 的 gutter / 折叠区）。
最终用 `git merge-file --diff3` 三方合并（base = 纯净 0.9.0、ours = 纯净 0.10.0、theirs = 打过 patch 的 0.9.0），
冲突只有 8 处（`element.rs` 5 / `tabs.rs` 1 / `tab/tab.rs` 1 / `dock/mod.rs` 1），其余 8 个文件零冲突。

**结论（可复用）**：跨大版本 vendor patch 的主线是三方合并，锚点法只能当兜底。

### 11.4 编译验证阶梯（含一次环境级阻塞的根因）

1. 首次 `cargo check -p gpui-base` 报 `LNK1104 无法打开 ...\.rds\tmp\lnk{GUID}`。
   根因：`.cargo/config.toml` 用 `force = true` 把 `TEMP/TMP/TMPDIR` 钉成相对路径 `.rds/tmp`，
   而新 worktree 里该目录不存在。**新 worktree 冷编前必须先建 `.rds/tmp`**。
2. 随后 5 次连续 `0xc0000409 STATUS_STACK_BUFFER_OVERRUN`，都死在第三方 `windows 0.62.2`，
   `-j 2` / `-j 1` / `RUST_MIN_STACK` 512MB、1GB / `CARGO_INCREMENTAL=0` 组合均复现。
3. 归因排除：`cargo metadata` 对比两侧 `windows 0.62.2` 的 feature 集合 —— **完全一致**
   （91 个 feature、71 个 `Win32_*`），所以不是升级把 feature 撑大。
   真正原因是**内存压力下的 rustc 栈溢出**（当时物理内存 16 GB 只剩 2.7–3.7 GB，
   并发编译时 `windows` 这个巨型 crate 会爆栈）。
4. 规避：先单独 `cargo check -p windows@0.62.2 -j 1`（`RUST_MIN_STACK=1GB`，2m05s 通过），
   再回到 `gpui-base`，此后一路通过。

**可复用口径**：串行冷编固定用
`RUST_MIN_STACK=1073741824`、`CARGO_INCREMENTAL=0`、`-j 1`；遇到 `windows` 崩溃先单独编它。

### 11.5 验证结果

| 验证项 | 命令 | 结果 |
| --- | --- | --- |
| 第三方阻塞 crate | `cargo check -p windows@0.62.2 -j 1` | ✅ 2m05s |
| vendor base（含私有 patch） | `cargo check -p gpui-base -j 1` | ✅ 15.39s |
| vendor component（含私有 patch） | `cargo check -p gpui-component -j 1` | ✅ 4m20s |
| 全工作区 | `cargo check --workspace -j 1` | ✅ 14m38s，0 error |
| SQL 层（sqlglot 0.11.2） | `cargo test -p rds-sql -j 1` | ✅ 218 + 10 passed |
| 编辑器 | `cargo test -p rds-editor -j 1` | ✅ 810 passed / 1 ignored |
| 结果面板 | `cargo test -p rds-result-panel` | ✅ 408 passed |
| UI 契约 | `cargo test -p rds-ui-guard --test workspace_ui_contract` | ✅ 4 passed |
| 全量 | `cargo test --workspace -j 1 --no-fail-fast` | ✅ 155 个 test target 全 ok、0 failed、1 ignored（见 §11.6-4） |
| 并入 main 后复验（`96a6893f` + 14 提交） | `cargo check --workspace` / `cargo test --workspace` | ✅ 1m18s / 155 target 全 ok、0 failed |

### 11.6 升级暴露并已处理的 4 处

1. `vendor/gpui-base/src/tabs.rs`：三方合并时导入列表取了上游那行，漏掉 `Entity`（上游 0.10.0 自己在用）。
   → 补回 `Entity` 导入。
2. `crates/ui_guard/tests/workspace_ui_contract.rs`：升级哨兵触发 —— `ThemeConfigColors` 键数 140 → **141**。
   已重跑 `python tools/gen_theme_keys.py`，新增键是 **`list.highlight.foreground`**（0.10.0 新增的列表高亮前景色）。
   守卫常量同步为 141。**我们的 `assets/themes/rds-theme.json` 没有为该键赋值**（未赋值就走上游默认色），
   要不要给 RDS Light/Dark 各配一个值属设计决策，留待确认后再改。
3. `crates/result_panel/src/panel/tests.rs` 两条用例（`delete_row_button_is_disabled_on_a_deleted_row`、
   `right_panel_pages_share_the_toolbar_shape`）在 0.9.0 基线下是绿的，升级后红。
   根因不是产品逻辑：gpui-fast 0.1.17 起**未被失效的保留子树在 `window.draw` 时被原样重放**，
   不再重跑 `render`/`paint`（见 `vendor/gpui-component/src/lib.rs::test_redraw` 的注释）。
   两条用例都在「直接改状态字段 / 只 refresh delegate」之后断言渲染结果，于是读到上一帧。
   → 在断言前加 `window.refresh()` 强制一帧，并写明原因；**这不是放宽断言，是跟上新的帧语义**。
   同类的另外三处（`rds-workbench` 的 `connection_dialog_ui`、`connection_render_matrix` 的
   `settle_reveal`、`rds-database` 的 `nav_view::tests::redraw` 助手）也按同一口径补了 refresh。

4. **`crates/database` 导航树的横向画布在 0.10.0 下失效（行为差异，未改断言）**：
   `chrome.rs` 的装配是「视口 `overflow_x_scroll` + 画布 `min_w(30rem)`」。
   0.9.0 上这条契约用例是绿的；0.10.0 上把面板夹到 320px（< 画布下界 480px）后，
   行宽仍只有 312px、`ScrollHandle::max_offset().x` 仍为 **0** —— 说明 `min_w` 不再撑宽内容，
   内容被夹到视口宽，**横向滚动条事实上没有出现的机会**。
   处置：用例加 `#[ignore = "…待真机目视确认后改断言"]`，注释写清实测数据；
   不悄悄放宽断言，因为「面板拖窄后还能不能横滚看全右对齐列」需要肉眼判断。
   真机确认结果出来后再决定：要么改断言（接受新语义），要么在装配上换手段（例如内容用 `min_w` 之外的方式定宽）。

### 11.7 没做 / 需要人工目视的部分

**目视清单（对应本轮实测，按优先级）**：

1. `docs/architecture/ui/ui-todo.md` §五 的 `V-1 ~ V-8` 目视项（未做）。
2. gutter 书签图标 vs 折叠箭头重叠时的 z 序（`paint_gutter_markers` 在 `paint_fold_icons` 之后）。
3. 导航树面板拖窄后：是否还能横向滚动看全右对齐列（§11.6-4 的实测结论要肉眼定案）。
4. 主题：`list.highlight.foreground` 未配值，列表高亮前景色走的是上游默认色，看是否与 RDS Light/Dark 冲突。
5. 折叠行占位符（`FoldPlaceholderRenderer`）与我们的书签列同时出现时的行高/对齐。

**未做**：

- 没有任何 GUI 运行验证。引擎、编辑器、主题、gutter 书签列、标签拖拽都只是「编过 + 单测过」。
- `cargo clippy` / `cargo fmt --check` 未跑。
- RustSec / `cargo-audit` 仍未核查（`rustls-webpki` 只做到「版本落后已消除」，未断言 CVE 一手影响面）。

### 11.8 全量测试

`cargo test --workspace -j 1 --no-fail-fast`：**155 个 test target 全 ok，0 failed**，
唯一非绿是 §11.6-4 的 `#[ignore]`（故意挂着）。日志 `.rds/tmp/wt_test_ws2.log`（gitignore）。

### 11.9 合并与回退指引（本地、不推远端）

```powershell
# 验收通过后，把隔离分支并入 main（本地合并，不 push）
cd D:\RdataStation\RDS\RdataStation-v2
git merge upgrade/core-2026-11-22

# 回退：main 未合并前直接弃分支
git worktree remove D:\RdataStation\RDS\RdataStation-v2-core-1122
git branch -D upgrade/core-2026-11-22
```

注意：隔离 worktree 内的 `third_party/duckdb/1.5.6/`、`target/`、`.rds/tmp/` 都是 gitignore 目录，
删除 worktree 时它们同时消失，不会影响主工作区。

---

## 12. 合流记录（2026-11-22 晚，`upgrade/core-2026-11-22` → `main`）

### 12.1 合流前 main 又前进了 17 个提交

`main` 从 §11 验证时的 `96a6893f` 走到 **`02a51b40`**（17 个提交），期间又动了几处与升级面相邻的地方：

| 方向 | 提交（节选） | 与本升级的关系 |
| --- | --- | --- |
| 结果面板第 76–81 批 | `3ae1c3a3` 选列/选行、`70a266cb` 切焦点、`fd407034` 隐藏/显示列、`c074ff56` 复制列名、`2788795e` 筛选设置、`6dfdbf5c` ESC 收起浮层 | 走的是 `gpui-base` 的 keystroke / Tab 装配，**0.10.0 的帧语义改动会波及这些新用例** |
| 通知中心重做 | `1fab7d89`（相对时间 / 节流落盘 / 替换式合并 / 单条已读） | 与升级面无关，但引入了 `crates/notification` 803 行新代码 |
| SQL 排版引擎 | `51b18592` 换 `lax-sql 0.3.0`（设置项 12 项） | **直接撞在 `Cargo.toml` 上**（本轮唯一冲突点）；语义仍走 `sqlglot-rust` |
| 文档 | 第 76–81 批文档 + `docs/architecture/sql/...` 新增 | 与本文 §11 记录共存 |

### 12.2 合并过程与唯一冲突

`git merge main`：271 个文件差异，**唯一冲突 `Cargo.toml`**（`crates/result_panel/src/panel/tests.rs` 自动合并成功）。

冲突内容与解决口径：

```
HEAD(分支) sqlglot-rust = "=0.11.2"
main        sqlglot-rust = "=0.11.1"
main      + lax-sql      = "=0.3.0"   （SQL 排版引擎，lossless）
main      + dprint-core  = "=0.67.4"  （lax 的配置类型直接引用它的 NewLineKind）
```

取「**保留 0.11.2（§5 已实测通过）+ 并入 main 的三个新依赖**」。理由：`sqlglot-rust` 与 `lax-sql` 职责不重叠
（语义 vs 排版，见 `crates/sql/src/formatter.rs` 头部），没有版本耦合，两边的收益可以叠加。

`Cargo.lock` 由 git 自动合并后，再用 `cargo metadata --format-version 1 --no-deps` 让它定型（本次 lock 变动 41 增 / 7 删）。
合并提交 **`41622794`**。

### 12.3 版本落点（合流后 `Cargo.lock` 实测）

| 包 | 版本 | 相对 main |
| --- | --- | --- |
| `gpui-kit` / `gpui-base` / `gpui-component` / `gpui-kit-assets` | **0.10.0** | ↑ 0.9.0 |
| `gpui-fast` | **0.1.17** | ↑ 0.1.13 |
| `gpui-pre` | **0.3.12** | ↑ 0.3.11 |
| `sqlglot-rust` | **0.11.2** | ↑ 0.11.1 |
| `lax-sql` / `dprint-core` | **0.3.0** / 0.67.4 | 与 main 一致（未动） |
| `rustls-webpki` | **0.103.17** | ↑ 0.103.15 |
| `duckdb` / `libduckdb-sys` | **1.10506.1**（DuckDB 1.5.6） | 未动（按 §4 结论） |
| `arrow` | **58.4.0** | 未动（按 §4 结论） |

### 12.4 合流后验证

| 验证项 | 命令 | 结果 |
| --- | --- | --- |
| 全工作区编译 | `cargo check --workspace -j 1`（`RUST_MIN_STACK=1GB`、`CARGO_INCREMENTAL=0`） | ✅ 4m25s，**0 error / 3 warning**（3 条都是 main 侧 `rds-notification` 的 `write_history*` dead_code，不是升级引入） |
| 全量测试 | `cargo test --workspace -j 1 --no-fail-fast` | 见下方「测试结果」 |

**测试结果**：`cargo test --workspace -j 1 --no-fail-fast` → **155 个 test target 全 ok、0 failed、
4617 passed / 35 ignored**（含本文 §11.6-4 那条故意 `#[ignore]` 的导航树横滚用例，其余 ignored 为既有/上游 ignore）。
与 §11.8 合流前的「155 target 全绿」一致：main 新增的 17 个提交（结果面板第 76–81 批 + 通知中心重做 + `lax-sql`）**在 0.10.0 上同样全绿**，
0.10.0 的保留帧语义没有打到这批新用例。日志 `.rds/tmp/test_merged2.log`（gitignore）。

### 12.5 本次一并同步的文档与 skill（清单）

代码改了就必须把「当前事实」一起改，否则下一个人（或下一次的我）会按旧口径干活。本次同步范围：

| 文件 | 改了什么 |
| --- | --- |
| 本文 | 新增 §12（合流记录）、§13（DuckDB 2.0 切换时间表） |
| `README.md` | 技术栈表 4 行：UI `0.8.1 → 0.10.0`、分析引擎 `1.5.5 → 1.5.6`（crate `1.10505.0 → 1.10506.1`）、SQL 工具链 `sqlglot-rust 0.10.33 → 0.11.2` + 补 `lax-sql 0.3.0` |
| `docs/architecture/dependencies/dependency-strategy.md` | R4 例外表：`gpui-kit` 0.10.0、`sqlglot-rust` 0.11.2、**新增 `lax-sql` 行**、`arrow` 行补「DuckDB 2.0 线要 arrow 60」；待办表 4 行；§3 之后补「2026-11-22 执行结果」 |
| `docs/architecture/theme/theme-tokens-complete.md` | 顶部状态行 + **新增 2026-11-22 勘误段**：141 键、资产缺 `list.highlight.foreground`、`theme-keys.json` 已 141、资产未配值属设计决策 |
| `docs/architecture/ui/theme-spec.md` | 顶部「基于 gpui-kit 0.8.1」→ 0.10.0 + 141 键 |
| `docs/architecture/theme/theme-design.md` | 技术栈行 → 0.10.0（设计意图仍标注 0.7.1 时代定下） |
| `docs/architecture/layout/layout-design.md` | 技术栈行 → 当前 0.10.0（布局决策标注 0.7.1 时代定下） |
| `docs/architecture/plugin/plugin-manager-ui.md` | 「vendor 0.9.0 为准」→ 0.10.0 |
| `docs/architecture/ui/ui-todo.md` | **新增 §六**：0.10.0 的 5 项目视验收（W-1~W-5），含导航树横滚那条待定案项 |
| `docs/architecture/editor/result-panel.md` | 技术栈行 `0.8.0 → 0.10.0`（并指向 §11 的帧语义） |
| `docs/architecture/README.md` | 索引补登本文（`dependencies/` 段）—— 注意该文件同时被用户 WIP 改过，合流时需人工核对这一行 |
| `AGENTS.md` | 「当前 vendor 0.9.0」→ **0.10.0**；并补「MCP 索引未覆盖 0.9.x/0.10.0 时以 vendor 源码为准」 |
| `.agents/skills/gpui-kit-dev/SKILL.md` | frontmatter description、标题、勘误块（含 0.10.0 四条口径）、「查 API」权威源段、组件选型表**补 0.10.0 新增模块 `grid_layout`**、测试段新增「改完状态要 `window.refresh()` 再 draw」 |
| `.agents/skills/rds-architecture/SKILL.md` | 勘误块；分层表 gpui-kit 行（0.10.0 + vendor 实测 55 顶层模块 / 29 子目录） |
| `.agents/skills/rds-design/SKILL.md` | 勘误块 |
| `.agents/skills/rds-theme/SKILL.md` | 勘误块；「140/140 全量色板」→「140 已配 / 141 键」；`theme_audit.py` 口径改 141；`ThemeColor` 行补 141 键 |
| `.agents/skills/rds-ui-spec/SKILL.md` | 勘误块；组件规格总表标题加「0.10.0 未逐项复核」免责与回写口径 |
| `.agents/skills/rds-layout/SKILL.md` | 边栏三模式标题去掉版本号，补「0.10.0 下 `set_dock` / `toggle_dock` / `set_dock_size` 仍在用且编译通过」 |

**有意不改的**：`docs/architecture/**` 里大量「技术栈：gpui-kit 0.7.1」的原型/开发计划文档 —— 那是**当时锁定版本的事实陈述**，
改成 0.10.0 会让历史记录失真。统一口径是：这类文档只作为历史决策依据读，**当前版本一律以 `README.md` 技术栈表、
`dependency-strategy.md` R4 表、`AGENTS.md` 与 `Cargo.lock` 为准**，版本差异看本文 §11。

### 12.6 main 工作区的 24 项未提交改动（合流的前置条件）

`main` 工作区当时有用户自己的 **24 项**未提交改动（19 项 `M` + 5 项 `??`，含 `crates/project` / `crates/settings` /
`crates/sql` / `crates/workbench` 的版本徽章特性与文档），其中 **5 个文件**与本次合流的改动面重叠：

`crates/settings/src/registry/mod.rs`、`crates/settings/src/service/mod.rs`、`crates/workbench/src/view/actions.rs`、
`docs/architecture/README.md`、`docs/architecture/settings/settings-architecture.md`。

这 5 个文件在 main 上被改过、在分支上也被改过（分支侧是 gpui-kit 0.10.0 带来的装配与注册表调整）。
处置口径：**不合流就不丢**；要合流必须先让 main 干净 —— `git stash push -u`（带唯一 tag）→ `merge --ff-only`
（分支已包含 main 全部提交，故可快进）→ `stash pop`；`pop` 若冲突就**立刻停下并保留 stash**，不丢用户 WIP。

---

## 13. DuckDB 2.0 的切换时间表（2026-11-22 结论）

用户问「什么时候能升 DuckDB 2.0」。结论：**不建议现在切，目标窗口 2027 Q1，硬 deadline 2027-03**。

### 13.1 三条决定时间的硬约束

1. **1.5 LTS 支持到 2027-03**（1.4 LTS 已于 2026-11-17 EOL），官方迁移页同时写明「mission-critical 工作负载建议等后续 patch」。
2. **依赖死锁**：`duckdb 1.20001.0`（DuckDB 2.0.1，2026-11-20）把 `arrow ^60` 写成**非可选**依赖；我们现在的
   `duckdb 1.10506.1` 要 `arrow ^58`（caret 即 `>=58,<59`）。**只能一步到位，不存在先升 arrow 再升 duckdb 的路径。**
3. **存储格式单向**：2.0 默认 storage version **69**，2.0 之前读不了 2.0 写出的库；我们有一个持久库
   `analytics.duckdb`（`DuckDBManager` 独占写连接），还有用户自己的 `.duckdb` 文件要 `ATTACH`。

### 13.2 arrow 58 → 60 的实际成本（已核实，比预想低）

| 核对项 | 结论 | 依据 |
| --- | --- | --- |
| 引用面 | 26 个 `.rs` / 116 处 / 18 条符号路径（`RecordBatch` 16、`DataType` 13、`StringArray` 10…） | `probe_arrow_surface.py` |
| 符号存在性 | `StringArray` / `LargeStringArray` / `BooleanArray` / `Int64Array` / `Date32Array` / `Decimal128Array` / `ListArray` / `compute::cast` / `ipc::reader·writer` **全部仍在** | 解包 `arrow-60.2.0` / `arrow-array-60.2.0` / `arrow-schema-60.2.0` 逐项 grep |
| 枚举变体 | `DataType` **41 → 41**、`TimeUnit` 4 → 4、`IntervalUnit` 3 → 3，**零增删** | 两份 `arrow-schema` Cargo 包体枚举体解析比对 |
| default features | 58 与 60 均为 `["csv","ipc","json"]`，`ipc` 仍默认开启 | 两侧 `Cargo.toml` `[features]` |
| 剩余风险 | arrow 60「移除长期废弃 API + `From<Vec<_>>` → `TryFrom`」是否命中我们的 116 处，**只能靠一次 `cargo check` 定论**（约 15 分钟） | arrow-rs 60.0.0 公告 |

### 13.3 2.0 的破坏面（与本项目用法绑定）

- 语义级、**静默改结果**：`~` / `!~` 变部分正则匹配、阶乘 `!` 绑定更紧、`X'…'` / `B'…'` 改产 `BLOB` / `BITSTRING`。
- 运行期才炸：整数除零 / 取模 `//` `%` 由返 NULL 改**报错**、`MERGE INTO` 多源匹配报错、事务内任何错误直接中止事务。
- 类型系统：未类型化 `NULL` 现在保持 `NULL` 类型（`CREATE TABLE … AS SELECT NULL AS x` 建出 "NULL" 列）。
- 生态：**C API v2「尚未稳定」**、ICU 移出核心构建（时区/日历/排序改由 `icu` 扩展承担）。
- 与本项目耦合最紧的两条：`analytics.duckdb` 的存储版本、以及 `ATTACH … (TYPE mysql|postgres|sqlite|duckdb)` 联邦加速用到的扩展。

### 13.4 建议路线（带闸门）

- **P0（现在～12 月中，不动版本号）**：① 定 `analytics.duckdb` 的存储策略（`storage_compatibility_version` / `ATTACH … STORAGE_VERSION`
  保 v1.5.6 可读，或改内存库 + 关闭导出）；② 把上面 13.3 写成 SQL 回归清单并接到 `insight` / EDA / 结果面板的分析 SQL 上；
  ③ 内核管理器加「DuckDB 版本能力探测」，2.0 专属语法按版本开关；④ 建 A/B 基准集。
- **P1（12 月中～1 月，隔离分支，不合并）**：换 `libduckdb 1.20001.0`（`tools/fetch-duckdb.sh`）+ arrow 60，只跑
  `rds-engine` / `rds-mock` / `rds-sql` / `rds-insight`，产出「编译错误数 / 测试红数 / 基准对比」三张表。
- **P2（2027-01～03，正式切换）**：闸门 = 2.0.x 修到 **.3** 且无新回归 + P1 三张表全绿 + 基准收益达阈值 + 回退方案就绪。
  过闸就切；过不了就停在 1.5 LTS，但 2027-03 之后是**无补丁裸奔**，届时必须做决定。

**口径声明**：本节的收益部分全部来自官方发布说明，**没有任何本项目 A/B 数据**。在跑出基准之前，不许对外宣称
「2.0 让我们的分析更快」。另一方面，`VARIANT` / 嵌套 schema / `CONNECT`（quack 服务端）属**功能型**收益，
应单独立项评估，不应捆绑在「内核升级」里一起做。
