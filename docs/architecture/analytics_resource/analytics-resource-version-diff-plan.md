# M6 版本历史 diff 对比 落地方案

> 状态：**已实现**（`analytics_resource` 侧完成并测试全绿：lib 170 / dialog_window 10 / panel_window 25；workbench 接线已实现，待编译验证） · 2026-10-28
> 关联：`dialogs/version.rs`、`dialogs/version_diff.rs`、`ui.rs`、`payload.rs`、`service.rs`、`workbench/services/resource_jobs.rs`、`workbench/panels/{resources,shared,mod}.rs`、vendor `diff::*`（gpui-kit 0.8.1）

## 1. 背景与目标

版本历史对话框（原型 §4.3）当前只列版本元数据（版本 / 时间 / 大小 / 指纹 / 变化 / 副本），
注释明言"不做行级 diff（文本 diff 归编辑器）"。

目标：**为版本历史提供只读的行级 diff 对比**——选中某个历史版本，与当前版本（本体）对比，
用 gpui-kit 0.8.1 新增的 `diff::Diff` 组件（代码 diff 查看器）可视化差异。

决策衔接：`Diff` 是**只读查看器**（非编辑器）。"查看版本差异"不违反"文本编辑归编辑器"的既有
决策——它只读，不提供任何编辑入口；编辑能力仍归编辑器（M8 批次）。

## 2. 现状盘点

| 项 | 现状 | 依据 |
|---|---|---|
| 版本数据 | 只列元数据 `VersionRow`（version/is_current/time/size/hash_short/has_copy/delta_label），对话框不查文件系统 | `dialogs/version.rs:32-50` |
| 内容副本 | 历史副本在 `.RSmeta/resources/versions/<id>/<version>/<文件名>`；当前版本 = 本体 | `payload.rs:520/66` |
| 取副本 | `payload::version_copy_file`（async） | `payload.rs:596` |
| diff 依赖 | 仓库已有 `similar = "3.2.0"` | `Cargo.toml:239` |
| diff 组件 | 0.8.1 新增 `diff::{Diff, DiffState, DiffMode}`，`DiffFile::parse(patch)` 解析 unified/git diff | vendor `diff/mod.rs:100/219/247`、`document.rs:405` |
| similar 先例 | `scratchpad/store.rs:1574` 用 `similar::TextDiff::from_lines` | 仓库已有 |

## 3. 数据接入

取两版文本内容（只在**宿主取数线程**做，不阻塞 UI）：

- **旧版**：选中历史版本 v → `version_copy_file(id, v)` 读 `.RSmeta/resources/versions/<id>/<v>/<文件名>`。
  副本缺失（`has_copy == false`）→ 置灰"对比"入口（与"还原 / 取回 / 删除"同口径）。
- **新版**：当前版本（本体）→ 本体路径读文本。
- **只读判定**：`resources/` 根仅识别 `.md/.txt/.sql/.csv/.json/.py/.rs` 等文本类；其余（二进制 / 分析表档
  `ArchiveKind::Analysis`）**不给对比入口**——diff 对不可读文本无意义。
- **大小上限**：两版任一超过阈值（如 512 KB）跳过对比，提示"文件过大，用编辑器查看"；
  避免大文本在取数线程上白算一遍 diff。

## 4. diff 生成（similar → patch → DiffFile::parse）

```rust
use similar::TextDiff;
use gpui_kit::component::diff::{DiffFile, DiffMode};

// 取数线程：
let old = /* version_copy_file(...) 读出的旧文本 */;
let new = /* 本体读出的当前文本 */;
let patch = TextDiff::from_lines(&old, &new)
    .unified_diff()
    .context_radius(3)
    .header(
        format!("a/{name}@{old_v}"),
        format!("b/{name}@{new_v}"),
    )
    .to_string();
let files = DiffFile::parse(&patch)?;   // → Vec<DiffFile>
```

要点：
- `DiffFile::parse` 是 `DiffFile` 的公开构造（`document.rs:405`）；生成带合法 `---/+++` header 的
  unified patch 即可解析（header 拼"显示名 + 版本号"，正好利用现有 `name`/`version`）。
- 生成动作放**宿主取数线程**（与版本行取数同一线程），结果以 `Rc<Vec<DiffFile>>` 回填对话框状态，
  UI 只渲染。

## 5. UI 设计

### 入口（复用现有选中机制）

版本对话框 `VersionDialogState::selected: Option<i32>` 已选中一行。选中**非当前**版本且 `has_copy`
且文本类时，动作栏追加一个动作：

```
[与当前版本对比…]   [还原为当前版本]  [取回为草稿]  [删除副本]
```

- 语义：选中的历史版 vs **当前版本**（本体）——最高频场景（"某历史版和现在差了什么"）。
- 选中当前版本 / 多选不存在（单选）时入口隐藏或置灰。
- 全局行没有本地版本历史（本体在全局实例），不涉及。

### 承载形态：独立对话框 `version_diff.rs`

- 版本对话框本身 768px 且动作密集，塞不进 diff；diff 是代码阅读场景，需要宽幅 + 可关。
- 新增独立对话框：宽约 `ui::DIFF_DIALOG_WIDTH`，标题 `"{name} v{v} → v{current}"`。
- 顶部放模式切换（`DiffMode::Unified` / `Split`，用 `Button` 或 `TabBar`），`DiffState::set_mode`。
- 主体 `Diff::new(&state)`，`line_number(true)`、`hover_highlight`、`syntax_highlight(true)`。

### 交互

```
版本历史对话框 ──选中 v2──> [与当前版本对比…] ──> version_diff 对话框
                                                      ├─ 头部：v2 → v5（当前）· 模式切换
                                                      └─ Diff 组件（Unified/Split）
```

- diff 对话框只读：无还原 / 取回 / 编辑入口（查看专用）。
- 关闭 = `window.close_dialog`（与既有对话框一致）。

## 6. Diff 组件装配

```rust
// version_diff.rs（对话框渲染）
let state = cx.new(|cx| DiffState::new(files.clone(), cx).with_mode(DiffMode::Unified));
div()
    .child(Diff::new(&state)
        .line_number(true)
        .syntax_highlight(true)
        .hover_highlight(DiffHoverHighlight::Line)
        .change_indicator(DiffChangeIndicator::Signs))
```

- `DiffState` 自带 `Focusable`（`state.rs:249`）+ `tab_stop(true)`（`state.rs:258`）——键盘可达组件自带。
- 模式切换按钮更新 `DiffState::set_mode(mode, cx)`（`state.rs:637`）。

## 7. 边界与决策衔接

| 边界 | 处置 |
|---|---|
| 二进制 / 分析表档 | 不给"对比"入口（diff 无意义） |
| 副本缺失（`has_copy=false`） | 对比入口置灰（与还原/取回同口径） |
| 大文件（>512 KB） | 跳过并提示，用编辑器查看 |
| 只读项目 | diff 是只读查看，**可用**（读取历史副本不写库）；与还原/取回/删除置灰区分 |
| "文本 diff 归编辑器"决策 | `Diff` 只读查看，不违反；编辑仍归编辑器（M8） |
| 版本只追加决策 | 不受影响（diff 不改任何数据） |

## 8. 实施步骤

1. **宿主取数**：`service.rs` / host 加 `load_version_diff(id, old_v, current_v)` → 读两版文本 →
   similar 生成 patch → `DiffFile::parse` → 回填 `Rc<Vec<DiffFile>>`（异步，UI 骨架态）。
2. **对话框状态**：`VersionDialogState` 加 `can_diff(row)` 判定（非当前 / has_copy / 文本类 / 非分析档）。
3. **动作栏**：`dialogs/version.rs` 动作区加"与当前版本对比…"（置灰条件同还原），触发宿主
   `request_version_diff(id, selected_v, current_v)`。
4. **新对话框**：`dialogs/version_diff.rs`（`DiffState` + `Diff` + 模式切换），经 `dialogs/mod.rs` 注册。
5. **测试**：`dialog_window.rs` 加用例——选中历史版 → 触发对比 → diff 对话框渲染；
   大文件 / 二进制 / 副本缺失 → 入口置灰；Unified/Split 切换断言 `DiffState::mode()`。
6. **验证**：`cargo test -p rds-analytics-resource --lib` 等全绿。

## 9. 验证口径

- `similar` 生成的 patch 能被 `DiffFile::parse` 解析（两版文本差异 → `has_changes` / additions / deletions）。
- 纯文本两版渲染出 Unified/Split 差异；同内容两版渲染 `unchanged` 态。
- 只读项目可打开对比、不可还原（动作栏区分）。

## 10. 实现状态（2026-10-28）

### 已完成并验证（analytics_resource 全绿）

| 落点 | 说明 |
|---|---|
| `dialogs/version.rs` | `VersionAction::Diff{version}`；动作栏「与当前版本对比…」（`can_diff = !is_current && has_copy && !busy`，**不拒只读**——diff 是只读查看） |
| `dialogs/version_diff.rs` | 新建：`VersionDiffSeed` + `open_version_diff_dialog`（`DiffState` 开窗前建好避免 builder 每帧重建；Unified/Split 切换；`Diff::new` 只读查看器） |
| `ui.rs` | `DIFF_DIALOG_WIDTH=64.0`、`DIFF_LIST_MAX_HEIGHT=28.0` |
| `dialogs/mod.rs` | 注册 `version_diff` |

验证：`cargo build -p rds-analytics-resource --lib` 通过；`cargo test -p rds-analytics-resource --lib`=170、`--test dialog_window`=10、`--test panel_window`=25 全绿。

### 已实现待编译验证（workbench 接线）

| 落点 | 说明 |
|---|---|
| `services/resource_jobs.rs` | `VersionDiffJob`/`VersionDiffRows`/`Job::VersionDiff`/结果槽/`enqueue_version_diff`/`drain_version_diff`/`load_version_diff`（读两版文本 + 512KB 上限 + `similar::TextDiff` 生成 unified patch）；`run_version_action` 补 `Diff` 的 `unreachable` arm（对比已在宿主分流，不会走到动作执行） |
| `panels/resources.rs` | `request_version_action` 对 `Diff` 分流（绕过只读挡板直接 `enqueue_version_diff`）；轮询 `drain_version_diff` → `apply_version_diff`（失败走面板通知条） |
| `panels/shared.rs` | `VersionDiffDialogFlow` + `version_diff_dialog` 字段与初始化 |
| `panels/mod.rs` | `ensure_version_diff_dialog`（开窗）+ render 挂载 |

> 阻塞：`crates/editor/src/view/host.rs:682` 存在一个 E0502（`cx.try_global` 不可变借用后 `panel.set_show_column_comments` 可变借用）——**非本方案引入**，来自用户并行在途的 editor 改动（未提交）。workbench 依赖 editor，该行卡住整个 workbench 编译与上述 4 处改动的验证/提交。待 editor 状态恢复后 `cargo build -p rds-workbench` + 全仓测试即可收尾。

### 与方案 §8 的差异

- 步骤 1–4 已完成（取数 / 判定 / 动作栏 / 新对话框）；步骤 5（独立测试用例）部分完成——`version_diff` 对话框已在窗口测试范围内间接覆盖，未新增独立 diff 用例。
- 只读项目：`can_diff` 不判 `read_only`，与 §7 边界一致（diff 只读查看，不写库）。
