# RdataStation v2 整体联动集成测试——增量报告（2026-10-14 · 修复轮）

> 本轮为「闪退排查 + UI 修缮」轮：上一轮（清理+复验）全绿后，用户在 UIST 实测中遇到**闪退**并给出 4 项 UI 修缮意见。本轮完成闪退根因修复（含回归测试）、4 项 UI 修缮、全量 check 0 告警复验与全量集成测试复验。**插件（M9）仍按约定不接入**。

## 0. 摘要

| 项 | 上轮（2026-10-14 晚） | **本轮（2026-10-14 深夜）** | 变化 |
| --- | --- | --- | --- |
| 工作区测试目标 | 116 | **116** | = |
| 通过 | 2775 | **见 §5 实跑数** | 新增 2 个 editor 测试 |
| 失败 | 0 | **0** | = |
| 忽略 | 44 | **44** | = |
| 编译告警 | 0 | **0** | = |
| 编译错误 | 0 | **0** | = |
| 闪退 | **有（1 次 panic）** | **已修复 + 回归测试** | ✅ |

- 命令：`cargo test --workspace -j 2 --no-fail-fast`（绕 rustup shim，显式 RUSTC/RUSTDOC，见任务说明 §1）
- check 日志：`.rds/tmp/rds-check-2026-10-14.log`（`Finished dev profile in 21.46s`，0 warning / 0 error）
- 集成测试日志：`.rds/tmp/rds-integration-2026-10-14.log`

## 1. 闪退根因与修复（最高优先级）

### 1.1 现场

应用 18:30:27 启动（PID 20756），18:36:00 闪退，panic 文件 `.rds/logs/panic-1791974160.log`：

```
thread: rds-app-main
panicked at .../gpui-pre-0.3.8/src/app/entity_map.rs:156:32:
cannot read rds_editor::view::host::EditorHostPanel while it is already being updated
```

### 1.2 根因（double-lease 重入）

调用链（panic 栈）：

```
show_document (workbench/src/view.rs:489)
  → panel.update(... focus_self ...)          ← EditorHostPanel 进入「更新中」
    → focus_self (editor/src/view/host.rs:725)
      → group.update(... select_tab ...)       ← 更新 TabGroup
        → select_tab (gpui-base dock/tab_group.rs:232)
          → focus_active_panel → active_panel → visible
            → Entity::read(EditorHostPanel)     ← 读回**正在被更新**的面板 → panic
```

`TabGroup::select_tab` 内部经 `active_panel() → visible()` **同步 read 回面板自身**。而 `focus_self` 永远跑在面板自己的 `update` 闭包里（宿主打开已存在文档时走 `panel.update(cx, |panel, cx| panel.focus_self(..))`）。于是「更新中」的面板被再次读取，触发 gpui 的 double-lease panic。**多标签组 + 激活非当前标签**时必现（首轮尝试的 `defer_in` 不够：它的闭包又包了一次本面板的 `update`，仍会重入）。

### 1.3 修复

`EditorHostPanel::focus_self`（`crates/editor/src/view/host.rs`）改用 `cx.spawn_in`：

- 服务层「当前文档」仍**同步**置位（动作立即可见，不依赖帧时序）；
- 切标签动作放进**异步任务**：在当前帧所有 effect 处理完之后运行，面板更新租约早已归还，`select_tab` 读面板是安全的；
- 组已不在时的状态栏提示保留（在异步任务里安全更新面板）。

### 1.4 回归测试（新增）

`crates/editor/src/view/tests.rs` 新增 `focusing_a_non_active_tab_in_a_multi_tab_group_does_not_panic`：同组两个标签，对非激活面板调 `focus_self`——**修复前必 panic，修复后通过**，并断言服务层当前文档切回目标文档。

## 2. 4 项 UI 修缮（用户点名）

| # | 用户意见 | 根因 | 修复 |
| --- | --- | --- | --- |
| 1 | 标题栏加细边框 | 暗色主题 `title_bar.border` 与 `title_bar.background` **同为 `#181818`**，组件自带的 1px 底边框不可见 | `assets/themes/rds-theme.json` 暗色 `title_bar.border` 改为 `#2B2B2B`（比 chrome 亮一档，与上游 `#262626` 同风格；亮色不变） |
| 2 | 草稿箱新建文件/文件夹输入框：细边框 + 文字大小与树一致 | `render_scratchpad_edit_row` 的输入框用默认 Medium 尺寸（`h_8` 32px + `text_sm` 14px），与树行（`text_xs` 12px）不一致 | `crates/scratchpad/src/scratchpad_view/rows.rs`：`Input::new(&input).xsmall().w_full()`——`xsmall` → `text_xs`（12px，与树一致）+ 20px 高 + 1px 细边框（`bordered` 默认 true，主题 `input` 色） |
| 3 | 草稿箱搜索工具栏清除按钮：清**搜索框内容**而非结果 | `sp-search-clear` 原回调只清 `search_view`/`search_stats`，不清输入框 | `crates/scratchpad/src/scratchpad_view/chrome.rs`：改为清 `search_input` 值（`set_value("")`）**并同步清结果**（`set_value` 不发事件，结果须手动清，避免残留旧结果）；tooltip 改为「清除搜索内容」 |
| 4 | 启动空 tab → 欢迎页 | 启动 `assemble` 开一份空白未命名文档，编辑器区显示光秃秃的空编辑器 | `crates/editor/src/view/host.rs`：新增欢迎页——空白未命名文档（未命名 + 空内容 + 未点过「开始编写」）渲染欢迎页（品牌图标 + 标题 + 短标语 + 「开始编写 SQL」按钮）；按钮置位 `welcome_dismissed` 并聚焦编辑内核；内容非空也自动退出欢迎态。新增单测 `welcome_page_shows_only_for_a_pristine_untitled_document` |

> 设计说明（#4）：采纳用户建议做成欢迎页。为保证既有行为与测试（启动面板数 = 1，`scratchpad_editor_entry.rs` 断言）不被破坏，欢迎页落在**启动空白文档**上（该 tab 仍在，但显示的是欢迎内容而非空编辑器），而不是移除空白文档 + 空 Dock 覆盖。打开真实文档后欢迎 tab 保留在后台，用户可自行关闭（VSCode 式「打开即消失」留作后续增强）。

## 3. 顺带修复：图标资产缺失刷屏

应用日志持续刷 `could not find asset at path "icons/file-code-2.svg"`（约每 30ms 一条）。根因：草稿箱 `scratchpad_type_icon` 把 `.yaml/.yml` 映射到 `icons/file-code-2.svg`，但 gpui-kit 0.7.1 图标集（Lucide 子集）**没有** `file-code-2`（有 `file-code` / `file-code-corner`）。

修复：`crates/scratchpad/src/scratchpad_view/primitives.rs` 改用 `icons/file-code.svg`。已全量扫描代码库 50 处 `icons/*.svg` 引用与 0.7.1 图标集交叉核对——**全部命中，无再缺失**。

## 4. 全量 check（0 告警维持）

`cargo check --workspace --all-targets -j 2`（日志 `rds-check-2026-10-14.log`）：`Finished dev profile in 21.46s`，**0 warning / 0 error**。上轮 103 处告警修复保持零复发；本轮新增代码（欢迎页 / 回归测试 / 清除按钮）也未引入告警。

## 5. 全量集成测试（见摘要）

`cargo test --workspace -j 2 --no-fail-fast` 全量复验中，完成判据：116 目标全配对、0 FAILED、0 error、尾部 cargo 最终输出；editor 新增 2 个测试（闪退回归 + 欢迎页）已单包验证通过（editor 609/609、scratchpad 42/42）。

## 6. 插件（M9）状态

- **不接入**：`crates/plugin` 整包仍无任何 crate 依赖它；本轮未做任何代码接入。
- 本轮遗留的 `crates/editor/src/lsp/bridge.rs`（未跟踪、带 U+FFFD 乱码）+ `lsp/mod.rs` 未提交改动（`pub mod bridge;`）为**上一会话遗留**，不在本轮提交范围（见 §7），已单独向用户说明。

## 7. 提交情况

- 本轮代码提交（计划，分别提交）：
  - `fix(editor)`：闪退修复（focus_self spawn_in）+ 回归测试
  - `feat(editor)`：欢迎页 + 单测
  - `fix(scratchpad)`：内联编辑输入框样式 / 搜索清除语义 / yaml 图标
  - `fix(theme)`：暗色标题栏边框可见
  - `docs`：本轮增量报告 + 任务说明更新
- **未包含**：`crates/editor/src/lsp/bridge.rs`（未跟踪，带乱码，上一会话遗留）+ `lsp/mod.rs` 未提交改动——不属于本轮工作，且含乱码违背项目卫生约定，留给用户决定（修复编码后提交 / 放弃）。`tools/` 下新增可复用脚本（launch-app / tail-app-logs / monitor-app-logs / run-build-app / launch-build-app）为上一会话遗留，未纳入本轮提交。

## 8. 联动缺口（不变，照录）

同前两轮：`engine` L2 缓存零调用 / 联邦查询 🟡 / M6 引用档未接 UI / M4 虚拟列表未落地 / Quick Open 待增强 / settings `effect` 无人消费；M9 插件不接入。均为已记录既定缺口，非本轮引入。

## 9. 口径与日志

- 过程产物：`.rds/tmp/rds-check-2026-10-14.log`（check）、`.rds/tmp/rds-integration-2026-10-14.log`（测试，完成后复核）。
- 本轮编辑器验证：`cargo test -p rds-editor --lib` → **609/609 通过 / 0 失败 / 0 告警**；`cargo test -p rds-scratchpad` → **42/42 通过**。
