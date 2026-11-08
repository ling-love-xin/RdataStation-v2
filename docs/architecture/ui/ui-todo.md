# UI 待办清单（RdataStation-v2）

> 跟踪 UI 现代化迭代的未决 / 待拍板 / 挂起项。**已交付并验证的条目不在本表**（见文末「最近交付」），
> 只记「还没做完、还没定、还没验」的。图例：⬜ 未开始 · 🔵 进行中 · ⏳ 等拍板 · 🧊 挂起 · ⚠️ 需核实。

## 一、待拍板（需要产品 / 用户决策后才动代码）

| 状态 | 编号 | 事项 | 现状与选项 |
|---|---|---|---|
| ✅ | T-1 | TabBar 设置页配置 | **已交付**（见「最近交付」T-1 条）。最小集已落：`appearance.tab_bar_max_width`（120–600px，预设 120/160/200/240/320，默认 200）+ `appearance.tab_bar_variant`（五态 tab/outline/pill/segmented/underline，默认 tab），设置页「外观」分组即时生效并持久化；workbench 经 `SettingsHost::on_settings_changed` 装配到 DockSkin（渲染读取在 vendor `dock/tab_panel.rs`）。省略规则 / 滚动切换等候选扩展验证后再评估。 |
| ✅ | T-2 | 搜索框风格范围确认 | **已确认关闭**（2026-10-20 用户同意）：无边框 + 匹配开关（Aa/.*）入框内只保留在**左右侧边栏**（数据导航栏、草稿箱）。依据：匹配开关语义只在内容/导航过滤场景成立（资源库/元数据/项目选择器搜的是名字，不需要正则大小写）；对话框/弹层输入框需要有边框建立输入边界；无边框是对 kit Input 的样式覆写，收敛范围小利于后续升级。 |

## 二、挂起 / 遗留（有明确背景，暂不动）

| 状态 | 编号 | 事项 | 背景 |
|---|---|---|---|
| 🧊 | T-3 | 模块内部组件换 gpui-kit 现成组件 | 早期「不手搓组件、用 kit 现成」的收敛项，面板壳层已 dock 化，内部细节组件（树、列表等）逐模块核对 kit 可替换项后逐步换，未排期。 |
| ⚠️ | T-4 | 工作区并行改动未提交（非本清单） | 当前工作区存在**与本 UI 清单无关的并行工作**未提交，提交时已隔离：`crates/workbench/src/components/mock_host.rs`（temp_table_row_count）、`crates/database/src/lib.rs`（`pub mod er`）、`assets/icons` 重构（删旧图标 + dark/light 新目录）、`Cargo.lock`、`README.md`、engine/insight/mock 等 crate 的 M。**确认归属后再由对应工作流提交**。 |

## 三、最近交付（2026-10-20，已编译 + 测试）

**T-1（标签栏设置页配置）**：`appearance.tab_bar_max_width` / `appearance.tab_bar_variant` 两设置项进「外观」分组，即时生效 + 持久化。
- 链路：settings registry 两 Slot（Number 预设 120–600px / Enum 五态）→ model 默认 200.0 + "tab" → lib.rs 读写分支 + 校验 + persist → settings_page `SettingsHost::on_settings_changed`（commit 落盘后触发）→ workbench `Self::apply_tab_bar_settings` 装配 DockSkin → vendor `dock/tab_panel.rs` TabBar 渲染读取（`.with_variant` + `.max_width`）。
- vendor 扩展：`SkinShared` 加 `tab_bar_max_width: Cell<f32>` / `tab_bar_variant: Cell<TabVariant>` + 两个公开 setter（带 notify）。
- 验证：`rds-settings` 29 测试全过（28 并行 + `invalid_locale_is_rejected` 单跑；该用例并行隔离缺陷为已知基线）、`rds-workbench` 编译通过。

9 条修复（前一轮）全部完成：①明暗切换不再改字号（set_theme_mode 末尾重放 set_font_size）；②导航栏搜索行加 `group_box` 背景块与列表分层；③连接名称去「显示区域前缀」+ 类型简称 `[简称]`（默认同徽标缩写）；④归属域颜色跟随连接状态（Project=info / Global=cyan / Shared=primary，仅连接成功显色）；⑤草稿箱双搜索框（初始「搜索文件名或文件夹名」/ 搜索态「搜索文件内容」，独立输入与 Enter 动作）；⑥设置 →「类型简称映射」弹窗（保存 / 恢复默认 / 关闭，不展示驱动概念）；⑦搜索框风格范围=仅左右侧边栏（现状满足，待 T-2 确认）；⑧筛选图标换 `sliders-horizontal.svg`（`filter.svg` 不在 kit 资产集）；⑨TabBar 建议见 T-1。

验证：`rds-settings` 29 测试全过（含字符串表豁免规则）、`rds-database` 152 全过、`rds-app` 编译通过并重启（Responding=True）。

## 四、验收路径

1. 切换明暗主题：字号不变、搜索行背景显形（明亮主题下与列表边界清晰）
2. 数据导航栏：连接名 `[MY] 192.168.3.138` 形态、归属域胶囊连接后着色、筛选按钮有图标、搜索开关在框内
3. 设置页 → 数据源导航 →「类型简称映射…」：改简称后连接名 / 徽标同步
4. 草稿箱：初始框提示「搜索文件名或文件夹名」，打开搜索面板后查询框提示「搜索文件内容」
5. **T-1**：设置 → 外观 →「标签栏最大宽度」选 120px → 标签立即变窄、文字省略、✕ 完整；「标签栏样式」切 下划线/胶囊 → 标签条形态即变

## 五、待人工目视验收（gpui-kit 0.9.0 + gpui-fast 并入后，2026-11-08）

> 背景：`chore/deps-patch-batch-2026-11` 已并入本地 `main`（见
> `docs/architecture/ui/gpui-kit-0.9.0-upgrade-report.md` §12）。自动化侧 **142 目标 / 3794 passed /
> 10 failed**，其中 9 条是基线、1 条来自工作树未提交改动；剩下这 8 项只有**肉眼**能判，
> 因为它们要判的是"画面有没有"，而 gpui-fast 的保留绘制模式恰恰会让 `cx.debug_bounds(..)` 这类
> 断言读不到真实画面（详见该报告 §7 / §9）。
>
> 跑法：`target\debug\rds-app.exe` 起应用，**重点走编辑器模块**（书签 / 折叠）与四个对话框位。
> 全部通过 → 本节整表删除（同时删掉报告 §12.9 里对应的"未做"说明），不留悬空条目；
> 任一项不过 → 把现象记到表里的「结果」列，再按归属分派（gpui-fast 观察层 / 内核移植 / 业务侧）。

| 状态 | 编号 | 检查项 | 通过判据 | 关联证据 |
|---|---|---|---|---|
| ⬜ | V-1 | 分析资源对话框关闭后无残影 | 打开版本对话框 / 索引修复对话框后点关闭：背景编辑器与导航栏立即重绘干净，无半透明残留、无旧图层压字（`rds-analytics-resource --test dialog_window` 对应两条用例红在 `debug_bounds` 观察层） | `version_dialog_lists_rows_and_shows_actions_after_selection`、`index_repair_dialog_groups_rows_and_shows_inline_actions` |
| ⬜ | V-2 | 右 Dock 单独存在时草稿行出现 | 只留右 Dock（关掉中间编辑器/结果区）：草稿箱出现 `mock-draft-open` 那一行且可点开 | `rds-workbench --test mock_panel_probe` 的 `right_sidebar_alone_renders_draft_row` |
| ⬜ | V-3 | 导航树虚拟列表行真的画出 | 数据导航树滚动/展开：对象行逐行可见，不出现"有高度但空白"的条带 | `nav_view::tests::nav_tree_rows_are_laid_out_by_the_virtual_list` |
| ⬜ | V-4 | 列类型不匹配警示行可见 | 打开列编辑对话框并造出类型冲突：警示行出现且文案可读（不是高度占了但没字） | `rds-analytics-resource` 的 `column_dialog_shows_type_mismatch_warning` |
| ⬜ | V-5 | 标题栏折叠后正文可还原 | 连接对话框点标题行折叠 → 正文收起；再点一次 → 正文完整回来 | `header_interaction::clicking_the_title_row_collapses_and_restores` |
| ⬜ | V-6 | 书签图标在行号栏最左侧 | 有书签的行：书签图标贴在 gutter 最左，行号整体右移一列；无书签的编辑器行号栏宽度不变、不吞行号点击 | `element.rs` 的 `GutterMarkerLayout.column_width`、`marker_column_x = origin_x` |
| ⬜ | V-7 | 有说明的折叠行：箭头常显 + 下方一道横线 | 带说明的折叠行箭头**常显**（不受 hover 影响），箭头下方 9×1.5 的横线可见；无说明的可折叠行仍只在 hover / 当前行 / 已折叠时显箭头 | `GUTTER_FOLD_NOTE_UNDERLINE_WIDTH 9` / `HEIGHT 1.5`、`FoldIconLayout.fold_note_rows` |
| ⬜ | V-8 | 书签悬停说明气泡 | 指针停在有说明的书签行 → 该行下方、书签列右侧浮出淡色气泡（长文本截断带 `…`）；设置里关掉 `editor.bookmark_hover_note` 后气泡不再出现 | `bookmark_hover_note_setting_off_strips_note`（自动化已绿，视觉未验） |

**已由自动化覆盖、不需要肉眼重复的部分**：书签投影 / 单击切换 / Shift 弹说明三条用例已绿
（`bookmarks_project_into_gutter_markers`、`gutter_click_toggles_bookmark_and_shift_opens_note`、
`bookmark_hover_note_setting_off_strips_note`），折叠候选回灌也已绿
（`folding_candidates_are_fed_after_the_gutter_restored`）。
