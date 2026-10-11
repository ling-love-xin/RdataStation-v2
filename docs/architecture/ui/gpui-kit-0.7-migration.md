# GPUI-kit 0.6.1 → 0.7.0 迁移记录（M3 全程实测）

> 状态：**0.6.1 → 0.7.0 已合入**（`Cargo.toml` / `Cargo.lock` = `gpui-kit 0.7.0`，2026-10-01 verify/gpui-kit-0.7 分支验证后合回 main）；**0.7.0 → 0.7.1 已在分支 `refactor/ui-standalone` 完成文件级 + 编译级升级**（2026-10-11，差异见文末 §8，全仓测试待合 main 前 CI 门禁）。
> 本文记录两版差异的**源码实测结论**（2026-10-02 对 crates.io 锁定解包源码逐个 diff），供迁移、多轨开发与 API 查证使用。
> 权威源（0.6.1 / 0.7.0）：`D:/Dev/Rust/.cargo/registry/src/mirrors.tuna.tsinghua.edu.cn-4dc01642fd091eda/{gpui-kit,gpui-base,gpui-component,gpui-kit-assets,gpui-component-macros}-0.7.0/`；0.7.1 起本项目 vendor 化，权威源 = `vendor/gpui-component/`（0.7.1 源码 + tab_panel patch）。
> 同步文件：`docs/architecture/ui/ui-design-spec.md`、`.agents/skills/{gpui-kit-dev,rds-layout,rds-theme}.md`。

## 1. 结论一句话

**0.6.1 → 0.7.0 是源码兼容的增量升级**：组件模块只增不减、关键组件 pub fn 几乎逐字不变；唯一需要警惕的是**行为回归实例**（卡片标题双重居中）与个别 API 移除（dock tiles 滚动条模式）。

## 2. 组件模块清单（gpui-component lib.rs pub mod）

- 0.7.0 共 **65 个模块**；0.6.1 → 0.7.0 **+4 无删除**：
  - `carousel`（数据展示：轮播）
  - `empty`（空态组件）
  - `questionnaire`（表单问卷）
  - `toolbar`（按钮组）
- 勘误（2026-10-11 复核修正）：早期清单称 `time`（calendar / date_picker）"在两版 lib.rs 均不存在"——**不准确**：0.7.0 与 0.7.1 的 `lib.rs` 均无 `pub mod time`，但都有 `pub use time::{calendar, date_picker, time_field}`（隐藏 mod re-export，2026-10-11 对锁定源码实测）；日期选择器组件实际可用（`gpui_kit::component::calendar` / `date_picker` / `time_field`）。

## 3. 关键组件 pub fn 差异（逐个 diff 实测）

| 组件 | 0.7.0 变化 | 影响 |
| --- | --- | --- |
| `switch::Switch` | +`tab_index`、+`tab_stop` | 新增（键盘可达补齐），无破坏 |
| `dock` | +`set_close_button_visible`；**−`set_tiles_scrollbar_mode`、−`tiles_scrollbar_mode`** | tiles 滚动条模式 API 移除，改用标准 scroll 装配（项目内无调用点，无影响） |
| `theme` | +`update` | 新增，无破坏 |
| `popover` | +`arrow`、+`offset` | 新增（箭头与偏移定制），无破坏 |
| `button` / `input` / `tab` / `list` / `select` / `combobox` / `checkbox` / `radio` / `text` / `menu` | **pub fn 无变化** | 项目内全部引用点无需改 |
| `h_flex` | 定义两版逐字一致（`flex().flex_row().items_center()`，styled.rs） | 无变化 |

## 4. 主题体系（gpui-base theme_tokens，逐字段统计）

| Token 结构 | 0.6.1 | 0.7.0 |
| --- | --- | --- |
| `ColorTokens` | 18 项 | **18 项（不变）** |
| `SemanticThemeTokens` | 23 项 | **23 项（不变）** |
| `RadiusTokens` / `ShadowTokens` / `SpacingTokens` / `TypographyTokens` / `TextStyleToken` | 27 / 3 / 21 / 11 / 14 | 全部不变 |

结论：`rds-theme` 的「语义面固定 18 角色（ColorTokens）」断言在 0.7 **依然成立**；产品语义 token（`workbench_shell::product_tokens`）路线不受影响。

## 5. 行为回归实例（必须注意）

| 实例 | 说明 | 修复 |
| --- | --- | --- |
| 连接对话框卡片标题「双重居中」 | 0.7 下标题在容器内**双向居中**（0.6.1 左对齐）；**非** `h_flex` 定义变化（两版逐字一致） | `connection_dialog/helpers.rs::section_header` 加 `w_full`（标题占满行宽后左对齐）；已实测通过 |
| 排查方向 | 0.7 新增代码遇到「莫名居中 / 莫名被裁」：先查 flex 祖先的 `justify_center` / `items_center` 组合 | — |

## 6. 菜单断言（0.6.1 / 0.7.0 复核，2026-10-02；0.7.1 复测，2026-10-11）

- **0.6.1**：`PopupMenuItem` / `DropdownButton` **无 shortcut 槽位**（菜单项显示不了快捷键；快捷键只能写在 `Kbd` 元素或文案里）。
- **0.7.0 起**：`PopupMenuItem::Item` 带 `action` 字段，`PopupMenu::render_key_binding`（实测 0.7.1 `popup_menu.rs:1119`）经 `Kbd::binding_for_action_in` / `global_binding_for_action`（:1143-1144）**自动渲染快捷键**——挂 action + 注册 key binding 即可显示（`project-dev-plan` C2 已解除阻塞）。
- `PopupMenu` **不暴露菜单项**（只有 `is_empty()`）；`ready_for_use()` 之类探针不存在。
- 测试写法不变：「菜单长什么样」抽成纯函数，测试断言纯函数（参考 `crates/project/src/ui.rs::project_menu_entries`）。

## 7. 迁移指引（多轨统一到 0.7）

1. 新代码**直接按 0.7.0 写**，无需兼容 0.6.1 写法（源码兼容已验证）。
2. 遇到编译错：`grep -rn "pub fn" <0.7.0 源码目录>/src/` 查证，**不凭 0.6.1 记忆猜 API**。
3. 本地 `D:\ABDM\Compressed\gpui-kit-main\gpui-kit-main` 是 main 分支快照（**不是 0.7.0**）：只用于读 `skills/` / `docs/` 规范内容；**禁止**拿它的函数签名 / 组件清单当 0.7.0 依据。
4. 本项目已按 0.7 完成验证的存量：`connection_dialog` 五 Tab / 列表 / 标题左对齐。

## 8. 0.7.0 → 0.7.1 升级记录（2026-10-11 对锁定源码 + vendor 实测）

**结论：0.7.1 是纯增量 patch，符号零删除，风险低**（crates.io 0.7.1 为 2026-10-05 发布）。

| 类别 | 0.7.0 → 0.7.1 |
| --- | --- |
| 组件模块（`lib.rs` pub mod） | **+1 无删除**：0.7.0 = 67 → 0.7.1 = 68，新增 `speech`；`pub use time::{calendar, date_picker, time_field}` 两版均有 |
| `ThemeColor` 字段集 | **141 → 141 无变化**（`theme_color.rs` 逐字段对比） |
| 关键组件 | `button.rs` **focus 样式重构**（新增 `FocusLine` / `focus_style`，filled/ghost/no_padding 三种焦点线；`focus_ring_style` 改为 `focus_style` 调用）；`popup_menu.rs` +4 行（shortcut 渲染保持）；`dock/tab_panel.rs` 无变更 |
| 依赖 | `gpui-pre` `=0.3.7` → `=0.3.8`（26 包解析）；`gpui-component` 0.7.1；`lsp-types` ^0.97 不变；仍**不含** `gpui-wry` |
| 门面符号 | gpui-kit 门面 20 → 21（+1 无删除）；gpui-base 1572 → 1584（+12 无删除） |
| vendor 化 | 0.7.1 起本项目 vendor `gpui-component`（`vendor/gpui-component/`，含 `tab_panel.rs` 的 `title_suffix` 补丁重放，hash 校验一致）；0.7.0 的 vendor patch 升级后被静默弃用（`patch gpui-component v0.7.0 was not used`），0.7.1 下已重新生效 |
| 验证 | `cargo check -p rds-app` 0 error（2026-10-11）；聚焦 10 个 UI crate 测试进行中；全仓 ~1328 测试留合 main 前 CI 门禁 |

迁移指引更新：新代码直接按 0.7.1 写（0.7.0 兼容写法同样有效）；查 API 优先看 `vendor/gpui-component/src/`（权威），其次 `D:/Dev/Rust/.cargo/registry/src/.../gpui-kit-0.7.1/` 等解包源码。

---
*记录：2026-10-02 · Doubao 办公模式；全部结论来自锁定源码逐项 diff，非估计。（§8 追加于 2026-10-11）*
