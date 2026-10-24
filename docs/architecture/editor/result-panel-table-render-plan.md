# 结果面板表格渲染 P0 改进方案（clip 裁剪 + 分段语义）

> 目标：落地上一轮竞品对照（`docs/analysis/竞品分析-dbdelve表格渲染与全竞品表格对照.md`）的 P0 两项。
> 依据（源码）：`crates/result_panel/src/grid.rs`、`crates/engine/src/services/sql_service.rs`、`crates/editor/src/view/host.rs`、`crates/result_panel/src/store.rs`。
> 参考：dbdelve `result_grid.rs`（`CELL_DISPLAY_LIMIT=300` + `clip`）、`store.rs`（`GRID_ROW_CAP=5000` + `total_rows`）。

---

## 0. 现状澄清（先修正上一轮报告的两个表述）

1. **"10000 行硬顶"不准确**：`MAX_QUERY_ROWS=10_000`（`sql_service.rs:44`）是**单次 `execute` 的截断上限**（`sql_service.rs:570` `result.truncate(MAX_QUERY_ROWS)`），不是总行数硬顶。
2. **窗口分段取数已存在且已接线**：`sql_service.rs:73-84` 定义了窗口包装（`LIMIT n OFFSET m` 子查询，别名 `rds_segment`，四方言通吃），`execute_first_segment`（:286）/`execute_segment`（:389）实现取段；编辑器侧 `host.rs:731` `set_load_more_hook` → `panel.fetch_more`（:6715）→ `ResultEntry::can_fetch_more`（`store.rs:531`）；`crates/workbench/tests/editor_exec_real.rs:755-791` 有真实取数测试（"第二段拿满 / 到底"）——**滚动加载更多已经工作**。

所以两个 P0 的真实差距是：

| 差距 | 现状 | dbdelve 做法 |
|---|---|---|
| P0-1 单元格渲染 | 全文进 `render_td`（`truncate` 样式截断 + 悬停全文 tooltip） | 每帧每可见格 `clip` 到 300 字符，大值不进 GPUI 布局 |
| P0-2 分段语义 | 窗口分段可用但**界面不表达**：状态行只报"已拿到 M 行"（`grid.rs:2840`），不显示"还有更多/每段上限"，窗口重跑代价（ORDER BY 不稳可能重复/跳过，`sql_service.rs:78-79` 注释明说"要写在界面上"）**没写**；无"共 N 行"总数 | `GRID_ROW_CAP=5000` + `total_rows` 真数 + 落盘恢复（哲学不同：dbdelve 全量流式拉入，RDS-v2 窗口界住内存——窗口内存更稳但不知总数） |

---

## 1. P0-1：单元格渲染裁剪（clip）

### 目标
让**任意长的单元格值**（JSONB / PostGIS 几何 / 大文本，可达数 MB）不参与 GPUI 文本布局——帧率随最宽值退化的现象消除。

### 改动落点：`crates/result_panel/src/grid.rs`

**现状（`render_td`，:2340-2440）**：
```
text = self.cell(row_ix, col_ix)        // 全文
is_null = text == "NULL"                // NULL 判定（clip 前）
display = if is_blob { blob_summary(&text) } else { text.clone() }   // BLOB 已摘要
div().truncate()...tooltip(全文)...child(display)                    // 全文进布局
```

**改动**：
1. 新增常量与纯函数（`grid.rs` 顶部，与 dbdelve 对齐并注明出处）：
   ```rust
   /// 单元格展示文本的字符上限（对齐 dbdelve result_grid.rs CELL_DISPLAY_LIMIT=300）：
   /// 只裁"画出来的这一格"，复制/编辑/悬停/导出取完整值。
   pub const CELL_DISPLAY_LIMIT: usize = 300;

   /// 字符级裁剪（char_indices 取边界，防 Unicode 截断），超限补 "…"。
   fn clip_cell_display(value: &str, limit: usize) -> String;
   ```
2. `render_td` 的 `display` 构造处应用（`is_blob` 分支不动，BLOB 摘要已短）：
   ```rust
   let display = if is_blob {
       blob_summary(&text).unwrap_or_else(|| text.clone())
   } else {
       clip_cell_display(&text, CELL_DISPLAY_LIMIT)
   };
   ```

### 边界（不动的东西——值完整性不受影响）
- `self.cell()` / `row_cell_text`（:1437）：**不动**——复制、导出、统计、编辑初值走完整值
- 悬停全文 `tooltip(full)`（:2436-2437）：**不动**——看全值的路保留（值预览/数值查看器也在）
- 右键菜单 `ContextTarget.value`（:2473）：**不动**——按值筛选用完整值
- 编辑态：`is_editing` 提前返回 `render_edit_cell`（:2348），不走此路径
- 行号槽（:2375-2386）：短值，不 clip
- NULL 判定（:2389）：`"NULL"` 仅 4 字符，天然不超限，判定顺序不变
- 转置视图（:2354-2366）：同一 `render_td`，类型按行取——clip 函数与类型无关，自动生效
- 数字右对齐 / 布尔时间等宽 / 脏格/冲突底色：与内容无关，不受影响

### 单测（`grid.rs` 底部测试区）
- 恰好 300 字符不裁；301 字符裁 + 尾随 `…`
- 中文/emoji 多字节字符边界（`char_indices` 不 panic、不裁半字）
- 多行值、NULL 不被裁、BLOB 摘要优先

### 为什么只裁"展示"不裁"存储"
dbdelve 同款取舍（`result_grid.rs:1313-1333,1555-1602`）：`clip` 只影响画面；`cell()` 返回完整值、`⌘C` 复制全量。RDS-v2 现有 `truncate` 样式已是"视觉截断"，本改动把截断**提前到字符串层**，GPUI 布局只见到 ≤300 字符的短串——布局/测量成本有硬上界。

---

## 2. P0-2：大结果分段语义呈现（+ 可选精确总数）

### 目标
把已经存在的窗口分段能力**诚实地表达在界面上**：用户知道"还没取完、能继续取、每段上限、窗口代价"；需要时能拿到精确总数（"共 N 行"）。

### 2a. 状态行补分段语义（改动最小，只动 `grid.rs` 状态行）

现状（`grid.rs:2840` 附近 `ResultStatus`/`status_segments`）：只报"真拿到多少行，上限是驱动侧的事（engine 侧 10000 行），编辑器不猜那个数"。

改动（状态行 + 页脚）：
- `has_more == true` 时追加「· 滚动到底继续加载」或「· 已加载 M 行 / 还有更多」——`has_more` 字段已在（`grid.rs:543`，面板从权威结果同步）；
- 第一段截断（`truncated == true`）且 `has_more == false` 时追加「· 已达单段上限 10000 行」——区分"取完了"与"截断了"；
- 窗口代价提示（落实 `sql_service.rs:78-79` 注释"要写在界面上，不能假装它和游标等价"）：页脚次级文字/tooltip「分段加载：每段重跑查询；无稳定 `ORDER BY` 时行可能重复或跳过」。

### 2b. 可选精确总数（COUNT，默认关闭）

- engine 新增（`sql_service.rs`）：
  ```rust
  /// 窗口包装的 COUNT 变体：SELECT COUNT(*) FROM (原句) AS rds_segment_count
  pub async fn count_wrapped(&self, conn_id: Option<&str>, sql: &str) -> Result<u64, CoreError>;
  ```
  复用 `rds_segment` 包装的方言适配（别名要求同窗口包装）。
- 面板侧（`host.rs` `fetch_more`/`Refresh` 路径 + `grid.rs` 状态行）：
  - 工具栏「显示总行数」开关（默认关）；
  - 开启时首次取段后异步触发 `count_wrapped`，**超时 2s 放弃**（复杂查询的 count 可能慢，不给用户卡顿），成功则状态行「共 N 行 · 已加载 M 行」；
  - 窗口偏移（已加载 M 行）与 count 结果各自独立展示，不混用。

### 明确不做（本 P0）
- **结果落盘恢复**（dbdelve `StoredGrid` 写盘、重启恢复）→ P1 另立项；
- **全量流式拉入**（dbdelve 极限场景 4GB 持有）→ 不采纳：RDS-v2 窗口模式内存被窗口界住，哲学更稳，保留。

---

## 3. 影响面与验证

| 改动 | 文件 | 行为回归 |
|---|---|---|
| P0-1 clip | `result_panel/src/grid.rs`（常量+函数+`render_td` display 构造+单测） | 无：复制/导出/编辑/筛选/统计/悬停全走完整值 |
| P0-2a 状态行 | `grid.rs`（`ResultStatus`/`status_segments`） | 无：只读已存在字段（`has_more`/`truncated`） |
| P0-2b count | `sql_service.rs`（新方法）+ `host.rs`（触发）+ `grid.rs`（状态） | 默认关，无默认行为变化 |

验证：
- P0-1：单测（Unicode 边界/NULL/300 阈值）+ 视觉抽查（宽 JSON 列滚动帧率，可用 dbdelve 同款"百万行滚动"场景比对）；
- P0-2a：状态行文本断言（`status_segments` 已有单测模式）；
- P0-2b：`editor_exec_real.rs` 式真实取数测试（count 与窗口偏移一致性、超时放弃路径）。

## 4. 任务拆解（按序）

1. P0-1：`clip_cell_display` + 单测 → `render_td` 接线 → 视觉验证（半天）
2. P0-2a：状态行三段文案（还有更多 / 单段上限 / 窗口代价 tooltip）+ 单测（半天）
3. P0-2b：`count_wrapped` + 开关 + 超时放弃 + 真实取数测试（1 天）

---

## 5. 完成状态回写（2026-10-25，以磁盘代码为准）

| 项 | 状态 | 落地说明 |
| --- | --- | --- |
| P0-1 单元格裁剪 | ✅ 完成 | `CELL_DISPLAY_LIMIT = 300` + `clip_cell_display`（`char_indices` 边界，不裁断中文 / emoji）；`render_td` 非 BLOB 分支接裁剪；复制 / 悬停 / 导出 / 编辑 / 右键筛选 / 统计全走完整值。单测 `clip_cell_display_respects_char_boundary_and_limit` |
| P0-2a 状态行分段语义 | ✅ 完成 | `MAX_SINGLE_SEGMENT_ROWS = 10_000`、`WINDOW_FETCH_NOTICE`、`fetch_more_hint` / `truncation_notice` / `window_fetch_active`；右段顺序 = 当前格预览 → 取段提示 → 截断警告 → 旧文案兜底；两段提示先 `.id(...)` 转 `Stateful<Div>` 再挂 `tooltip`，内层 Fn 闭包 `tip.clone()` 避免 move 捕获冲突。单测 `status_fetch_and_truncation_hints_follow_state_combination` |
| P0-2b COUNT 精确总数 | ⏸ 延后 | 本轮不做；engine `QueryResult.total_rows` 恒 0，`has_more` 时状态行只报 `N+`；`count_wrapped` + 默认关开关 + 2s 超时放弃另立项 |

### 5.1 计划外同批落地（见 `result-panel.md` §8.13）

- **P1-4 格内 JSON pretty-print**：`pretty_json_display`，对象 / 数组压缩串美化，非法 JSON / 裸标量回退普通文本；展示派生，原文走完整值路径。
- **P1-5 列宽会话内记忆**：`column_widths: HashMap<String, Pixels>` 按列名存（隐藏列 / 将来列序拖拽不错位）；`set_data` 清空；宿主接 `TableEvent::ColumnWidthsChanged`；不落盘。
- **P2-1 整数千分位**：`group_digits` 内核 + `format_integer_cell`；数值列整数分组，浮点 / 文本 / NULL 不分组；展示层，原值不变。
- **P2-2 计算 / 分组列级访问器**：`column_stats_from_values` / `group_by_values` 列级纯函数 + delegate `column_values` / `column_stats_for` / `group_by_for`；消除 `visible_rows()` 整行矩阵物化；菜单事件期一次性现算保留不动。跨帧结果缓存未做（render 纯读 + 失效口径易脏，记 backlog）。

### 5.2 回归基线

- `cargo test -p rds-result-panel --lib` = **164 passed / 0 failed / 0 ignored**。
- `cargo test -p rds-editor --lib` = **603 passed / 0 failed / 1 ignored**，编译无 error；本线宿主侧仅 `ColumnWidthsChanged` 分派小段。历史 683 基线与 603 的差额来自并行线未提交改动，非本线因果；后续整包若被并行线阻塞，按纪律如实上报、不绕过、不替并行线修。

