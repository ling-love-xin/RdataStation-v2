# 设置面板迁移：自研两栏 → gpui-kit `setting` 组件

> 状态：已完成并提交（`main 027d9a9b`）。提交范围 `crates/settings/`（6 文件，+635/-897）。

## 1 迁移目标

把 `crates/settings/src/settings_page.rs` 的**自研两栏布局**（List 导航 + 行卡片 + 顶部搜索 + 命中高亮 + 分段控件）整体替换为 gpui-kit 0.8.1 的 `setting` 组件族
（`Settings` / `SettingPage` / `SettingGroup` / `SettingItem` / `SettingField`）。

- 旧实现：`settings_page.rs` **1032 行**，导航与分组控件全部手搓。
- 新实现：`settings_page.rs` 压缩为 **~180 行**（chrome 壳层），构造逻辑下沉到 3 个新模块。

## 2 新架构

```
settings_page.rs   SettingsHost(6 回调) / SettingsPage / 顶栏 / 底栏 / 落盘失败横幅 / skeleton::build
      │
      ├── bridge.rs   值桥：current_value / is_changed(对照 spec.default_value) /
      │                write_scalar(走唯一写入路径 SettingsService::apply_by_key) / reset_to_default
      ├── fields.rs   字段适配：value_item(Switch / TabBar::segmented 分段) + 恢复默认按钮 +
      │                action_row(动作行) + layout_preview(布局占比示意) + tab_bar_preview(标签栏样式预览)
      └── skeleton.rs 骨架构造：registry 12 节 → SettingGroup → SettingPage → Settings 树
```

- **数据写入**仍走 `SettingsService::apply_by_key`（唯一写入路径），字段只在 render 期经 `value_by_key` 读当前值。
- **副作用**仍经 `SettingsHost` 的 6 个回调（on_close / on_open_cache / on_open_log_dir / on_open_log_view /
  on_open_type_short_map / on_settings_changed）交给宿主，签名未变，workbench 集成点零改动。
- **落盘失败横幅**改为纯读全局 `last_save_error()`（不再维护页内快照字段）。

## 3 gpui-kit 组件复用（关键决策）

- **分段控件**：复用 `tab::TabBar::segmented()`（单选 `selected_index` + `on_click(&usize)`），经
  `SettingField::render` 嵌入，**不手搓**。曾误判"库没有对应物"，源码核实后修正。
- **开关**：`switch::Switch`（`.disabled` 走 `Disableable`）。
- **恢复默认按钮**：`SettingItem` 值行挂 `.on_reset`（`Element` 用 `SettingItem::on_reset`，`Item` 字段用
  `SettingField::default_value`/`on_reset`），旁挂 `RotateCw` 图标 Button。
- **分组容器**：`GroupBoxVariant::Normal`（从 `group_box` 导入）。
- **布局预览 / 标签栏预览**：RdataStation vendor patch，render 期读当前值实时示意。
- 尺寸全走 `workbench_shell::ui` 常量与 `rems()`，颜色全走 `cx.theme().colors.*`——满足
  `ui_contract.rs` 对 `settings_page.rs` 的裸 `px` / 裸色扫描。

## 4 编译踩坑记录（后续别重蹈）

- `h_flex()` 不是根自由函数 → `div().h_flex()`。
- `RenderOptions` 从 `component::setting`、`GroupBoxVariant` 从 `component::group_box` 导入。
- `SettingValue::as_bool/as_text/as_number` 是 `&self` → `.and_then(|v| ...)` 拿到引用后要 `.map(owned)`，避免借用期问题。
- `SettingField::render` 闭包捕获的 `Rc` 必须在内部 clone；`on_click` 同理（Fn 闭包不能 move out）。
- 补 trait 导入 `Disableable as _` / `Selectable as _` / `ActiveTheme`。
- `Size::Small` 需显式 `use gpui_kit::component::Size`（否则被 gpui 的 `Size<T>` glob 遮蔽）。
- 测试模块**禁用 `use super::*`**：会把 `gpui_kit::*` 的 `test` 宏带进作用域，与 `#[gpui_kit::test]` 递归冲突。

## 5 验证

| 项 | 结果 |
|---|---|
| `cargo check -p rds-settings` | 通过 |
| `cargo test -p rds-settings`（33 个：bridge 纯函数 + 窗口冒烟 + registry 契约 + lib.rs） | 全绿 |
| `cargo check -p rds-workbench -p rds-app` | 通过（宿主集成类型兼容） |
| ui_contract 对 settings_page.rs 的 px / 色检查 | 合规（精确复刻逻辑验证 False） |

## 6 遗留 / 未在本提交范围

- 完整 `cargo test -p rds-workbench` 仍有 4 处失败，全部位于**并行重构**的模块，与本次迁移无关、未处理：
  - `connection_render_matrix.rs:471`（`outline_section` 展开后高度，headless spring 时序 flaky）
  - `ui_contract.rs:118`（`workbench_shell/src/ui.rs` 右侧边栏起步宽 18rem≠320px）
  - `ui_contract.rs:649`（workbench `Shared` 字段集新增未白名单）
  - `ui_contract.rs:311`（`panels/left_tabs.rs` 有裸 `px(...)`）
- 旧 `FocusSettingsSearch`（Ctrl+F，key context "settings"）命令仍注册，但搜索框已随迁移移除，无实际聚焦目标，保留即可。
