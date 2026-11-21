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
