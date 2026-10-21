# GPUI-kit 0.7.1 → 0.8.0 升级验证报告

- 分支：`upgrade/gpui-kit-0.8.0`
- worktree：`D:\rds-upgrade-work`（独立 target，不干扰 main 工作区）
- 验证日期：2026-10-21
- 基线：main `87d61400`（与 main 同 commit 起分支）

## 1. 结论

**编译与全量测试全部通过，可合并。** `cargo build --workspace` 全绿；`cargo test --workspace` 127 套件 / **3073 通过 / 0 失败**。迁移评估中 153 显式 + 53 通配符号引用全部编译解析，无 API 硬断。剩余工作仅两类：① 纯视觉/交互行为需人工运行 app 复核；② 升级后 UI 优化（TabBar 5 处消费点接线），不阻塞合并。

## 2. 依赖与 vendor

| 项 | 值 |
| --- | --- |
| 根 Cargo.toml | `gpui-kit = "0.8.0"`（74 行），`[patch.crates-io]` 指向 `vendor/gpui-base`、`vendor/gpui-component` |
| vendor 来源 | crates.io 发布包（**非** GitHub 仓库源码——仓库源码 Cargo.toml 用 `workspace=true` 继承，直接复制无法解析 manifest） |
| gpui-base 0.8.0 | sha256 `d5cd7de7…b1823a7`，与 crates.io API checksum 逐字符一致 |
| gpui-component 0.8.0 | sha256 `4097e1…b6c2d1`，与 crates.io API checksum 逐字符一致 |
| 发布包归档 | `.rds/tmp/gpui-base-0.8.0.crate`、`.rds/tmp/gpui-component-0.8.0.crate` |

## 3. 项目 patch 清单（vendor 内，均保留）

### gpui-base（`src/input/base/state.rs`，+1299/-69）

0.7.1 vendor patch 的 6 项，0.8.0 下核对后重打：

| # | patch | 0.8.0 状态 | 说明 |
| --- | --- | --- | --- |
| ① | `add_cursor_above/below` pub 化 | 0.8.0 官方已 pub，保留签名 | 兼容 |
| ② | `add_cursor_above_line/below_line` | 仍缺失 | 内部调 `add_cursor_vertical` |
| ③ | `collapse_cursors` | 仍缺失 | `undo_manager.break_transaction_coalescing` + `selections.remove_all_but_active`（0.8.0 均 crate 内可见） |
| ④ | `cursor_count/cursor_offsets` | 仍缺失 | `selections.len` / 遍历 `cursor_offset`，插入在 `active_selection` 之前 |
| ⑤ | `select_columnar_block` | 官方两版（0.7.1/0.8.0）均无 | 从 0.7.1 vendor 移植（`git show HEAD`），依赖 0.8.0 `ColumnarPoint::new`（state.rs:320/325 pub(super)）与 `build_columnar_selection`（2509 pub(super)），签名兼容 |

### gpui-component（`src/dock/mod.rs`，+105）

| patch | 说明 |
| --- | --- |
| `SkinShared` + `tab_bar_max_width: Cell<f32>`（默认 200.0）、`tab_bar_variant: Cell<TabVariant>`（默认 Tab）+ getter | DockSkin 配置承载 |
| `DockSkin::set_tab_bar_max_width/set_tab_bar_variant` | notify 重绘；tab_panel.rs TabBar 链 `.with_variant(...).max_width(...)` 消费 |

- 0.7.1 的 `title_suffix` patch：0.8.0 官方已原生支持，无需保留。
- 注意：`set_tab_bar_max_width/variant` 两方法 0.7.1/0.8.0 官方均无，属项目私有扩展（`crates/workbench/view.rs::apply_tab_bar_settings` 调用）。

## 4. 编译验证

- `cargo build --workspace`：**全绿**。
- 产物：`target/debug/rds_app.exe`、`target/debug/pg_probe.exe`（已确认存在）。
- 环境要点：
  - worktree 需主工作区 `third_party/duckdb/1.5.5`（40MB，含 duckdb.lib/dll）才能链接；已复制。
  - 运行测试需 `duckdb.dll` 在 `target\debug\`（已复制，36,724,520 字节）。

## 5. 测试验证

- 命令：`cargo test --workspace -j 2`（**-j 2 降并行**——默认并行下链接多个测试 exe 触发内存不足：LNK1102/0xc00000fd/os error 1453）。
- 结果：**127 套件 / 3073 passed / 0 failed**；doc-tests 全过（1 ignored 为 db_icons 说明示例）。
- 日志：`.rds/tmp/test_run4.log`（3625 行，全绿；无 panic/FAILED/链接错误）。

## 6. 行为回归（B1-B20）对照

回归面分三层，均已核实：

| 层 | 证据 |
| --- | --- |
| 编译面（API 硬断） | 迁移评估 153 显式 + 53 通配引用全部编译通过；B4 Tooltip 94 处消费点、Dock/DataTable/ContextMenu/ListView/TextView/ProgressCircle/TestWindowExt 等全部解析 |
| 自动化契约层 | 项目 3073 测试全绿（含 tooltip/dock/tabbar/select/table/list/textview 相关契约测试、草稿箱/导航树等业务测试） |
| 视觉/交互层 | **需人工运行 app 复核**：Dock 拖拽/吸附、TabBar 滚轮与裁剪、PopupMenu 弹层、Select 下拉、编辑器滚动等纯视觉行为 |

## 7. 升级后 UI 优化清单（不阻塞合并）

TabBar 5 处消费点均未接线 0.8.0 新能力（升级后建议补）：

| 消费点 | 位置 | 建议 |
| --- | --- | --- |
| 连接 Tab | `connection-dialog render.rs:526`（conn-tabs underline Small） | `.track_scroll(&ScrollHandle)` + `.menu(true)` |
| scope 分段 | `render.rs:1822`（scope-seg segmented） | 同上 |
| Insight Tab | `insight view/mod.rs:67`（insight-tabs underline Small） | 同上 |
| 项目选择器 Tab | `project ui.rs:1865`（picker-tabs segmented + tab_stop） | 同上 |
| 编辑器结果集 | `editor results/sets.rs:111`（editor-result-sets segmented Small） | 同上 |
| 设置页泛型 TabBar | `settings_page.rs:816` | 同上 |

0.8.0 提供：`.menu(true)`（overflow 菜单）、`.max_width()`、`.track_scroll(&ScrollHandle)`、TabReveal 弹簧滚入、`ScrollableMask::vertical_wheel`（滚轮方向修复）、TabVariant Folder 变体。

## 8. 风险与遗留

| 项 | 状态 |
| --- | --- |
| 内存受限 | worktree 全量并行编译/链接测试目标时 OOM（机器 16GB、空闲 ~9GB）；`-j 2` 稳定通过。正式机器跑测试建议同样降并行 |
| duckdb.dll 部署 | worktree `target\debug\duckdb.dll` 需手动复制（主工作区有）；若换 target 目录需重复制 |
| origin/main 推送 | 未推送，本地领先（用户拍板后推送） |
| main 工作区暂存改动 | `crates/editor/src/shared.rs / view/host.rs / view/tests.rs`（用户【2d-笔记级】功能，324 行未提交）——合并升级分支前需先提交或处理，避免与升级分支合并冲突 |
| 旧分支 refactor/ui-standalone | 删留待用户拍板 |
| TabBar 视觉复核 | 依赖用户运行 app 人工确认（B7 回归项） |
