# RdataStation v2 整体联动集成测试——增量报告（2026-10-14 · 闪修复轮 2）

> 本轮为「资产管理右键闪退排查」轮：上一轮（UI 修缮 + 多标签闪退修复）全绿后，用户在 UIST 实测中再次遇到**闪退**（资产右键），并复核了 3 项功能缺口（高亮 / 洞察 / Mock 入口）。本轮完成新闪退根因修复（vendor gpui-component 一处行为补丁）、3 项功能缺口逐一核查（**均为已实现已接线**，非缺口）、崩溃现场 crate 与编辑器全量测试复验。**插件（M9）仍按约定不接入**。

## 0. 摘要

| 项 | 上轮（2026-10-14 深夜） | **本轮（2026-10-14 夜 2）** | 变化 |
| --- | --- | --- | --- |
| 工作区测试目标 | 116 | **116** | = |
| 全量集成测试 | 2799 通过 / 0 失败 | **被并发会话阻塞**（见 §5） | ⚠️ |
| 崩溃现场 crate（analytics_resource） | — | **197 通过 / 0 失败** | ✅ |
| 编辑器 crate（editor） | 609 | **616 通过 / 0 失败** | +7 |
| 编译告警/错误（全量 check） | 0/0 | **1 error / 1 warning**（并发会话 `editor_lsp.rs`，非本轮） | ⚠️ |
| 闪退 | 多标签 focus_self（已修复） | **资产右键 a11y 双 set_focus（已修复）** | ✅ |

## 1. 闪退根因与修复（本轮最高优先级）

### 1.1 现场

应用 19:55:26 启动（PID 23408），20:06:02 闪退，panic 文件 `.rds/logs/panic-1791979562.log`：

```
thread: rds-app-main
panicked at .../gpui-pre-0.3.8/src/window/a11y.rs:533:17:
set_focus called more than once in a single frame
```

panic 栈（截取关键帧）：

```
10: <A11yNodeBuilder>::push
11: <A11y>::set_focus                       ← 第二次 claim a11y focus → panic
12: <Interactivity>::prepaint
...
21: <ViewElement<Entity<PopupMenu>>>::prepaint   ← PopupMenu（右键菜单）根
...
59: <Window>::prepaint_deferred_draws            ← deferred 绘制阶段
```

### 1.2 根因（gpui a11y 单帧单焦点约束 × 菜单 deferred 夺焦）

gpui `div.rs` prepaint 对每个**聚焦中的 focusable 元素**（`track_focus`）都会调 `window.a11y.set_focus(node_id)`，而 a11y 层**每帧只允许一个**元素 claim 焦点（`a11y.rs:533` debug 断言）。当 Windows 无障碍（读屏）激活时（本机 a11y 激活，应用日志持续输出 a11y 信息），同一帧出现两次 `set_focus` 即 panic。

触发序列（资产管理右键文件）：

1. 用户此前**左键**点过列表行 → gpui 点击聚焦（`div.rs` 的 click-to-focus）把窗口焦点给到列表/面板（`list-state` / `analytics-resource-panel` 均 `track_focus`）；
2. 用户**右键**同一行 → `ContextMenu` 打开（`open = true`，deferred 建菜单）；
3. 下一帧主树 prepaint：列表/面板 handle 仍 `is_focused` → **set_focus（第 1 次）**；
4. `prepaint_deferred_draws` → `DeferredMenu::build_menu` → `focus_handle.focus(window, cx)` **立即**把 `window.focus` 改为菜单 handle（`window.focus()` 同步生效）；
5. PopupMenu 根 `track_focus` 看到自己 `is_focused` → **set_focus（第 2 次）** → **panic**。

`FocusId::is_focused` 是严格相等（`window.focus == Some(id)`），所以两处 claim 必然来自「主树旧焦点 + deferred 新焦点」跨阶段并存。

### 1.3 修复（vendor gpui-component 行为补丁）

`vendor/gpui-component/src/menu/context_menu.rs` 右键处理器中，**捕获 `previous_focus_handle` 之后、deferred 建菜单之前**，仅当 `window.is_a11y_active() && window.focused(cx).is_some()` 时 `window.blur(cx)`：

- blur 发生在事件派发（上一帧），下一帧主树 prepaint 时旧元素不再 `is_focused` → 不再 claim a11y focus；
- 只有菜单 claim → 单帧单 set_focus，不再 panic；
- `previous_focus_handle` 在 blur **之前**捕获，关闭菜单时仍恢复原焦点（`menu.set_previous_focus`）；
- 仅在 a11y 激活时生效：无读屏的机器零行为变化。

这是本项目对 vendored gpui-component 的**第二处**行为补丁（首处见 `Cargo.toml` 注释：多光标 / 焦点处理器改 pub）。`[patch.crates-io]` 指向 `vendor/gpui-component`，构建即生效，无需改上游。

### 1.4 回归验证说明

该 panic 被 `window.a11y.is_active()` 门控，而 gpui 测试平台（`platform/test.rs`）**不实现 a11y 激活**——单测环境 a11y 恒为 false，无法直接复现该 panic。已核实：

- vendored gpui-component 的自身测试目标因 `include_*` 路径缺失（icon.rs / button_icon.rs / dock / theme 等，vendor 只含 `src/`）**本就无法编译**，故无法在 vendor 测试模块补回归测试；
- 本仓库内可编译的窗口级测试（`rds-analytics-resource` 的 `panel_window.rs`，25 个窗口级用例）覆盖右键菜单打开/交互路径，全部通过；
- 修复的**真实验证 = 应用带 a11y 实测**（用户 UIST 复测右键菜单）。

## 2. 3 项功能缺口核查（用户点名）

用户报「高亮没实现 / 洞察没接入 / Mock 没入口」。逐一核查代码后，**三者均为已实现且已接线**，结论如下：

| # | 用户意见 | 核查结论 | 证据 |
| --- | --- | --- | --- |
| 1 | 编辑器语言高亮没实现 | **已实现已接线**（SQL 语义着色）。用户可能测试的是文本模式（`General` 是纯记事本，**有意**不上色），或并发会话 LSP 改动期间的中间态 | `editor/src/view/highlight.rs`：`SqlSemanticTokensProvider` + `install`；`view/host.rs:305/3088` 在 `highlight::is_enabled(mode)`（仅 Sql）时安装；单测 `view::highlight::tests::*` 全过（含 `legend_covers_every_theme_token_we_emit`） |
| 2 | 洞察没接入 | **已完整接入**（M8）。入口在导航树右键（查看洞察/结构洞察/查看统计）+ 结果集列头右键（洞察此列）+ Quick Open 命令 `panel.insight`；右 Dock 洞察面板默认激活 | `workbench/src/view.rs:165`（`editor_insight::attach`）、`view.rs:1415`（右活动栏遍历 `RightPanel::ALL`）、`panels/right.rs:208`、`panels/shared.rs:639-753`（open_insight_* 四个入口）、`quick_open/commands.rs:87`；editor 单测 `grid::tests::context_menu_only_offers_insight_when_available` 通过 |
| 3 | Mock 数据没入口 | **入口存在**。右活动栏第 2 个图标即 Mock（`RightPanel::Mock`，label「Mock 生成」，图标 `icons/dice-5.svg` 在 gpui-kit-assets 0.7.1 中确认存在）；Quick Open 命令 `panel.mock` | `workbench_shell/src/model.rs:100-126`（`RightPanel::ALL` = Insight/Mock/History/Archive + 图标路径）、`quick_open/commands.rs:98` |

> 说明：三项都「入口在、数据/前提在」——高亮需 **Sql 模式**文档（文本模式刻意不上色）；洞察需**可取样目标**（导航表/视图右键或查询结果列头）；Mock 需**数据库连接 + 表/视图**（导航树表/视图节点的 Mock 入口，`nav_view/rows.rs:2236`）。若用户在空连接/文本模式下找，自然「看不到」。报告建议：UIST 复测时按上述前提操作。

## 3. 验证情况（并发会话阻塞的说明）

### 3.1 已通过（本轮涉及 crate）

| crate | 结果 | 备注 |
| --- | --- | --- |
| `rds-analytics-resource`（崩溃现场） | **197 通过 / 0 失败**（162 lib + 10 dialog_window + 25 panel_window） | panel_window 25 个窗口级用例覆盖右键菜单交互路径；编译即验证 vendor 补丁在消费侧 OK |
| `rds-editor` | **616 通过 / 0 失败** | 含上轮闪退回归测试、欢迎页、高亮、洞察右键、以及并发会话的 lsp::* 测试（全过） |

### 3.2 全量 check / 集成测试：被并发会话阻塞（非本轮引入）

- `cargo check --workspace` 与 `cargo test --workspace` 均在 **`rds-workbench` 编译期失败**：`crates/workbench/src/services/editor_lsp.rs:161` `error[E0716]`（临时值借出）+ `:41` `warning: unused import: LspPortHandle`；
- `editor_lsp.rs`（未跟踪）与 `workbench/src/view.rs` / `services/mod.rs` 的改动是**另一并发会话正在进行的 LSP server 集成**（Python + jedi-language-server，`assemble` 加 `spawn_lsp` 参数、`editor_lsp::attach`），带 U+FFFD 乱码注释，属进行中工作；
- **工作树其余文件（含 Cargo.lock 新增 alsa/borsh 等 LSP 依赖）同为并发会话改动**，本轮一律未触碰、未提交。

## 4. 提交情况

- 本轮代码提交（1 个）：
  - `398067a fix(menu): 上下文菜单打开时清除焦点避免 a11y 双 set_focus 闪退`（`vendor/gpui-component/src/menu/context_menu.rs`，+12 行）
- **未包含**（并发会话工作，用户另行处理）：`crates/editor/src/lsp/{bridge.rs,host.rs}`（未跟踪）、`crates/editor/src/lsp/{client.rs,mod.rs}`、`crates/editor/src/shared.rs`、`crates/editor/src/view/host.rs`、`crates/workbench/src/services/{mod.rs,editor_lsp.rs}`、`crates/workbench/src/view.rs`、`docs/architecture/plugin/plugin-prototype-design.md`、`Cargo.lock`。

## 5. 待办（阻塞项）

1. **并发会话完成 `editor_lsp.rs` 修复后**，全量 check（0 告警）+ 全量集成测试（`cargo test --workspace -j 2 --no-fail-fast`）才能跑通——当前 workbench 编译错误阻塞全部测试目标；
2. 应用**重启 + UIST 复测**：本轮修复须在真实 a11y 环境下验证（右键资产管理文件 → 菜单应正常打开、不闪退；关闭菜单后焦点恢复到原行）；
3. 复测要点：Sql 文档看高亮、导航表/视图右键看洞察、右活动栏 Mock 图标进 Mock 面板。

## 6. 口径与日志

- 过程产物：`.rds/tmp/rds-check-2026-10-14.log`（check，workbench 编译错误）、`.rds/tmp/rds-integration-2026-10-14.log`（集成测试，build 阶段失败）、`.rds/tmp/analytics-test.log`（未生成，直接用终端跑）、panic 原始文件 `.rds/logs/panic-1791979562.log`。
- 本轮单独验证：`cargo test -p rds-analytics-resource` → **197/197**；`cargo test -p rds-editor --lib` → **616/616**。
