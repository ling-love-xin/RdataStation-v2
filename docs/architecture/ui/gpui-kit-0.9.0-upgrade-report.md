# GPUI-kit 0.8.1 → 0.9.0 升级 + gpui-fast 启用报告

> 分支 `upgrade/gpui-kit-0.9.0`（worktree `D:\RdataStation\RDS\RdataStation-v2-upgrade`），基线 `9ce3c98e`，2026-11-08。
> 与本文同批提交的代码：根 `Cargo.toml`、`vendor/gpui-base`、`vendor/gpui-component`、`crates/app/build.rs`、`Cargo.lock`。
> 上一版参考：`docs/architecture/ui/gpui-kit-0.8.0-upgrade-report.md`（0.8.1 过程见其 §9）。

## 1. 结论

1. **gpui-kit 0.9.0 迁移完成，项目侧零改动**：`cargo check -p rds-app` 全绿（0 error / 0 warning），27 个 workspace member 无一需要改代码。0.9.0 的 API 断裂面全部被两个 vendor 包的私有 patch 吸收，没漏到业务 crate。
2. **gpui-fast 已启用**（`gpui-kit` 加 `features = ["gpui-fast"]`），编译面全绿；`gpui-fast` 0.1.13 渲染内核与 `gpui-pre` 0.3.11 两套核心同编、二进制不双份。
3. **manifest 前置已核清，结论是「不需要应用侧自嵌」**：gpui-fast 走 `windows-manifest-provided-by-host`（自己不再嵌清单），但我们实测发现 `gpui-pre` 0.3.11 的 build.rs 在其默认 feature `windows-manifest` 下已经嵌了一份清单（ID=1），内容与 gpui-fast 的要求逐项吻合。应用侧再嵌一份会**资源重复**，链接期 `CVT1100` → `LNK1123`（本轮真踩到了，日志见 §5）。因此 `crates/app/build.rs` 保持只嵌 icon，并在注释里写清「清单由谁提供、什么情况下才需要自嵌」。
4. **启动冒烟通过，交互未验**：编译、链接、进程启动（12s 存活、0 panic）都实测过了；IME、多光标、折叠、拖拽、a11y、滚动性能均未实测（见 §7）。
5. **`gpui-fast` 的 6 条失败已定性 → 适合开启**：A/B 三组把归因钉在 fast 上（0.9.0 无 fast 全绿，0.8.1 也全绿）；失败点全落在 gpui-kit 的**观察数据**（`Weak` 元素注册 + 绘制记录复用），没有一条是「画面渲染不对」。代价是全仓 37 处 `debug_bounds(..).is_none()` 的测试口径失真，需单独一轮「测试观察层」迁移；仍欠一次真机目视（对话框关闭无残影等 5 项）。详证见 §7 第 5 条。

## 2. 依赖与 vendor

| 包 | 0.8.1 | 0.9.0 | 来源 |
| --- | --- | --- | --- |
| `gpui-kit` | 0.8.1 | **0.9.0** | crates.io（2026-11-02 发布） |
| `gpui-base` | 0.8.1 | **0.9.0** | `vendor/gpui-base`（path + `[patch.crates-io]`） |
| `gpui-component` | 0.8.1 | **0.9.0** | `vendor/gpui-component`（path + `[patch.crates-io]`） |
| `gpui-component-macros` | 0.8.1 | 0.9.0 | crates.io（传递依赖） |
| `gpui-kit-assets` | 0.8.1 | 0.9.0 | crates.io（传递依赖） |
| `gpui-pre` / `gpui-pre-platform` | 0.3.10 | **0.3.11** | 上游 0.9.0 钉 `=0.3.11` |
| `gpui-fast` 全族（fast / platform / web / windows / wgpu / macros / apple / linux / macos） | 0.1.12 | **0.1.13** | crates.io（传递依赖） |
| `lsp-types` | 0.97.0 + `proposed` | 不变 | 必须与 `gpui-base` 的 trait 签名同版本 |

vendor 两个目录是**整目录替换为 0.9.0 发布包源码**（4.8 MB / 4.2 MB），再在发布包上重打项目私有 patch；`[patch.crates-io]` 未动。

## 3. 上游 0.8.1 → 0.9.0 变更面与 patch 迁移

**上游变更面**（GitHub `longbridge/gpui-kit` compare `v0.8.1...v0.9.0`）：151 modified / 10 added / **0 deleted**，即纯增量、无符号删除。实质改动集中在输入内核：`crates/base/src/input/base/blink_cursor.rs` 整体重写（+429/−80），连带文本选择与光标的若干行为。

**为什么必须重打 patch**：私有 patch 里有 4 个符号在上游 0.9.0 中仍然是 **0 命中**——`add_cursor_above_line`、`add_cursor_below_line`、`collapse_cursors`、`select_columnar_block`。这些是本仓列编辑（多光标）与折叠保护的实际依赖，不能指望上游。

**迁移手法**：`git apply` 在本机不可用——vendor 发布包与 patch 的行尾（CRLF/LF）不一致，13 个 hunk 全部 reject（`while searching for ...`）。改为**锚点字符串替换**：每条替换先断言在目标文件中出现次数（`count == expect`），再落盘，失败即退出非零。两个包各一次通过。

gpui-base（4 文件）：

| 文件 | 变更点 |
| --- | --- |
| `src/input/base/state.rs` | `LineDelta` 结构体 + `last_edit_delta` 字段/初始化 + `take_last_edit_delta()` + `record_edit_delta()`（三处 `adjust_folds_for_edit` 之后各插一行）+ `cursor_count()` / `cursor_offsets()` + `add_cursor_above/below` 由 `pub(super)` 提为 `pub` + `add_cursor_above_line` / `add_cursor_below_line` / `collapse_cursors` / `select_columnar_block` + 两处折叠候选保护（`set_fold_candidates` 空则跳过、`update_fold_candidates_for_edit` 空增量跳过） |
| `src/input/editor/display_map/display_map.rs` | 新增 `has_fold_candidates()` |
| `src/input/base/element.rs` | `editor_gutter_bounds` 改为 `let bounds = ...; bounds` 形式 |
| `src/resizable/resize_handle.rs` | `HANDLE_PADDING`：`px(4.)` → `px(2.)`（分割条更细） |

gpui-component（7 文件）：

| 文件 | 变更点 |
| --- | --- |
| `src/dock/mod.rs` | `DockSkin` 加 `tab_bar_max_width: Cell<f32>`（默认 200.0）与 `tab_bar_variant: Cell<TabVariant>`（默认 `Tab`）+ 两个 getter + `set_tab_bar_max_width` / `set_tab_bar_variant`（RDS 设置项 `appearance.*` 的消费点） |
| `src/dock/tab_panel.rs` | `menu::{DropdownMenu as _, PopupMenuItem}`；`TabBar` 加 `.with_variant(...)` / `.max_width(...)`；`visible.clone()`；整段右键菜单 `on_context_menu`（缩放 / 关闭 / 关闭其他 / 关闭全部） |
| `src/input/input.rs` | `set_editor_paddings` 先算 `paddings` 变量；代码编辑器左边距 `px(0.)`（行号顶到最左） |
| `src/input/mod.rs` | re-export `register_syntax_context_provider` + `SyntaxContextFactory`（自研 SQL 词法判定的注册点） |
| `src/input/syntax_context.rs` | `HashMap` + `Arc` + `LazyLock` + `Mutex` 全局注册表、`custom_providers()`、`register_syntax_context_provider`、`syntax_context_provider` 两个分支（有/无 tree-sitter）都先查自定义再回退 grammar |
| `src/notification.rs` | `timeout: Option<Duration>` 字段 + `.timeout()` builder + Toast 用 `timeout.unwrap_or(5s)` |
| `src/tab/tab.rs` | `on_context_menu: Option<Rc<...>>` 字段 + builder + render 末尾 `tab_element` 双分支（有菜单包 `context_menu`，否则原样 `into_any_element()`） |

patch 注释一并从英文 `RdataStation vendor patch (0.8.1)` 改为【项目私有 patch】中文说明，并补了 `take_last_edit_delta` 缺失的文档注释。

## 4. 项目侧改动

1. **根 `Cargo.toml`**：`gpui-kit = "0.8.1"` → `{ version = "0.9.0", features = ["gpui-fast"] }`，并补 0.9.0 沿革注释与 gpui-fast 说明。
2. **`crates/app/build.rs`：结论是不改行为，只补注释**。第一版实现里我加了自嵌 manifest（PerMonitorV2 / SegmentHeap / comctl32 v6 / asInvoker），`cargo build` 链接失败：

   ```
   CVTRES : fatal error CVT1100: 资源重复。类型: MANIFEST，名称: 1，语言: 0x0409
   LINK : fatal error LNK1123: 转换到 COFF 期间失败: 文件无效或损坏
   ```

   原因链：`gpui-kit` 带 `gpui-fast` 时给 `gpui-fast` 开 `windows-manifest-provided-by-host`（所以 gpui-fast 自嵌逻辑直接 return），但依赖图里同时有 `gpui-pre`，而 `gpui-pre` 0.3.11 的 build.rs 在默认 feature `windows-manifest` 下用 `embed-resource` 编 `resources/windows/gpui.rc` —— 第一个清单资源 ID=1 就是它。再叠加一份同 ID 的清单即重复。
   
   `gpui-pre` 内置清单（`gpui-pre-0.3.11/resources/windows/gpui.manifest.xml`）内容：`asInvoker` / Win10 supportedOS `{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}` / `dpiAware true/pm` / `dpiAwareness PerMonitorV2` / `heapType SegmentHeap` / comctl32 v6 —— 与 gpui-fast 的要求逐项一致，因此**不需要应用侧自嵌**。
   
   最终 build.rs 保持「只嵌 icon」的老行为，并补注释说明：清单当前由 gpui-pre 提供；只有将来去掉 gpui-pre、只留 gpui-fast 时，才需要在应用侧补一份（那时 gpui-fast 也不嵌，exe 会完全没有清单，高 DPI 会模糊）。
3. **`Cargo.lock`**：`cargo update -p gpui-kit --precise 0.9.0` 连带把 `icu_*` 2.3.x→2.4.0、新增 `icu_segmenter`、移除 `metal 0.33.0`；另有 `criterion` / `clap` / `ciborium` / `plotters` / `oorandom` / `perf-event2` 进 lock——这些是 `gpui-base` 0.9.0 的 **dev-dependencies**（bench 用），不参与应用构建，只占 lock 行数。

## 5. 编译验证（worktree 独立 `target/`，`-j 2`）

| 命令 | 结果 | 耗时 | 日志 |
| --- | --- | --- | --- |
| `cargo check -p gpui-base` | exit 0，0 error | 3m17s | `.rds/tmp/check64-base.log` |
| `cargo check -p gpui-component` | exit 0，0 error | 1m14s | `.rds/tmp/check64-comp.log` |
| `cargo check -p rds-app`（未开 fast） | exit 0，**0 error 0 warning** | 3m20s | `.rds/tmp/check64-app.log` |
| `cargo check -p rds-app`（开 fast） | exit 0，**0 error 0 warning** | 2m17s | `.rds/tmp/check64-fast.log` |
| `cargo build -p rds-app`（开 fast） | **exit 0**，产物 `target/debug/rds-app.exe` 153.7 MB | 首轮链接失败，见下 | `.rds/tmp/build64-fast2.log` |
| 启动冒烟（`rds-app.exe` 直接拉起 12s） | **存活未退出**，stderr 47 行全是正常 INFO（nav view / project.db / analytics.duckdb 初始化），`panic` / `ERROR` 命中 0 | — | `.rds/tmp/run-err.log` |

**首轮 `cargo build` 失败记录（已在 §4.2 修掉）**：`CVTRES : fatal error CVT1100: 资源重复。类型: MANIFEST，名称: 1` + `LINK : fatal error LNK1123`——原因是我在 `crates/app/build.rs` 里自嵌了一份 manifest，与 `gpui-pre` 自带的那份 ID 撞车。去掉自嵌后一次链接通过。

开 fast 那次日志里可确认：`Compiling gpui-fast v0.1.13` / `gpui-fast-windows` / `gpui-fast-macros` 与 `gpui-pre v0.3.11`、`gpui-base v0.9.0(vendor)` 同时出现——**双核心并按**，符合预期。

## 6. gpui-fast 启用说明

- feature 定义（`gpui-kit` 0.9.0 `Cargo.toml` L50-57）：`gpui-fast = ["dep:gpui-fast", "dep:gpui-fast-platform", "dep:gpui-fast-web", "gpui-base/gpui-fast", "gpui-component?/gpui-fast", "gpui-kit-assets?/gpui-fast"]`。
- 打开后依赖图里同时有 `gpui-fast*`（0.1.13）与 `gpui-pre*`（0.3.11）两套核心：编译时间与磁盘各多一份，**二进制里不会出现两套渲染器**（只有一个入口被宿主使用）。
- `gpui-fast` / `gpui-fast-platform` 依赖 `windows-manifest-provided-by-host`：清单必须宿主/同图其它 crate 提供。本项目由 `gpui-pre` 的默认 `windows-manifest` 提供（见 §4.2），**应用侧不要再嵌**，否则 `CVT1100` 资源重复。
- release 构建另需 `fxc.exe`（`gpui-fast-windows` 仅在 `not(debug_assertions)` 编 HLSL）；本机在 `C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\fxc.exe`，不在 PATH，可用 `GPUI_FXC_PATH` 指过去。debug 构建不涉及。

## 7. 未验证 / 风险

1. **交互层零验证**：0.9.0 重写了 `blink_cursor`（+429/−80），影响光标闪烁与文本选择；中文 IME 这段是 `gpui-fast` 的 `composition.rs`（78 KB）在管，与 gpui-pre 的实现不同源。本轮只证实了「进程能起、日志无 panic」，必须实机回归：中文输入法组字、选区拖拽、双击/三击选词、Shift+方向键选、列编辑、折叠、对话框层级、拖拽落点、a11y。
2. **性能未测**：没有滚动帧时间 / 首帧 / 内存基线数据。`engine` 侧结果集仍有 10000 行硬顶，要测 10 万行滚动得先放开或走 `load_more`。
3. **业务代码 0 报错 ≠ 行为等价**：0.9.0 的断裂面被 vendor patch 吸收，说明项目依赖的旧签名都还在；但语义变化（尤其光标/选择）不会体现在编译错误里。
4. **patch 与上游长期分叉**：4 个符号上游至今没有，`resize_handle` 的 padding 是项目口味，`syntax_context` 注册表是项目扩展——每次 gpui-kit 升级都要重演 §3 的迁移动作，成本随 patch 面增长。
5. **✅ 已定性（2026-11-08 补）：`gpui-fast` 开启后 6 条 UI 用例失败的成因与「是否适合开启」的结论**
   全量 `cargo test --workspace -j 1 --no-fail-fast` 在开 `gpui-fast` 时共 8 条失败：2 条是仓库既有 `rds-editor` 失败（与本升级无关），
   另 6 条的定性如下。

   **A/B 三组对照（同一份 0.9.0 代码，只切 feature）**

   | 组 | gpui-kit | `gpui-fast` | `dialog_window` 10 条 | 结论 |
   | --- | --- | --- | --- | --- |
   | A | 0.8.1（`main`） | 关 | 10/10 通过 | 基线 |
   | B | 0.9.0 | 关 | 10/10 通过 | 0.9.0 的 API 变更**不背这个锅** |
   | C | 0.9.0 | 开 | 8 通过 / 2 失败 | **归因 `gpui-fast`** |

   **两点机制（源码级，已核）**

   1. `gpui_kit::test` 的观察数据来自两套记录：
      - **元素注册表**：`gpui-base/src/test_support.rs:171-172` 是 `Weak` 注册（`REGISTRY: HashMap<WindowKey, HashMap<GlobalElementId, Weak<Registration>>>`），
        元素一旦不在本帧被构建、且 arena 被清，`Weak` 就失效 → `missing ElementId`（panic 出处 `gpui-kit-0.9.0/src/test.rs:140-143`）；
      - **绘制记录表**：`cx.debug_bounds(..)` 读 `window.rendered_frame.debug_bounds`（`gpui-pre-0.3.11/src/app/test_context.rs:1031-1032`）。
   2. `window.draw(cx).clear(cx)` 里的 `clear` 清的是**元素 arena**（`gpui-pre-0.3.11/src/window.rs:484` `ArenaClearNeeded::clear`），
      而 retained 渲染对**内容未变的子树不重新构建**（`Frame::reuse_paint` 把上一帧 `debug_bounds_records[range]` 回灌新帧，
      `gpui-fast-0.1.13/src/window.rs:4370-4374`）——于是「本帧没重建的东西查不到」与「本帧没画的东西记录里还在」两种情况都能出现。

   **逐条定性（6 条都是「观察口径 vs 复用」，不是画面错误）**

   | 用例 | 断言形态 | 在 fast 下为什么失败 | 性质 |
   | --- | --- | --- | --- |
   | `dialog_window.rs` L445 `version-restore` | `debug_bounds(..).is_none()`（断定“应消失”） | 关闭后的元素记录被复用子树回灌，`Some` 残留 | 记录表口径（画面不会残留，父级重建即消失） |
   | `dialog_window.rs` L525 `repair-adopt-0` | 同上 | 同上 | 记录表口径 |
   | `right_sidebar_alone_renders_draft_row` | `try_find("mock-draft-open")` | 该行是静态节点，本帧未重建 → `Weak` 失效 | 注册表口径 |
   | `nav_tree_rows_are_laid_out_by_the_virtual_list`（“对象行应被画出”） | 注册表查行 | 虚拟列表已渲染过的行不再重建 | 注册表口径 |
   | `mock_view::column_dialog_shows_type_mismatch_warning`（“应渲染警示行”） | 注册表查行 | 同上 | 注册表口径 |
   | `header_interaction::clicking_the_title_row_collapses_and_restores`（“展开时正文应可见”） | 注册表查正文 | 折叠/展开状态未触发重建（或按 `cx.notify()` 才重建） | 注册表口径（也可能是缺失效点，需目视） |
   | `workbench/tests/scratchpad_editor_entry.rs:91`（“请求应已被消费”） | 状态断言 + 观察 | 同上 | 观察口径 |

   **结论：`gpui-fast` 适合开启（建议作为默认开启的 feature），但不能把「测试全绿」当门槛。**
   理由：① A/B 已把归因钉在 fast 上，且 0.9.0 本体干净；② 6 条的失败点全部落在 gpui-kit 的**观察数据**（`Weak` 注册 / 绘制记录复用），
   报告里没有一条是「渲染出来的画面不对」；③ 画面正确的判断依据是「父级内容变了就会重建」，而 6 条里只有标题栏折叠那条存在
   「更新但没 notify」的嫌疑（retained 模式的接线成本，正是上游 README 要求的那条：改了 tracked 之外的状态要 `cx.notify()`）。
   **代价与待办**：全仓 **37 处** `debug_bounds(..).is_none()` 断言与 6 处可见性断言在 fast 下口径失真，
   需要一次「测试观察层」迁移（断言前强制整帧重建 / 换用 `snapshots()` 全集比对），与业务代码无关；这件事单独开一轮做，不要混在依赖升级里。
   **仍需真机目视一次**（本轮未做，明确记为未验证）：对话框关闭后无残影、右 Dock 草稿行可见、导航虚拟列表行可见、
   列类型警示行可见、标题栏折叠/还原。这些点一旦出问题就是「缺 `cx.notify()`」，与测试口径无关。

## 8. 复现步骤

```powershell
# 1) 换 vendor 发布包源码（两包各一次）
#    归档取自 static.crates.io：gpui-base-0.9.0.crate / gpui-component-0.9.0.crate
# 2) 在发布包上重打私有 patch（锚点替换，不要用 git apply）
python .rds\tmp\port_base.py --apply
python .rds\tmp\port_comp.py --apply
# 3) 改根 Cargo.toml 后重算 lock
cargo update -p gpui-kit --precise 0.9.0
# 4) 三级验证
cargo check -p gpui-base -j 2
cargo check -p gpui-component -j 2
cargo check -p rds-app -j 2
```

诊断产物（不入库；**随临时工作区清理已删除**）：`.rds/tmp/pristine/`（干净解包）、`.rds/tmp/patches/*.patch`（0.8.1 上的 patch 原文）、`port_base.py` / `port_comp.py`。仅 `port_gutter.py` 保留在 `tools/_archive/`（该目录 gitignore，属本机过程资产）。

## 9. rebase 到 main 之后：书签列 patch 的二次移植（2026-11-08）

**背景**：本分支起了之后 main 往前走了 26 个 commit（mock panel 拆分那批），其中 `8234474b` /
`a569df57` 在 **0.8.1 的 vendor 上**加了「书签列 / 折叠说明」这套 UI 私有 patch。把 vendor 整体换成
0.9.0 之后，两边的改动在同一个文件上撞了。

**冲突表象**：`git rebase main` 第 1/15 个 commit 就在
`vendor/gpui-base/src/input/base/element.rs` 冲突。取值时应取**我们的 0.9.0**（目录整体替换），
但随后发现是**半成品状态**：

| 文件 | main 的 patch 量 | rebase 后实际落进 0.9.0 的 |
| --- | --- | --- |
| `input/base/state.rs` | 224 行 | 91 行（自动合并能对上的部分） |
| `input/base/element.rs` | 464 行 | **0 行**（冲突，取了 0.9.0 侧） |

后果是「数据结构在、绘制与点击都不在」——`GutterMarker` / `set_gutter_markers` /
`InputEvent::GutterMarkerClick` 都还在，`element.rs` 里却一处都没引用它们：**书签会静默消失**，
而且编译器不会报错（只是死字段）。所以不能就这么合。

**为什么不能改走 0.9.0 自带的 `gutter_decorations`**：0.9.0 新增的 `GutterDecoration` 是**只画不点**
（`InputExtras::gutter_decorations` 返回 `&GutterDecoration`，内核没有装饰点击事件）；而我们的书签要
「点击切换 / Shift 开说明 / 悬停弹说明气泡」，缺了点击就只剩半个功能。因此**保留私有 patch**，
把它移植到 0.9.0。

**移植方法**（可复用）：

1. 先试三方合并：base=`pristine/gpui-base-0.8.1`、ours=0.9.0、theirs=main 打过 patch 的 0.8.1，
   `git merge-file -p`。**失败**——element.rs 30 处冲突、state.rs 50 处，粒度太细。
2. 改用「**只搬新增块 + 锚点替换**」（与 §3 同一个套路）：常量段、两个结构
   （`GutterMarkerLayout` / `HoverNote`）、两个方法（`layout_gutter_markers` /
   `paint_gutter_markers`）按**行号原样**搬过来；9 个接线点用「字符串替换 + `count == expect` 断言」，
   全部一次通过（脚本 `tools/_archive/port_gutter.py`；临时工作区清理时已移入该归档区）。
3. **行尾坑**：工作区文件是 CRLF，直接按 `\n` 断言会全部落空（第一次跑 15 处全 0）；脚本必须
   `read → replace("\r\n","\n") → 编辑 → 写回时还原 CRLF`。
4. 适配 0.9.0 的差异（编译报错逐个消）：`Cell` / `MouseDownEvent` / `TransformationMatrix` 补导入；
   行号数字的右移改成**叠加** `line_number_offset + gutter_marker_layout.column_width`
   （0.9.0 自带 `gutter_decorations` 的 offset，不能覆盖它）。

**验证**：`cargo check -p gpui-base` 0 error 0 warning；`-p gpui-component -p rds-editor -p rds-app`
0 error（仅 `rds-mock` 2 条既有告警）。提交标题「fix(vendor): 把 main 的书签列 / 折叠说明 patch
移植到 gpui-kit 0.9.0 内核」——**hash 每次 rebase 都会重写，以标题为准**（并入主线时是 `dce9433d`）。

**这一节的教训**：「vendor 目录整体替换」在 rebase 时**不能只看冲突列表**——自动合并会静默保留对方
patch 的一部分（这里 state.rs 落了 41%），剩下的部分既不报错也不生效。判断法是挑一个**只在该 patch
里出现的符号**，在目标版本里 grep：`GutterMarkerKind` 在 vendor 里只有 state.rs 命中、
element.rs 里 0 命中，就说明绘制侧丢了。

## 10. rebase 后全量回归（2026-11-08）

命令：`cargo test --workspace -j 1 --no-fail-fast --exclude rds-mock`（在 rebase 到 `a569df57`
的那一版上跑；并入主线前 main 又前进了 2 个 commit，见 §11）。

**排除 rds-mock 的原因（与升级无关）**：main 自己的 `crates/mock/src/generators/text.rs:222` 写着
`mod tests;`，而 `generators/text/tests.rs` 还没提交——`rds-mock` 的**测试目标**编译不过
（`E0583`）。`rds-mock` 作为**库**照常编译并被其它 crate 依赖，只是它自己的单测这一个 target 跳过。
同理 `--all-targets` 的 workspace check 也会卡在这一处；除它之外 `cargo check -p gpui-base` /
`-p gpui-component -p rds-editor -p rds-app` 全 0 error。

**结果：135 个测试目标，3502 passed / 9 failed。**

| 目标 | 结果 | 说明 |
| --- | --- | --- |
| `rds-editor --lib` | 758 passed / **2 failed** | `view::tests::legacy::opening_an_analysis_document_builds_a_cell_editor_per_cell`（`legacy.rs:58`）、`view::tests::notebook_win::analysis_cell_renders_multiple_output_blocks`（`notebook_win.rs:347`）——rebase 前基线就是这 2 条 |
| `rds-analytics-resource --test dialog_window` | 8 / **2 failed** | `version_dialog_lists_rows_and_shows_actions_after_selection`（`L445`，即旧名 `version-restore`）、`index_repair_dialog_groups_rows_and_shows_inline_actions`（`L525`，`assertion failed: cx.debug_bounds("repair-adopt-0").is_none()`）——gpui-fast 观察层口径 |
| `rds-connection-dialog --lib` | 66 / **1 failed** | `header_interaction::clicking_the_title_row_collapses_and_restores`（`helpers/tests.rs:1076`，`展开时正文应可见`） |
| `rds-database --lib` | 302 / **1 failed** | `nav_view::tests::nav_tree_rows_are_laid_out_by_the_virtual_list`（`nav_view/tests.rs:812`，`对象行应被画出`） |
| `rds-workbench --test mock_panel_probe` | 4 / **1 failed** | `right_sidebar_alone_renders_draft_row`（`missing ElementId Name("mock-draft-open")`，panic 落在 `gpui-kit-0.9.0/src/test.rs:142`） |
| `rds-workbench --test scratchpad_editor_entry` | 0 / **1 failed** | `an_open_request_lands_in_a_sql_editor_tab`（`scratchpad_editor_entry.rs:89`，`请求应已被消费（取出即清空）`） |
| `rds-ui-guard --test workspace_ui_contract` | 3 / **1 failed** | `no_new_hardcoded_color_or_pixel` —— **main 自带，与本次升级无关**（见下） |

前 6 条与 rebase 前的基线**逐条对应**。基线里那第 7 条 UI 观察层用例
`mock_view::tests::column_dialog_shows_type_mismatch_warning` 属 `rds-mock`，本轮被 exclude 没跑；
并入前 main 的 `5675df1e` 删掉了那条悬空的 `mod tests;`，于是**补跑**
`cargo test -p rds-mock -j 2`：**292 项全绿、0 failed**，其中
`mock_view::tests::column_dialog_shows_type_mismatch_warning ... ok` ——
**它在 main 的 mock 拆分之后就复现不了了**（不再走那条受复用绘制影响的断言路径）。
所以完整账目是 **3794 passed / 9 failed**（3502 + 292），基线的 7 条 UI 观察层里 6 条复现、1 条消失。

第 7 个失败目标（`rds-ui-guard`）是新出现的：

- `no_new_hardcoded_color_or_pixel` 报的是 `crates/result_panel/src/grid/fit.rs`（未登记的新文件，裸色值 0 / 裸 px 6）与 `crates/result_panel/src/ui.rs`（`px(4) -> 12`）。这两个文件来自 **main 的 `44040e21`「列宽自适应」**，本分支一行都没碰 `crates/result_panel/`（`git diff --name-only a569df57..HEAD` 里没有它）。也就是说：main 上这条守卫本来就是红的，只是 main 当前测试编译不过（`rds-mock` 的 `mod tests;`），没人跑过它。
- 处置：**不在升级分支里改别人的 UI 代码**（提常量的量值、登记 BASELINE 都是设计取舍）。记在这里当交接项，由 result_panel 那侧决定。

结论：**本次升级没有引入新的业务失败**；gpui-fast 的 6 条仍然是「观察层口径」那批（§7 第 5 条），
`9aacef30` 的书签列移植没有把相关用例打红。

**书签列移植的行为证据**（不只是「编译过」）——`rds-editor --lib` 里这三条全 `ok`：

```
test view::tests::bookmarks::bookmarks_project_into_gutter_markers ... ok
test view::tests::bookmarks::gutter_click_toggles_bookmark_and_shift_opens_note ... ok
test view::tests::bookmarks::bookmark_hover_note_setting_off_strips_note ... ok
test view::tests::core::folding_candidates_are_fed_after_the_gutter_restored ... ok
```

第一条断言的是 `GutterMarker` 列表真的从编辑器投进 gutter（绘制侧数据源在了）；第二条断言**点击
能切换书签、Shift 点击能开说明**（这正好是「0.9.0 自带 `gutter_decorations` 只画不点」那半个功能
被补回来的证据）；第三条断言关掉设置后说明被剥掉。仍**未做**的是真机目视：书签列是否真的画在
行号左侧、折叠说明下划线位置、悬停气泡形态。

## 11. 二次 rebase 与并入主线（2026-11-08）

第一次 rebase 到 `a569df57` 之后、准备并入之前，main 又往前走了 **2 个 commit**：

| commit | 内容 |
| --- | --- |
| `3cdf651f` | feat(editor): 折叠末行一律收起（候选侧统一 `end+1`，对齐 VS Code） |
| `5675df1e` | fix(mock,editor): 清掉 gutter 收尾后遗留的技术债 |

**碰面分析**：这两个 commit 改的是 `crates/editor/src/support/fold.rs`、`view/fold.rs`、
`view/host/toolbar/render.rs`、`view/tests/core.rs`、`crates/mock/**`、`editor-backlog.md`；
本分支改的是 vendor 两包、`Cargo.toml`/`lock`、`crates/sql`、`crates/settings`、
`crates/editor/{language/language_service, lsp/sql_hover.rs, support/translate.rs, view/host/{format,session}.rs}`
与若干 docs。**改动面零重叠**，二次 `git rebase 5675df1e` 17/17 一次过、无冲突。

**复验（折叠正好和书签列在同一片区域，只跑 `rds-editor` lib）**：`757 passed / 2 failed`
（失败仍是 §10 那两条既有），其中 bookmark / gutter 相关 **28 条全 `ok`**，fold 相关**零失败**。
总数从 761 变 760 的原因**不是丢用例**：main 把 fold 用例改名合并了——旧名
`a_structural_block_wins_over_an_indent_block_on_the_same_line` /
`kernel_end_line_keeps_the_last_content_line_of_an_indent_block` /
`kernel_end_line_tucks_in_the_closing_line_of_a_structural_block` /
`kernel_ranges_keep_the_last_content_line_of_an_indent_block` /
`kernel_ranges_tuck_the_closing_line_of_a_structural_block`（5 条），
新名 `kernel_end_line_tucks_in_the_last_line_of_every_block_kind` /
`the_widest_block_wins_when_two_blocks_start_on_one_line` /
`kernel_ranges_also_tuck_the_last_content_line_of_an_indent_block` /
`kernel_ranges_tuck_the_closing_line_of_a_block`（4 条）。

**并入**：`git merge --ff-only` 到 main，HEAD = 分支 HEAD = `7c12d7e8`（无 merge commit）。

**冒烟**：`cargo build -p rds-app -j 2` 3m48s 成功；`target\debug\rds-app.exe` 跑 20 秒，
日志 49 行、**panic 0 / ERROR 0 / WARN 0**，启动链路（旧数据迁移 → 全局系统库 → 日志系统 →
插件管理器 → `Themes reloaded successfully` → 项目 SQLite 迁移 → 导航树恢复 `G_real_mysql`）
全部走完。

**两件交接项**（都不是升级本身的问题）：

1. `rds-ui-guard::no_new_hardcoded_color_or_pixel` 仍红（`result_panel` 的 `grid/fit.rs` 裸 px 6、
   `ui.rs` `px(4) -> 12`），来自 main 的 `44040e21`「列宽自适应」，本分支没碰 `crates/result_panel/`。
2. main 工作区（`D:\RdataStation\RDS\RdataStation-v2`）的 `target/` 是按 0.8.1 建的，ff 之后
   第一次构建要全量重编（Rust 侧依赖树换了一大批），不是回归。

## 12. 并入证明与主分支验证（2026-11-08）

这一节回答两个问题：「分支的变动真的都进 main 了吗」「进 main 之后能用吗」。
两个问题都不接受"看起来一样"，所以下面全是可重跑的判据与观测值。

### 12.1 提交级包含关系（最硬的一条）

```
$ git merge-base --is-ancestor chore/deps-patch-batch-2026-11 main ; echo $?
0                                        # 分支是 main 的祖先
$ git rev-list --count main..chore/deps-patch-batch-2026-11
0                                        # 分支里没有 main 之外的提交
$ git rev-list --count chore/deps-patch-batch-2026-11..main
1                                        # main 只多一个提交
$ git diff --stat chore/deps-patch-batch-2026-11 main
 docs/architecture/ui/gpui-kit-0.9.0-upgrade-report.md | 4 ++--
```

唯一的差异是并入之后补的文档订正（`07f9b939`）。**这不是内容相似，是提交级包含**：
分支的每个提交都能在 main 的历史里点到。

### 12.2 并入的提交清单（19 条）

`git log --reverse --oneline 5675df1e..95997ebb`：

| # | 提交 | 内容 |
| --- | --- | --- |
| 1 | `e128bb38` | gpui-kit 0.9.0 + gpui-fast |
| 2 | `df3b06d9` | 记录 gpui-kit 迁移 |
| 3 | `d3f67006` | duckdb `=1.10506.1`，删 reqwest 死声明 |
| 4 | `fc64334f` | 记录 DuckDB 1.5.6 |
| 5 | `22618516` | sqlglot-rust 0.11.1（补 CTAS 两字段） |
| 6 | `ae1b2b40` | 记录 sqlglot 升级 |
| 7 | `222418f6` | 12 个补丁版本批次 |
| 8 | `74939175` | 记录补丁批次 + 释放度盘点 |
| 9 | `ee98915b` | 记录 gpui-fast 回归（6 条待定性） |
| 10 | `99a5c4e4` | sqlglot 能力释放 + 设置开关 |
| 11 | `e7185e6e` | 记录能力释放与设置域 |
| 12 | `6f101b9a` | 定性 6 条失败，结论适合开启 |
| 13 | `0c9080fe` | hover 类型行接通 + 转译提示 |
| 14 | `d1e9261a` | 接线 3→6、压后项侧记 |
| 15 | `6966555a` | 列级血缘设计 |
| 16 | `dce9433d` | **书签列/折叠说明 patch 移植到 0.9.0** |
| 17 | `7c12d7e8` | 同步 rebase 后现状 |
| 18 | `5dd88ef5` | 记录二次 rebase 与并入 |
| 19 | `95997ebb` | 补跑 rds-mock，修正账目 |

并入之后 main 侧另有 `07f9b939`（移植脚本归档 + 诊断产物标注）。

### 12.3 分支独占文件：rebase 前 vs 并入后逐字节比对

比对对象是 **rebase 前的分支 tip `a82123c0`**（不是 main，免得把 main 自己的改动算成差异）。
29 个分支独占文件里，**25 个与 rebase 前逐字节一致**，5 个有差异且每一处都能归因：

| 文件 | 差异 | 归因 |
| --- | --- | --- |
| `vendor/.../input/base/element.rs` | +448 / −10 | 书签列 + 折叠说明 patch 移植（rebase 曾把这段丢干净过） |
| `vendor/.../input/base/state.rs` | +91 | 同上（`GutterMarker` 状态与点击回报） |
| `crates/editor/src/view/host/session.rs` | +57 | rebase 前分支基座更旧，这些是 main 在中间 26 个提交里的改动 |
| `docs/architecture/dependencies/dependency-strategy.md` | +36 | 并入后补的 §4.1 |
| `docs/architecture/editor/editor-prototype-design.md` | 2± | 并入后按实测回填 |

一致的 25 个里包括 `Cargo.toml` / `Cargo.lock` / `.cargo/config.toml` / `tools/fetch-duckdb.sh` /
`crates/sql/src/builder.rs` / `analysis.rs` 与 3 个 `settings/sql_analysis.rs`——即**四个包的落地配置与
新增代码面 rebase 一次没丢**。

### 12.4 四个核心包的落地事实（直接读 main HEAD 的文件）

| 位置 | 观测值 |
| --- | --- |
| 根 `Cargo.toml` | `gpui-kit = { version = "0.9.0", features = ["gpui-fast"] }` |
| | `duckdb = { version = "=1.10506.1", features = ["appender-arrow"] }` |
| | `arrow = "58.4.0"`、`sqlglot-rust = "=0.11.1"` |
| | `[patch.crates-io]` → `vendor/gpui-base`、`vendor/gpui-component` |
| `Cargo.lock` | gpui-kit / gpui-base / gpui-component 均 `0.9.0`；gpui-fast `0.1.13`；gpui-pre `0.3.11`；duckdb `1.10506.1`；sqlglot-rust `0.11.1` |
| `.cargo/config.toml` | `DUCKDB_LIB_DIR = { value = "third_party/duckdb/1.5.6", relative = true }` |
| `vendor/gpui-base/Cargo.toml` | `version = "0.9.0"`；`vendor/gpui-component/Cargo.toml` 同 |
| `third_party/duckdb/1.5.6/` | dll 36,697,896 / h 298,229 / hpp 2,177,251 / lib 1,998,226 / extension.h 135,845 |
| dll SHA256 | `7E90BDEF028D57B45490D6F53530E3C012A5D9ABDC4C6723449200663DC84ED0` |
| 书签 patch 符号（`element.rs`） | `GutterMarkerKind` 12、`layout_gutter_markers` 2、`paint_gutter_markers` 2、`GUTTER_BOOKMARK_NOTE_SVG` 3、`report_gutter_marker_click` 1、`fold_note_rows` 6 |
| 书签 patch 符号（`state.rs`） | `GutterMarker` 12、`set_gutter_markers` 3、`gutter_markers` 9 |
| sqlglot CTAS | `builder.rs` 中 `with_data` / `with_properties` 各 2 处（两个 `CreateTableStatement` 构造） |
| sqlglot 设置域 | `settings/src/{model,registry/entries,service}/sql_analysis.rs` 三件套齐 |

### 12.5 12 个补丁批次：12/12 命中

`Cargo.lock` 里每个目标版本都在（同一包名的其它版本是传递依赖，不是漏升）：
tokio 1.54.1、tokio-util 0.7.20、serde_json 1.0.154、thiserror 2.0.21（另有 1.0.69 供旧依赖）、
uuid 1.29.1、encoding_rs 0.8.44、flate2 1.1.11、bigdecimal 0.4.11、jupyter-protocol 2.0.3、
rand 0.10.3（另有 0.8.8 / 0.9.5）、similar 3.2.1（另有 2.7.0）、winresource 0.1.34。
根 manifest 的声明与 lock 一致。

### 12.6 主分支冷编 + 冒烟（`target/` 被清空过，等于从零）

```
cargo build -p rds-app -j 2
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12m 56s
    error 0 行、warning 0 行
```

- `target\debug\duckdb.dll` = **36,697,896 B**（与 1.5.6 那份同尺寸，build script 自动拷到 exe 同级），
  运行时查找路径成立；`target\debug\rds-app.exe` 存在。
- `rds-app.exe` 跑 **20 秒**：**49 行日志、panic 0 / ERROR 0 / WARN 0**，启动链路走完
  （旧数据迁移 19 项 → 数据根 → 全局系统库 → 日志系统 → 导航动作 → 左栏标签 → `gpui::window` 起窗）。

### 12.7 全量测试：142 目标 / 3794 passed / 10 failed / 49 ignored

```
cargo test --workspace -j 1 --no-fail-fast
```

**10 条失败 = 9 条既有基线 + 1 条工作树未提交改动引起**：

| # | 用例 | 归属 |
| --- | --- | --- |
| 1–2 | `legacy.rs:58` 单元源不是内核编辑器、`notebook_win.rs:347` 输出块未渲染 | 升级前基线既有 |
| 3–4 | `version_dialog_lists_rows_and_shows_actions_after_selection`、`index_repair_dialog_groups_rows_and_shows_inline_actions` | gpui-fast 观察层（`debug_bounds` 读上一帧） |
| 5 | `clicking_the_title_row_collapses_and_restores` | 同上 |
| 6 | `nav_tree_rows_are_laid_out_by_the_virtual_list` | 同上 |
| 7 | `right_sidebar_alone_renders_draft_row`（`missing ElementId`） | 保留绘制模式下未重建子树 |
| 8 | `an_open_request_lands_in_a_sql_editor_tab` | gpui-fast 观察层 |
| 9 | `no_new_hardcoded_color_or_pixel` | main `44040e21` 的 `result_panel` 裸 px，分支没碰该目录 |
| 10 | `support::scan::tests::the_scan_uses_the_fold_keywords_it_was_given` | **工作树里 `crates/editor/src/support/scan.rs` 的未提交改动**（该用例在并入前是绿的） |

### 12.8 定向验证（比总数更能定位责任）

| 范围 | 结果 | 说明 |
| --- | --- | --- |
| `rds-engine --lib` | **219 / 0** | 真开 DuckDB（内存库、临时表、分析、联邦会话），1.5.6 运行可用 |
| `rds-sql --lib` | **206 / 0** | sqlglot-rust 0.11.1 解析 + `analysis` 11 条 |
| `rds-settings --lib` | **34 / 0** | `sql_analysis` 设置域 |
| `rds-mock --lib` | **243 / 0** | 补丁批次后的 mock 引擎 |
| `rds-editor --lib` | 757 / 3 / 1 ignored | 3 条中 2 条是基线，1 条是工作树改动 |
| 书签行为 | `bookmarks_project_into_gutter_markers` / `gutter_click_toggles_bookmark_and_shift_opens_note` / `bookmark_hover_note_setting_off_strips_note` 全 **ok**，另有 `folding_candidates_are_fed_after_the_gutter_restored` ok | 移植后的点击切换 + 悬停说明真的跑通 |

### 12.9 这次验证的边界（写清楚，不假装覆盖）

- 本次全量跑在工作树上，包含 6 个未提交改动（`scan.rs`、`view/host/exec/scan.rs`、
  `vendor/gpui-base/src/{test_support,text/node}.rs` 等）——第 10 条失败由此产生。
- **真机目视 5 项仍未做**：对话框关闭无残影、右 Dock 草稿行、导航虚拟列表行、列类型警示行、
  标题栏折叠还原；书签列的视觉位置（是否在行号左侧、折叠说明下划线位置、悬停气泡）也没肉眼确认。
- 全文只有一条"已知且未修"的红：`rds-ui-guard` 的裸 px，归属 main 侧 `result_panel`。

### 12.10 复现命令

```powershell
# 包含关系
git merge-base --is-ancestor chore/deps-patch-batch-2026-11 main; $LASTEXITCODE
git rev-list --count main..chore/deps-patch-batch-2026-11
git diff --stat chore/deps-patch-batch-2026-11 main
# 逐字节比对分支独占文件
git diff --quiet a82123c0 95997ebb -- Cargo.toml Cargo.lock .cargo/config.toml
# 冷编 + 冒烟
cargo build -p rds-app -j 2
# 全量（串行；-j 2 会触发 rustc 栈溢出）
cargo test --workspace -j 1 --no-fail-fast
```




