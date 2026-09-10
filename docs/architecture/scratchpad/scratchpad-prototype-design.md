# 草稿箱模块 · 原型设计（项目工作区）

> 状态：**根语义已落地（P0 + Phase A）；面板首切片（只读树）已接入**（2026-09-11） · 关联文件：`scratchpad-prototype.html`（可交互原型）、`scratchpad-dev-plan.md`（开发方案与进度）
> 参考基准：v1 实现（`v1/backend/src/core/scratchpad`、`v1/frontend/extensions/builtin/scratchpad`）与设计（`v1/docs/backend/SCRATCHPAD_DESIGN.md`、`SCRATCHPAD_SCHEMA.md`、`v1/docs/frontend/SCRATCHPAD.md`）
> 布局服从 `docs/architecture/layout/layout-design.md`（五段布局，左侧 Dock 240px，`LeftPanel::Draft`）；配色服从 `docs/architecture/theme/theme-design.md`（RDS Light/Dark，`assets/themes/rds-theme.json`）
> 技术栈：GPUI（gpui-kit 0.6），组件消费 `cx.theme()` 语义 token，**代码零裸 hex**

## 1. 设计基准与 v2 语义变更

| 维度 | 基准 | 说明 |
| --- | --- | --- |
| 布局 | 五段布局（已定） | 草稿箱 = 左侧 Dock 内容，`LeftPanel::Draft`，起步宽度 240px（可拖拽调宽） |
| 后端 | `crates/scratchpad`（已迁移） | `models` / `state` / `store`，47 个方法（列表/CRUD/回收站/搜索/替换/Diff/外部引用/可分析文件） |
| 交互蓝本 | v1 `ScratchpadPanel.vue` + `TreeNode` | 工具栏 / 分层树 / 内联创建重命名 / 右键菜单 / 回收站 / 搜索上下文 / 多选 / 撤销栏 |
| 配色 | `theme-design.md` | 侧栏 `sidebar`、选中 `list.active` + coral 左边条、弹层 `popover`、主按钮 `primary` |
| 主题 | `rds-theme.json` | 复用标准字段，尽量不新增产品语义 token（见 §6） |

### 1.1 核心变更：草稿箱根 = 项目目录

v1 的草稿箱是项目下的隐藏子目录 `{project}/.scratchpad/`。v2 中**每一个应用实例即一个项目**，因此：

- **面板树的根节点 = 项目根目录 `{project}/`**，用户看到的即为项目文件工作区（SQL 片段、Python 脚本、测试数据、随手建的目录）。
- 草稿箱自身的**内部元数据**（外部引用列表、文件元数据、回收站）不再污染项目根，统一收纳到项目元数据目录 `.RSmeta/scratchpad/`。
- 顶层以 `.` 开头的条目（`.RSmeta`、`.git` 等）一律**不显示**，与现有 `store.rs::scan_dir_tree` 跳过点文件/点目录的逻辑一致。

```
{project}/                          ← 草稿箱根（面板显示，排除点目录）
├── .RSmeta/                        ← 隐藏：项目内部元数据（project crate 管理）
│   ├── project.db                  # 项目 SQLite
│   ├── project.json                # 项目配置
│   ├── analytics.duckdb            # 项目分析库
│   ├── config/  queries/  project_metadata/
│   └── scratchpad/                 # 草稿箱内部目录（本模块管理）
│       ├── config.json             # 外部引用 + file_meta（原 .scratchpad.json）
│       └── trash/                  # 软删除回收站（原 .trash/）
├── data/
│   └── report.sql
├── 临时订单分析.sql                 # 草稿文件（双击 → SQL 编辑器）
├── transform.py
└── sample.csv
```

> 设计取舍：保留面板名「草稿箱」（活动栏图标 / Quick Open `打开草稿箱` 命令已注册），语义由「隐藏草稿区」扩展为「项目工作区根」；「草稿」与「正式项目资产」在 v2 不再靠物理目录区分，而由 `analytics_resource` 的提升机制承担（§4.6）。

## 2. 面板布局（240px 左 Dock）

草稿箱面板嵌入左侧边栏，结构自上而下四段：面板头 / 工具栏 + 搜索 / 树主体 / 底部状态。宽度受限，工具栏压成单排小图标按钮。

```
┌ 左侧边栏 240px（sidebar 底）──────────┐
│ 草稿箱                          [⋯]  │  ← 面板头（Dock tab，36px）
├ 工具栏（icon 按钮，28px）─────────────┤
│ [＋][📁＋][导入][引用][⤓][↻]        │
├ 搜索框（可切换 文件名 / 内容）────────┤
│ [🔍 搜索文件…]              [.*][Aa] │
├ 树主体（滚动，虚拟列表 >50）─────────┤
│ ▼ 📁 项目文件 · 营销分析             │
│   ▸ 📁 data                          │
│   📜 临时订单分析.sql          ●     │  ← ● 脏点（未保存）
│   🐍 transform.py                    │
│   📊 sample.csv                      │
│   [新文件名…]                        │  ← 内联创建（选中文件夹后）
│ ▼ 🔗 外部引用 (1)                    │
│   ⚡ 下载数据  D:\data\              │
│ ▶ 🗑 回收站 (3)                      │
├ 底部状态（11px，muted）──────────────┤
│ 12 个文件 · 3 个文件夹 · 项目:营销分析 │
└──────────────────────────────────────┘
```

### 2.1 工具栏（单排图标，悬浮出文字提示）

| 按钮 | 行为 | 对应后端 |
| --- | --- | --- |
| `＋` 新建文件 | 选中文件夹下内联输入；可选模板（SQL/JSON/Markdown/Python） | `create_entry` |
| `📁＋` 新建文件夹 | 选中文件夹下内联输入目录名 | `create_entry(is_folder)` |
| `导入` | 系统文件对话框 → 复制进项目根（或选中目录） | `import_external_file` |
| `引用` | 添加外部目录/文件别名引用（不复制） | `add_external_reference` |
| `⤓` 排序 | 名称 / 大小 / 修改时间，点击切换升降序 | 前端计算（`list_directory_entries` 数据） |
| `↻` 刷新 | 重新拉取树（文件监控之外的兜底） | `list_local_entries` |

### 2.2 空态

项目根无任何用户文件时显示引导（大图标 + 标题 + 说明 + 「新建」「导入」双按钮），避免 240px 版面显得空洞。

## 3. 树与分组

- **项目文件**（根组）：`ScratchpadEntry` 递归树，文件夹最多 4 层（`MAX_DEPTH`）；初始 `depth=0` 懒加载，展开文件夹时按需 `list_directory_entries(parent)`。
- **外部引用**：`ExternalReference{ alias, path }` 平铺列表，标题带计数；校验路径合法且不含 `..`（v1 `isRefValid`），非法项置灰并给出提示。
- **回收站**：折叠区，标题带计数；展开后 `list_trash` → 每项「恢复」，头部「清空」。
- **行视觉**：
  - 选中：`sidebar.accent.background` 底 + 左侧 2px `list.active.border`（品牌 coral）条
  - 悬停：`list.hover.background`
  - 文件夹展开箭头 `Disclosure`（▸/▼）；文件图标按后缀映射（§6.3）
  - 修改时间：相对时间（<1 分钟 / N 分钟 / N 小时 / N 天，7 天内），置于行尾 muted
  - 脏点 `●`：`dirtyFiles` 集合中的文件，颜色 `primary`；`Ctrl+S` 保存后消失

## 4. 核心交互

### 4.1 新建 / 重命名（内联，非模态）

- 新建：选中文件夹 → 工具按钮 → 树中该目录下插入输入框，Enter 提交、Escape 取消；**根目录新建**默认创建在项目根。
- 模板：新建文件时可从 `SQL / JSON / Markdown / Python` 选模板，自动补后缀并填充占位内容。
- 重命名：右键「重命名」或 `F2` → 行内输入框；提交时输入禁用 + 旋转指示，防重复提交；空值不提交。

### 4.2 打开文件（联动中央编辑区）

```
双击文件
  └── 按后缀选择编辑器
        ├── .sql → 中央编辑区 SQL 编辑器（草稿箱文件模式）
        │     · 标题 = 文件名；Ctrl+S → 保存回项目文件（原子写）
        │     · 保留连接选择与完整执行引擎（单/多语句、选中执行）
        │     · 自动恢复 file_meta.last_connection_id
        │     · 关闭方言转换 / DuckDB 加速 / 执行计划（草稿场景不需要）
        ├── .py / .json / .md / .txt → 代码编辑器（语法高亮）
        └── 其他 → 系统默认程序打开
```

> 同一文件不重复开 Tab，已存在则聚焦。多文件 Tab 体系属中央编辑区独立工作项；Phase C 先打通「单文件 → 编辑器 → 回存」。

### 4.3 搜索（两种模式，重结果落中央区）

- **文件名模式**（默认）：输入实时过滤树（匹配名称 + 引用别名/路径）。
- **内容模式**：`.*` 切正则（前端 `Regex` 预校验）、`Aa` 切大小写敏感；调 `search_file_content` 流式扫描（大文件不跳过，`BufReader` 逐行，30s/文件超时，结果 500 条截断）。
- 结果含**匹配行 + 前后 2 行上下文**，高亮匹配文本；点击行号 → 打开文件并跳转到该行。
- 结果集较大，渲染在**中央编辑区**（专用「搜索」面板），侧栏只承载输入与摘要，避免 240px 拥挤。

### 4.4 文件操作

| 操作 | 行为 | 后端 |
| --- | --- | --- |
| 删除 | 软删除进 `.RSmeta/scratchpad/trash/`；底部弹出 5s 撤销栏 | `delete_entry` / `restore_from_trash` |
| 批量删除 | 多选（Ctrl 点选 / Shift 范围 / Ctrl+A 全选）→ 右键或 Delete | `delete_entry` × N |
| 剪切移动 | 右键「剪切」→ 粘贴到目标文件夹（`fs::rename`）→ 5s 撤销栏 | `move_entry` |
| 复制 | 右键「复制」→ 粘贴生成 `_copy` 副本 | `create_entry` + `read/save_file` |
| 导入 | 系统文件对话框复制进项目 | `import_external_file` |
| 外部引用 | 添加别名 + 路径引用（不复制）；可移除 | `add/remove_external_reference` |
| 打开所在位置 | 系统文件管理器定位 | `open_in_system_explorer` |

### 4.5 编辑态与冲突

- `Ctrl+S` 原子写回项目文件；文件监控（`notify`）发现外部修改 → 冲突对话框（重新加载 / 忽略），并置脏点。
- 拖拽文件节点到中央编辑区 → 插入文件内容到光标处。
- **Diff 对比**：冲突时「查看差异」→ `diff_with_content`（`similar` 行级）→ 弹窗红/绿标记 → 接受右侧。
- **搜索替换**：结果区替换栏 → 预览计数 → 全部替换（`replace_in_file`，支持正则）→ 原子写回 → 刷新结果。

### 4.6 提升为分析资源

右键「提升为分析资源」→ 复制/移动到 `analytics_resource` 正式区（可选保留原稿）→ 完成后发事件通知刷新。跨模块协作走 **command / event**，草稿箱不直接依赖分析资源模块的视图。

### 4.7 键盘导航

`↑↓` 切换选中、`Enter` 打开、`F2` 重命名、`Delete` 删除、`Ctrl+N` 新建、`Ctrl+A` 全选、`Escape` 取消内联输入/关闭菜单。

## 5. 关键帧与状态流

```mermaid
flowchart TD
    A[进入草稿箱面板] --> B{项目已打开?}
    B -- 否 --> C[空态: 提示打开/创建项目]
    B -- 是 --> D[列出项目根 depth=0]
    D --> E{根下有用户文件?}
    E -- 否 --> F[空态引导: 新建 / 导入]
    E -- 是 --> G[渲染树: 项目文件 / 外部引用 / 回收站]
    G --> H[展开文件夹 → 懒加载子目录]
    G --> I[双击文件 → 中央编辑区]
    I --> J{SQL 文件?}
    J -- 是 --> K[SQL 草稿模式: 连接+执行+Ctrl+S 回存]
    J -- 否 --> L[代码编辑器]
    K --> M[执行 → 更新 file_meta]
    K --> N[外部修改 → 冲突 → Diff]
    G --> O[搜索 → 结果落中央区]
    G --> P[删除 → 回收站 + 5s 撤销]
```

## 6. 主题映射（token → 视觉）

> 实现时色值只存在于 `assets/themes/rds-theme.json`，GPUI 组件经 `cx.theme()` 读取，**禁止写裸 hex**（`theme-design.md` §5）。

### 6.1 容器与导航

| 元素 | Token | RDS Light | RDS Dark |
| --- | --- | --- | --- |
| 面板底 | `sidebar.background` | `#F3F3F3` | `#252526` |
| 面板头 | `tab_bar.background` / `sidebar.background` | `#F3F3F3` | `#252526` |
| 分隔线 / 边框 | `sidebar.border` / `border` | `#E7E7E7` / `#D4D4D4` | `#3C3C3C` |
| 正文 / 弱文字 | `sidebar.foreground` / `muted.foreground` | `#616161` / `#8E8E8E` | `#CCCCCC` / `#8A8A8A` |
| 行悬停 | `list.hover.background` | `#F0F0F0` | `#2A2D2E` |
| 行选中底 | `sidebar.accent.background` | `#E4E4E4` | `#37373D` |
| 选中左边条 | `list.active.border`（品牌 coral） | `#C25B46` | `#E8846F` |
| 工具图标（常态/悬停） | `muted.foreground` / `foreground` | `#8E8E8E` / `#333333` | `#8A8A8A` / `#CCCCCC` |
| 搜索输入框 | `background` + `input.border`；聚焦 `caret`/`primary` | `#FFFFFF` / `#D4D4D4` | `#1E1E1E` / `#3C3C3C` |
| 脏点 `●` | `primary.background` | `#C25B46` | `#E8846F` |
| 右键菜单 | `popover.background` / `popover.foreground` + `border` | `#FFFFFF` / `#333333` | `#252526` / `#CCCCCC` |
| 撤销栏 | `popover.background` + `border`；按钮 `primary` | — | — |
| 危险项（清空回收站/删除） | `danger` | `#DC2626` | `#F14C4C` |
| 成功（导入/提升） | `success` | `#16A34A` | `#89D185` |

### 6.2 分组头与徽标

| 元素 | Token | 说明 |
| --- | --- | --- |
| 分组标题（项目文件/外部引用/回收站） | `muted.foreground` + 字号 11px | 全大写/加粗由样式决定 |
| 计数徽标 | `muted.background` 底 + `muted.foreground` 字 | 胶囊样式 |
| 外部引用图标 | `info` | 路径引用语义 |
| 回收站图标 | `muted.foreground` | 非危险常态 |

### 6.3 文件类型图标色（复用标准字段，零 hex）

| 类型 | 后缀 | Token |
| --- | --- | --- |
| 文件夹 | — | `warning`（琥珀） |
| SQL / 脚本 | `.sql` | `info` |
| Python | `.py` | `success` |
| 数据文件 | `.csv .tsv .parquet .xlsx .db .duckdb` | `base.cyan` |
| 文档 / 其他 | `.md .json .txt` … | `muted.foreground` |

> 以上均为设计映射，若实测对比度不足（尤其 `base.cyan` 在浅色下）再调整为产品语义 token；默认不新增 token。

### 6.4 搜索高亮（待确认，可选新增产品语义 token）

v1 使用黄色 `<mark>`。若沿用标准字段，可用 `accent.background`（coral 淡底，与选中态区分度偏低）。建议新增产品语义 token（与 `theme-design.md` §5.4 同类，注册/消费方式一致）：

| 产品角色 | RDS Light | RDS Dark | 消费方 |
| --- | --- | --- | --- |
| `scratchpad.search.match.background` | `#FFF3C4` | `#4A3F00` | 搜索结果匹配文本底 |

若 0.6 schema 拒绝扩展字段，按 `theme-design.md` §5.4 落 `assets/themes/product-tokens.json`；未落地前先用 `accent.background` 兜底。

## 7. GPUI 落点映射

| 原型元素 | GPUI 落点 |
| --- | --- |
| 面板容器（草稿箱） | `crates/workbench/src/components/scratchpad_panel.rs`（`ScratchpadPanel: Entity<T>`） |
| 左 Dock 内容装配 | `crates/workbench/src/panels.rs`（`SidebarPanel` 的 `LeftPanel::Draft` 分支改调草稿箱视图） |
| 面板头 / 工具栏 | gpui-kit `Button`（`.icon().ghost()` 小尺寸） |
| 搜索输入 | `Input` + `InputState`（`cx.new(InputState::new)`） |
| 树 / 分组 / 折叠 | 自绘递归行 + `Disclosure`；>50 条用虚拟列表 |
| 右键菜单 | gpui-kit 弹层（`PopupMenu`/自绘 overlay），`popover` token 取色 |
| 导入 / 引用 / 冲突 / Diff 弹窗 | `Dialog` / 模态覆盖层 |
| 撤销栏 | 面板底部自绘条（`popover` + `primary`） |
| 域模型与存储 | `crates/scratchpad`（`models` / `state` / `store`），workbench 通过 workspace 依赖 `scratchpad` |
| 文件监控 | `notify`（v2 尚未接入，见 `scratchpad-dev-plan.md` Phase A） |
| SQL 草稿模式 | `crates/workbench/src/panels.rs` `EditorPanel`（新增 scratchpad-file 模式） |
| 提升为分析资源 | `analytics_resource` 服务，经 command/event 协作 |

## 8. 待确认项

1. **根目录语义**：草稿箱面板根 = 项目根目录，内部元数据迁至 `.RSmeta/scratchpad/`（`config.json` + `trash/`）——确认？
2. **旧数据迁移**：`{project}/.scratchpad/` 若存在，一次性迁移 `config.json`/文件到新位置，并清理空目录——确认迁移策略（迁移 vs 保留只读）？
3. **面板宽度**：240px 左 Dock 下工具栏压成单排图标；内容搜索结果、Diff、替换放中央编辑区——确认此分工？
4. **命名**：面板仍叫「草稿箱」（活动栏/命令已注册），语义为项目工作区——是否改名「项目文件/工作区」？
5. **搜索高亮 token**：新增 `scratchpad.search.match.background`，或先用 `accent.background` 兜底？
6. **当前项目会话**：草稿箱需要一个「当前项目根路径」来源（启动参数 / 最近项目 / 默认工作区），这是连接模块同源的缺口，见 `scratchpad-dev-plan.md` P0。
