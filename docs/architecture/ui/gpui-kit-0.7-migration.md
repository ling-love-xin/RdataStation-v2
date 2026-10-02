# GPUI-kit 0.6.1 → 0.7.0 迁移记录（M3 全程实测）

> 状态：**已合入**——`Cargo.toml` / `Cargo.lock` = `gpui-kit 0.7.0`（2026-10-01 verify/gpui-kit-0.7 分支验证后合回 main）。
> 本文记录两版差异的**源码实测结论**（2026-10-02 对 crates.io 锁定解包源码逐个 diff），供迁移、多轨开发与 API 查证使用。
> 权威源：`D:/Dev/Rust/.cargo/registry/src/mirrors.tuna.tsinghua.edu.cn-4dc01642fd091eda/{gpui-kit,gpui-base,gpui-component,gpui-kit-assets,gpui-component-macros}-0.7.0/`。
> 同步文件：`docs/architecture/ui/ui-design-spec.md`、`.agents/skills/{gpui-kit-dev,rds-layout,rds-theme}.md`。

## 1. 结论一句话

**0.6.1 → 0.7.0 是源码兼容的增量升级**：组件模块只增不减、关键组件 pub fn 几乎逐字不变；唯一需要警惕的是**行为回归实例**（卡片标题双重居中）与个别 API 移除（dock tiles 滚动条模式）。

## 2. 组件模块清单（gpui-component lib.rs pub mod）

- 0.7.0 共 **65 个模块**；0.6.1 → 0.7.0 **+4 无删除**：
  - `carousel`（数据展示：轮播）
  - `empty`（空态组件）
  - `questionnaire`（表单问卷）
  - `toolbar`（按钮组）
- 勘误：早期清单中的 `time`（calendar / date_picker）**在两版 lib.rs 均不存在**（0.6.1 / 0.7.0 已核实），属旧记录错误；日期选择器若需要须先查证再定方案。

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

## 6. 菜单断言（0.6.1 / 0.7.0 一致，2026-10-02 复核）

- `PopupMenuItem` / `DropdownButton` **无 shortcut 槽位**（菜单项显示不了快捷键；快捷键只能写在 `Kbd` 元素或文案里）。
- `PopupMenu` **不暴露菜单项**（只有 `is_empty()`）；`ready_for_use()` 之类探针不存在。
- 测试写法不变：「菜单长什么样」抽成纯函数，测试断言纯函数（参考 `crates/project/src/ui.rs::project_menu_entries`）。

## 7. 迁移指引（多轨统一到 0.7）

1. 新代码**直接按 0.7.0 写**，无需兼容 0.6.1 写法（源码兼容已验证）。
2. 遇到编译错：`grep -rn "pub fn" <0.7.0 源码目录>/src/` 查证，**不凭 0.6.1 记忆猜 API**。
3. 本地 `D:\ABDM\Compressed\gpui-kit-main\gpui-kit-main` 是 main 分支快照（**不是 0.7.0**）：只用于读 `skills/` / `docs/` 规范内容；**禁止**拿它的函数签名 / 组件清单当 0.7.0 依据。
4. 本项目已按 0.7 完成验证的存量：`connection_dialog` 五 Tab / 列表 / 标题左对齐。

---
*记录：2026-10-02 · Doubao 办公模式；全部结论来自锁定源码逐项 diff，非估计。*
