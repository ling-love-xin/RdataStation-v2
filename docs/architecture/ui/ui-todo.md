# UI 待办清单（RdataStation-v2）

> 跟踪 UI 现代化迭代的未决 / 待拍板 / 挂起项。**已交付并验证的条目不在本表**（见文末「最近交付」），
> 只记「还没做完、还没定、还没验」的。图例：⬜ 未开始 · 🔵 进行中 · ⏳ 等拍板 · 🧊 挂起 · ⚠️ 需核实。

## 一、待拍板（需要产品 / 用户决策后才动代码）

| 状态 | 编号 | 事项 | 现状与选项 |
|---|---|---|---|
| ✅ | T-1 | TabBar 设置页配置 | **已交付**（见「最近交付」T-1 条）。最小集已落：`appearance.tab_bar_max_width`（120–600px，预设 120/160/200/240/320，默认 200）+ `appearance.tab_bar_variant`（五态 tab/outline/pill/segmented/underline，默认 tab），设置页「外观」分组即时生效并持久化；workbench 经 `SettingsHost::on_settings_changed` 装配到 DockSkin（渲染读取在 vendor `dock/tab_panel.rs`）。省略规则 / 滚动切换等候选扩展验证后再评估。 |
| ⏳ | T-2 | 搜索框风格范围确认 | 无边框 + 匹配开关（Aa/.*）入框内：只改了**左右侧边栏**（数据导航栏、草稿箱），其他模块保持原生形态（资源 / Mock 等未动）。**待确认：用户所指「其他模块」是否有具体不符合项**；无则本条关闭。 |

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
