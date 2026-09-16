# 草稿箱模块（M5）· 设计理念与架构

> 本文回答**为什么这样设计 / 怎么运转**：不变式、概念模型、存储布局、数据流、决策表、降级、测试策略、实现映射，以及**权威的已知问题清单**。
> 视觉与交互规格看 `scratchpad-prototype-design.md`；进度与阶段任务看 `scratchpad-dev-plan.md`；使用方式看 `scratchpad-user-guide.md`。
> 状态：Phase A/B 已落地（2026-09-15）· Phase C 进行中（C-1 打开 / C-2 连接预选与执行回写 / C-3 脏点 / C-4 冲突 Diff 已接；剩拖放）· Phase D 未开始。

## 0. 裁决摘要（一页读完）

| # | 裁决 | 影响 |
| --- | --- | --- |
| 1 | **草稿箱根 = `{项目}/scratchpad/`**（可见模块目录），不是项目根、不是隐藏目录 | 「草稿」是目录语义；迁移/备份/引用都变简单 |
| 2 | **内部态全在 `{项目}/.RSmeta/scratchpad/`**（`config.json`） | 面板不需要忽略规则；用户可见目录里没有半内部文件 |
| 3 | **回收站为项目级** `.RSmeta/trash/`，条目自带 `origin` + `original_rel_path` | 草稿与将来的资源删除共用一个回收站，还原能回原模块原路径 |
| 4 | **导入与引用区分**：导入 = 复制进模块目录；引用 = 只记绝对路径 + 别名 | 体积/迁移/失效语义完全不同，UI 与后端都必须分开表达 |
| 5 | **编辑器无根**：树只发「打开这个绝对路径」的意图 | 编辑器不假设项目根，同一编辑器将来可开 `resources/`、`mock/` |
| 6 | **重结果落中央编辑区**（内容搜索结果与替换栏） | 侧栏只承载输入与开关；240 px 面板不承担宽内容 |
| 7 | **提升为资源 = 移动 + 只读存档**，修改须取回（检出） | 归档是"冻结凭证"语义，与 M6 的版本模型对齐 |
| 8 | **窗口 = 项目**：项目态不放进程单例，不做 multi-root | 会话/监控/元数据复杂度可控 |
| 9 | **业务与视图都在 crate**（2026-09-16 视图下沉，A'4） | 复制/搜索/替换/路径防护与面板纯函数可单测（32 项），宿主能力经 `ScratchpadHost` 端口注入 |

## 1. 定位与边界

### 1.1 与 M4 / M6 的三段式

```
M4 数据源管理/导航       M5 草稿箱（本文）          M6 资产库/分析存档
「我能看到什么数据」  →  「我正在做什么」       →  「我留下了什么，它当时长什么样」
库/表/列元数据           随手写的 SQL、脚本、       只读、有版本、带来源的
只读内省                 导入/引用的临时数据        正式存档
```

草稿箱是**工作台（项目）内的临时区**：允许乱、允许删、允许失效；一旦某份草稿值得留存，**提升**到 M6 冻结为存档（Phase D）。

### 1.2 与相邻模块的关系

| 模块 | 关系 |
| --- | --- |
| `project`（M1） | 提供项目会话（`OpenProject.root`）、项目锁、只读标志；**回收站位置与清单格式由本模块实现、位置约定与项目元数据目录一致** |
| `connection` / `database`（M2/M3/M4） | 草稿只保存**连接 ID**（`file_meta.last_connection_id` / `bound_connections`），执行与内省完全在它们内部 |
| `editor` | Phase C 接线的对方：草稿箱发「打开这个路径」，编辑器负责打开/回存/脏点/冲突 |
| `analytics_resource`（M6） | Phase D 的对方：草稿箱右键「提升为分析资源」经 command/event 发起，草稿箱**不依赖**其视图 |
| `mock`（M7） | 无直接依赖；M7 的产物进 `mock/` 与草稿箱同级 |

### 1.3 明确不做

- 不做跨项目共享草稿（YAGNI：草稿的价值在于"随手"，共享必须引入权限与并发语义）；
- 不做 multi-root 工作区（见 §7.4）；
- 不在树里做数据计算（计算属 `engine` / DuckDB）；
- 不自己实现文件监控（`notify` 接入是 Phase A 余项，接口已预留 `watcher_active`）。

## 2. 概念模型

### 2.1 实体

| 实体 | 位置 | 说明 |
| --- | --- | --- |
| **条目**（`ScratchpadEntry`） | `{项目}/scratchpad/**` | 文件或文件夹；名字即真相（重命名 = 真改文件名） |
| **外部引用**（`ExternalReference`） | `config.json` | 别名 + **绝对路径** + 创建时间；不复制本体 |
| **引用状态**（`ExternalReferenceStatus`） | 运行时计算 | `exists` / `is_dir`，用于置灰与「重新引用」入口 |
| **文件元数据**（`FileMeta`） | `config.json` | `last_connection_id` / `last_executed_at` / `bound_connections`（**仅 ID**） |
| **回收站条目**（`TrashEntry`） | `.RSmeta/trash/<id>/` | 自带 `manifest.json`（§4） |

### 2.2 四条不变式

1. **内容即文件**：草稿箱里没有任何"虚拟条目"——树中的每一项都对应磁盘上的真实路径，用户用系统文件管理器看到的与面板看到的**一致**（点前缀条目除外）。
2. **内部态不泄漏**：`.RSmeta/` 下的任何东西都不出现在树里、也不能经 API 读写（§5.1）。
3. **删除可逆**：任何删除都先进入项目级回收站，且撤销栏在 5 s 内提供一键还原。
4. **引用不搬运本体**：引用只改"怎么找到它"，不复制、不移动外部文件。

### 2.3 术语表（本项目内固定）

| 术语 | 含义（不要混用） |
| --- | --- |
| **草稿箱 / 工作区** | 模块整体；根 = `{项目}/scratchpad/` |
| **导入** | 从系统文件对话框复制文件**进来**（计体积、随项目迁移） |
| **引用（外部引用）** | 只记外部路径 + 别名（不计体积、可能失效）；**不是**代码引用 |
| **提升 / 归档** | 草稿 → 只读存档（Phase D，属 M6 语义） |
| **取回 / 检出** | 存档 → 可编辑工作副本（Phase D） |
| **模块目录** | 项目根下每个功能模块自己的可见目录：`scratchpad/`、`resources/`、`mock/` |

## 3. 存储布局

### 3.1 目录树

```
{项目}/
├── scratchpad/                     ← 草稿箱根（可见；内容 = 用户文件）
│   ├── 临时订单分析.sql
│   ├── data/…
│   └── …（点前缀条目被隐藏：树跳过，API 拒绝）
├── resources/                      ← M6 归档目标（Phase D；本模块只发起移动）
└── .RSmeta/
    ├── scratchpad/config.json      ← 模块内部态（外部引用 + file_meta）
    └── trash/                      ← 项目级回收站（本项目所有模块共用）
        └── <id>/{payload, manifest.json}
```

### 3.2 `config.json`

```jsonc
{
  "external_references": [
    { "alias": "下载数据", "path": "D:/data", "created_at": "2026-09-15T08:00:00Z" }
  ],
  "file_meta": {
    "临时订单分析.sql": {
      "last_connection_id": "P_1",
      "last_executed_at": "2026-09-15T08:10:00Z",
      "bound_connections": ["P_1", "G_2"]
    }
  }
}
```

- `file_meta` 的键是**相对模块根的路径**：项目整体迁移（换盘符/换目录）后依然有效。
- **绝不存凭据**：连接密码在 `connection` 的 `auth_store`（AES-256-GCM）；这里只有 ID。
- 配置读写有内存缓存 + 锁（`config_cache`），避免每帧重复解析。

### 3.3 旧布局迁移（`migrate_legacy_layout`）

v1/早期 v2 用过 `{项目}/.scratchpad/` + `.scratchpad.json`，迁移规则（幂等、非破坏）：

| 旧 | 新 |
| --- | --- |
| `.scratchpad/**`（内容） | `scratchpad/**`（不覆盖同名） |
| `.scratchpad.json` | `.RSmeta/scratchpad/config.json` |
| `.scratchpad/.trash/**`、早期 `.RSmeta/scratchpad/.trash/**` | 项目级 `.RSmeta/trash/`（逐条建 manifest） |
| 迁移后遗留的空壳目录 | 清理 |

迁移只在 `ensure_dir()` 内触发一次（旧目录存在时），失败不会破坏原数据。

## 4. 项目级回收站

### 4.1 为什么是"自包含目录"而不是"平铺文件 + 索引"

```
.RSmeta/trash/<id>/
├── payload          ← 被删的文件或目录（原样移动进来）
└── manifest.json    ← { id, name, origin, original_rel_path, kind, deleted_at, size }
```

- 还原所需信息**不依赖任何额外状态**：删掉索引也不会丢还原能力；
- `id` 唯一 → 同名/同路径重复删除互不覆盖（v1 平铺方案会覆盖）；
- 移动（rename）而非复制，删除大目录近乎零成本。

### 4.2 语义

| 操作 | 语义 |
| --- | --- |
| 删除 | `move_to_trash(路径, origin, 相对路径)`；`origin` 是模块标识（草稿箱为 `"scratchpad"`） |
| 列出 | 面板只展示，不区分来源（尾部标签显示 `origin`） |
| 还原 | 按 `origin` 分发：**草稿箱只还原 `origin == "scratchpad"` 的条目**，其他来源报错提示在其模块中还原 |
| 同名冲突 | 还原时若原路径已被占用 → 自动改名（不覆盖） |
| 清空 / 清除单条 | `empty()` / `purge(id)`，不可恢复 |
| 撤销栏 | 删除后 5 s 内可一键还原本批（`trash_ids` 记录本批新增 id） |

### 4.3 跨模块归属

回收站 API 按**模块无关**设计（`origin` + `relative` 由调用方给），但当前唯一使用方是草稿箱。按架构约定「≥2 使用方才上提」，M6 落地后再决定是否把它上提到 `shared`/`engine`。

## 5. 路径安全与只读

### 5.1 路径解析（`resolve_path` / `validate_name`）

| 规则 | 目的 |
| --- | --- |
| 拒绝含 `..` 的路径 | 防穿越出模块根 |
| 拒绝首段以 `.` 开头的路径 | `.RSmeta` 等内部目录天然不可经 API 触达 |
| 规范化（去 `\\?\` 前缀、前导分隔符）后 `canonicalize` 校验前缀 | 防符号链接/短路径绕过 |
| 列表跳过点前缀条目 | 面板与磁盘视图一致（用户放的点文件不会冒出来） |
| 名称校验：非空、无分隔符、无 `..`、不重名 | 新建/重命名/别名统一口径 |

### 5.2 只读项目

项目以只读打开（`Shared.project_ui.read_only`，由 `ProjectLock` 判定）时，面板层统一拒绝：新建 / 重命名 / 删除 / 粘贴 / 替换 / 引用增删改，并在状态栏提示。**crate 层不做只读判断**（它不知道项目打开方式），守卫留在 workbench，这是有意的边界划分。

## 6. 数据流

### 6.1 列表（首次进入 + 刷新）——**全异步**

```
render_scratchpad（首次 or loaded=false）
  → request_scratchpad_load(cx)：只入队 + 起轮询，**不做 I/O**
      · 无项目 → 直接置错误态并 invalidate 在途序号
      · 有项目 → scratchpad::jobs::enqueue_root_load(root, 已展开子目录)
                 并把返回的 seq 记入 view.load_seq（丢弃过期结果）
  → 工作线程（scratchpad::jobs::worker）在 tokio 运行时内完成：
      ensure_dir（含旧布局迁移，幂等）
      list_local_entries(0)（仅模块根）
      external_reference_status()（探测引用路径存在性）
      list_trash()
      对每个已展开子目录 list_directory_entries（失败忽略 = 已删）
  → 结果入队；轮询印（`ensure_scratchpad_pump`，60 ms）取回并 apply_scratchpad_loads：
      seq < view.load_seq 的旧结果直接丢弃；否则写入条目/引用/回收站/子目录缓存
  → cx.notify() 重绘；状态行在在途期间显示「加载中…」
```

- 展开文件夹的懒加载同样走队列（`enqueue_dir_load` → `apply_scratchpad_dirs`）。
- 渲染期只做「读状态 + 压平 + 算行高」，不再有任何 `Runtime::new` / `block_on` / 文件系统调用。

### 6.2 树渲染与虚拟化

- `flatten_scratchpad` 产出行序列 `(缩进层级, 条目)`，子目录优先取懒加载缓存，无缓存时用内联 `children`；
- 渲染走 **`v_virtual_list`**：`item_sizes` 逐行给出（树行 = `ui::ROW_HEIGHT`，内联编辑行 = `ui::CONTROL_HEIGHT_SM`），只画可视区；
- 虚拟列表的 `scroll_handle` 存在视图状态里，键盘导航用它把选中项滚入视口；
- **内联新建行占一个显示位置**：显示序号 ↔ 真实行序号通过 `ScratchpadRowCtx::{is_edit_row, real_index}` 换算，避免"插一行就错位"。

### 6.3 新建（含模板与落点）

```
起点：工具栏 ＋ / 🗀、Ctrl+N、空态按钮
  → start_scratchpad_edit(NewFile | NewFolder)
      落点 = 唯一选中且为文件夹 → 该目录（并自动展开）；否则模块根
      模板 = 记住上次选择（SQL/Python/Markdown/JSON/空白）
  → 内联行（Input + ✓/✕）渲染在落点位置；切换模板 chip 会替换"模板补的后缀"
  → Enter / ✓ → commit_scratchpad_edit
      create_entry(final_name, parent, is_folder)
      模板非空白 → save_file(相对路径, 占位内容)
  → loaded=false → 重新拉取（§6.1）
```

### 6.4 移动与复制

| 操作 | 后端 | 要点 |
| --- | --- | --- |
| 剪切移动 | `move_entry(src, dst_parent)` | 同名/同路径直接拒绝；`file_meta` 键随之迁移 |
| 复制 | `copy_entry(src, dst_parent)` | 文件与**文件夹递归**；命名避让 `_copy`/`_copy_N`（上限 1000）；**拒绝复制到自身子树**（防递归无界）；文件用 `fs::copy` 保证二进制安全；`copy_dir_contents` 深度上限 `MAX_DEPTH` |

### 6.5 删除 → 回收站 → 撤销

```
删除（行 ✕ / Del / 右键）
  → 批量 delete_entry（逐个 move_to_trash，记录本批 id）
  → 撤销栏（5 s 定时器；仅当仍指向同一批才清空）
  → 清空选择 / 重载（§6.1）
回收站区：逐条「还原」→ restore_from_trash（校验 origin）→ 重载
```

### 6.6 导入 vs 引用

```
导入（⬇，系统文件对话框多选）
  → import_scratchpad_files 只入队（scratchpad::jobs::Import）
  → 工作线程 import_external_file × N：复制进模块根（命名避让）
  → 回填：通知「已导入所选文件」+ 重载（部分失败也重载：已成功的会显现）
引用（🔗，文件或目录）
  → 选路径 → 内联输入别名（默认取名称）→ add_external_reference(alias, abs_path)
      · 引用只改 config.json（元数据级）→ 保持事件路径同步，不入队
  → 加载时 external_reference_status() 探测：不存在 → 置灰 +「（丢失）」
  → 行操作：↗ 打开位置 / ✎ 改别名 / ⟲ 重新引用（update_external_reference_path）/ ✕ 移除
```

### 6.7 内容搜索

```
侧栏（内容模式）：查询 + `.*`（正则）+ `Aa`（大小写）+ ⏎
  → 只入队（scratchpad::jobs::Search）+ 确保轮询印在跑
  → 工作线程 search_file_content(query, case, context=2, is_regex)
       · 遍历 ≤ MAX_DEPTH(4) 的树；单文件 30 s 超时；命中总数 500 截断
       · 正则循环外编译一次（RegexBuilder::case_insensitive）
       · 每行附 match_spans（字节区间，≤16 段/行；字面量模式在大小写不敏感时
         先校验小写化是否改变字节长度，改变则放弃高亮）
  → 回填（OpResult::Search）：ScratchpadSearchView 写入 Shared::scratchpad_search → 编辑区渲染
       · 命中文本高亮用产品 token `search.match.background`
       · 点命中标题 → `request_open_in_editor`（相对路径 → 模块内绝对路径；同路径只激活）
         跳转到具体行待编辑器跳行端口
       · 失败：清空结果视图 + 侧栏错误行显示原因
```

### 6.8 替换

```
结果面板：「替换为」输入 + 「全部替换」（禁用条件：输入为空）
  → replace_scratchpad_all 只入队（scratchpad::jobs::ReplaceAll）+ 置 Shared::scratchpad_pump_request
       并先在通知栏显示「替换中…」
  → 工作线程一次任务完成三步（避免中间态）：
      ① 先搜一遍得到**去重文件列表**（与侧栏搜索开关语义完全一致）
      ② 逐文件 replace_in_file(file, pattern, replacement, is_regex, case_sensitive)
           · 统一走 regex：字面量模式先 regex::escape（共享大小写开关）
           · 字面量模式替换串用 NoExpand（`$` 不当分组引用）；正则模式支持 `$1`
           · 无命中不写盘
      ③ 重搜一遍，随结果一起回传（新增命中也一并呈现）
  → 回填：通知「已替换 N 处（M 个文件）」+ 刷新结果视图；只读项目在动作入口拒绝
```

### 6.9 懒加载缓存的刷新（易错点）

`load_scratchpad` **不是**清空子目录缓存，而是记住已展开过的父目录并逐个重拉：

- 若只清缓存：任何一次操作（新建/删除/粘贴）后，已展开的文件夹会**看起来空了**（`expanded` 还在，`children` 没了）；
- 若保留缓存：会显示过期内容。
- 因此取"记住父目录 → 重新 `list_directory_entries` → 失败的（如已删除）忽略"的策略。

### 6.11 外部改动 → 去抖重拉

```
面板首次加载时（或项目根变化时）：ensure_scratchpad_watch
  → ScratchpadWatcher::start({项目}/scratchpad)（递归监听；.RSmeta 不监听）
       · OS 事件（含 Err）只置位 ChangeFlag，不做增量同步
  → ensure_scratchpad_watch_poll：常驻任务，每 ~1.2 s 看一次标记
       标记为真且「未在内联编辑 且 无加载在途」→ view.loaded = false + cx.notify()
       → 下一帧走 §6.1 正常重载（含已展开子目录）
```

- **为何是标记 + 轮询而不是事件驱动立即刷新**：编辑器保存一次常触发多条 OS 事件，
  立即刷新会把 UI 打成刷新循环；标记法天然合并事件风暴。
- **为何不做增量同步**：要复刻 `scan_dir_tree` 的排序/过滤/懒加载/行号语义，必然两处真相；
  草稿箱是临时区，一次重拉可控。
- **不回激自己**：只监听内容目录（配置写入在 `.RSmeta/`，不在监听范围）；
  且每次发起重载时会先清一次标记（本次重拉已包含此刻之前的所有改动）。
- **降级**：监控启动失败（权限/网络盘）只记 `tracing::warn`，退化为手动 `↻`，不影响功能。

### 6.12 打开文件（Phase C-1 已接 + C-2 前半：连接预选）

```
双击行 / Enter / 右键「打开」（仅文件）
  → ScratchpadView::request_open_scratchpad_file
  → ScratchpadHost::open_in_editor(path)（= Shared::request_open_in_editor，绝对路径）
  → 宿主 WorkbenchView::render 消费（take_open_in_editor，取出即清空）：
      open_in_editor(path) → editor::persist::open_file（同路径已打开只激活，不重读）
                            → show_document（建/复用 Dock 面板，加中央 tab 组）
      新建文档且路径在 `{项目}/scratchpad/` 下时：
        store.relative_path_of(path) → store.file_meta(rel).preferred_connection()
        → 该 id 在下拉里（`EditorShared::connection_options`）则 EditorService::set_connection
      失败 → 通知栏「打开文件失败: …」
```

- **为何不在草稿箱视图里直接开**：文档与 Dock 面板属宿主（`WorkbenchView`）状态，
  且编辑器**无根**（只认绝对路径）——视图只发“要打开这个路径”的意图。
- 模式与只读等级由编辑器按路径自行判定（`editor::mode::resolve_mode`）；
  草稿箱不做后缀分支。
- 连接预选（C-2 前半）只对**新建**文档生效：同路径已打开走“只激活”，不覆盖用户手动改过的连接；
  已被删除的连接（不在下拉里）不预选——绑上去只会让执行报错。读元数据属「元数据级操作保持同步」的既定口径（K1c）。
- **待接**（Phase C 余项）：冲突 Diff（`store.rs::diff_with_content` 已在手，未接 UI）、拖放导入/拖入编辑区。

### 6.13 执行后回写连接（Phase C-2 后半，已接）

```
编辑器执行完成（`ExecChannel` 出一条 `ExecOutcome`）
  → `EditorShared::drain_exec` 顺带留一份回执 `ExecReceipt{document, connection, succeeded}`
      · 结果仍归编辑区；回执只有“哪份文档、用了哪个连接、成没成”
      · 没人取走时队列封顶 64 条（只保留最近一批）
  → workbench `services::scratchpad_meta::write_back`（宿主 1 s 一拍，装配期在 `init_workspace` 起泵）
      · 只算成功的执行；文档 → 绝对路径 → `ScratchpadStore::relative_path_of` 落回模块内身份
      · 模块外的路径（`resources/`、临时文件）直接跳过；同一草稿多条回执**后到者胜**
  → `store.update_file_meta(rel, conn)` 写 `last_connection_id` + `last_executed_at`
      · 属元数据级写入（K1c）：一次小 JSON 写，保持同步，失败进状态栏提示
```

- **为何泵挂在宿主而不是草稿箱面板上**：侧栅切到别的工具时草稿箱不渲染，而执行照样会发生；
  泵在 `WorkbenchView` 上，与面板可见性无关。
- **为何不让草稿箱直接读编辑器**：`scratchpad` 不得依赖 `editor`；回执是编辑器对宿主的“公告”，
  消费方（草稿箱）自己判定“这路径是不是我的地盘”。
- 下游效果：下次打开这份草稿时，`file_meta` 里的绑定会被用作预选（§6.12）。

### 6.14 键盘导航

| 输入 | 路径 |
| --- | --- |
| `↑` / `↓` | `scratchpad_visible_keys()`（按当前过滤/排序/展开态重新压平）→ 移动单选 → `scroll_to_item(i, Center)` |
| `Enter` | 文件夹：展开/折叠（展开时懒加载）；文件：Phase C 前回落「打开所在位置」并提示 |
| `Ctrl+N` | 新建文件（落点规则同 §6.3） |

动作定义在 `workbench/commands.rs`（`scratchpad` key context），绑定在 `app/main.rs`；行点击会 `focus` 面板，保证快捷键生效。

### 6.15 冲突 Diff（Phase C-4，已接）

```
外部改动（watcher 置标记，与脏点/重拉同一拍）
  → `detect_scratchpad_conflicts`：取宿主脏文档 ∖ 「这文件在草稿模块内」
  → 每份新冲突：`host.draft_content(abs)` 拿编辑器缓冲 → `jobs::enqueue_diff(root, rel, 缓冲)`
      · 面板立即出现冲突条（「差异」按钮先置灰，标「计算中…」）
  → 工作线程：`store.diff_with_content(rel, 缓冲, "磁盘", "编辑器")` → `OpResult::Diff`
      · 全是 `Unchanged` → **不是真冲突**（外部改动已被写回 / 就是自己写的），撤掉提示
  → 冲突条 3 个动作：
      · 「差异」→ `host.show_diff(Some(…)` → 中央编辑区 `render_scratchpad_diff_pane`（左=磁盘/右=编辑器，行号 + 红/绿）
      · 「重载」→ `host.reload_draft(abs)`：编辑器读盘 → `set_content` → `mark_saved`（冲突消解）
      · 「忽略」→ 只移除本地冲突条目；下次磁盘再变才重新报
```

- **判据是内容而不是 mtime**：草稿箱分不清“磁盘上的新内容是不是就是我自己写进去的”，而 `diff_with_content` 能分清。
- **消解动作在侧栅的冲突条上，不在中央面板里**：它们要同时改编辑器与草稿箱自己的状态，
  面板只负责把差异看清楚（与「重结果落中央编辑区」同一分工）。
- 缓冲区只有两处会被请求（差异 / 重载），所以端口不做缓存（dirty 轮询只拿路径）。
### 6.16 脏点回显（Phase C-3，已接）

```
编辑器里改动未保存（`EditorService::dirty_ids`）
  → 宿主端口 `ScratchpadHost::dirty_files()`：脏文档的**绝对路径**集合
  → 视图侧 1.2 s 轮询（与目录监控同拍）比对缓存 `dirty_seen`，有变化才 `cx.notify()`
  → `scratchpad_row` 按条目绝对路径命中 → 名字之后画一个实心圆点（`primary`；与原型 §2 一致）
      · **只有文件**打点（文件夹不画）；`Ctrl+S` 回存后集合里消失，点随之消失
```

- **为何不在 `render` 里问宿主**：脏文档集合是**外部状态**（编辑器拥有），所以走「轮询取回 + 缓存比对」，
  `render` 只读自己的缓存——与「render 是纯读路径」一致。
- **为什么经端口而不是让草稿箱依赖 `editor`**：`scratchpad` 不得依赖编辑器 crate；
  「哪些文档脏了」本质是宿主能回答的问题（同项目根 / 只读判定）。

## 7. 分层与依赖

### 7.1 crate 切分

```
app ──► workbench ──► scratchpad ──► workbench_shell / gpui-kit / shared
 │            │             └────► project / database / settings / engine（宿主侧，不进 crate）
 │            └────► project / database / settings / engine
 └─ 宿主能力经本 crate 定义的 `ScratchpadHost` 端口注入（实现在 `workbench/src/components/scratchpad_host.rs`）
```

| 层 | 内容 | 测试性 |
| --- | --- | --- |
| `crates/scratchpad` | 全部文件系统与配置语义（含递归复制、搜索、替换、路径防护、回收站、迁移）+ **面板视图与后台任务** | 可单测（33 项：`#[tokio::test]` + 临时项目目录 + 面板纯函数） |
| `crates/workbench` | 宿主端口实现（项目根 / 只读 / 提示 / 重绘 / 搜索结果落地 / 打开文件 / 脏文档集合）、左 Dock 装配（`SidebarPanel` 持 `Entity<ScratchpadView>`） | 目前靠人工验收 |

**为什么复制/搜索放在 crate 而不是面板**：它们是"文件系统语义"，会随 `resources/`、`mock/` 复用；放 view 层则既难测又会被复制三份。

### 7.2 状态所有权

| 状态 | 归属 | 生命周期 |
| --- | --- | --- |
| `ScratchpadStore` | 按窗口按需构造（`Shared::scratchpad_store()`） | 每次调用新建，无进程级缓存 |
| `ScratchpadView` | `SidebarPanel` 持有的 `Entity<ScratchpadView>`（crate 内定义，自持状态） | 面板生命周期 |
| `Shared::scratchpad_search` | `Shared`（面板与编辑区**共用**） | 窗口生命周期 |
| `scratchpad_pump`（轮询任务） | `ScratchpadView` 的 `RefCell<Option<Task<()>>>` | 任务空闲自退；视图销毁后 `weak.update` 失败即结束 |
| `Shared::scratchpad_pump_request` | `Shared`（`Cell<bool>`） | 编辑区发起的替换需复用侧栏轮询印；置位后由 `ScratchpadView::render` 消费（**任何左侧面板模式下都消费**，仅“完全隐藏”时留到恢复侧栏的那一帧） |
| `jobs` 工作线程 / 结果队列 | 进程级单例（OnceLock，在 **crate 内**） | **无项目态**：只装「任务 + 结果」，不装当前项目；项目根作为参数传入 |
| `ScratchpadWatcher`（目录监控） | `ScratchpadView` 的 `Option<…>` | 随视图存活；项目根变化时换监控点；drop 即停止监听 |
| 监控轮询任务 | `ScratchpadView` 的 `RefCell<Option<Task<()>>>` | 常驻（1.2 s 一拍）；视图销毁后自动结束 |
| `ScratchpadState` | crate 提供，**当前无生产调用方** | 将来 watcher 用，接入时按窗口持有 |

> 为何工作线程可以是单例而项目态不行：线程与队列是无状态基础设施（等同连接池），每个任务自带 `project_root`，不会串项目。项目态（当前面板看到的条目/选中/展开）始终在窗口的 `Shared` 与视图实体里。

### 7.3 只读与锁（两道护栏）

1. **`ProjectLock`**（`project` crate，`{.RSmeta}/project.lock`）：同一项目根二次打开时弹「只读 / 仍要打开 / 取消」；
2. **只读标志联动**（`Shared.project_ui.read_only`）：草稿箱写入操作全部拒绝并提示。

两者是**独立**的：锁管"能不能开"，只读标志管"开着能做什么"。

### 7.4 为什么不做 multi-root

multi-root 会把三件事的复杂度抬高一个量级：项目会话（一个窗口 N 个项目）、文件监控（N 份 watcher 与失效判定）、元数据（`config.json` 与 `file_meta` 的相对路径基准不再唯一）。而本产品的定位是**应用实例即项目**（见 `../project/README.md`），草稿箱不需要 multi-root。

## 8. 决策表

| # | 决策 | 备选与否决理由 |
| --- | --- | --- |
| D1 | 根用**可见目录** `scratchpad/` | ❌ 项目根（"草稿"无法判定，需忽略规则） / ❌ 隐藏目录（用户无法直接取走文件） |
| D2 | 内容目录用英文名，内部态用 `.RSmeta/<模块>/` | ❌ 内部态混在内容目录（面板需要过滤、易误删） |
| D3 | 回收站**项目级**且自包含 manifest | ❌ 模块级回收站（跨模块删除要建 N 个）/ ❌ 平铺 + 索引（同名覆盖、索引一丢无法还原） |
| D4 | 导入与引用**分开建模** | ❌ 统一成"添加"（体积/迁移/失效语义会被埋没） |
| D5 | 引用存**绝对路径** | ❌ 相对路径（引用目标在项目外，相对基准没有意义） |
| D6 | 编辑器**无根**、按绝对路径打开 | ❌ 编辑器绑定项目根（将来 `resources/`、`mock/` 无法复用） |
| D7 | 重结果（搜索/替换/Diff）落**中央编辑区** | ❌ 侧栏自渲染（240 px 装不下，且与编辑器能力重复） |
| D8 | 提升 = **移动 + 只读存档**，修改须取回 | ❌ 复制（两份真相）/ ❌ 就地加锁标记（用户可直接改文件绕过） |
| D9 | 项目态**不放进程单例** | ❌ 单例（多窗口 = 多项目时串味；`ScratchpadState` 因此不是默认路径） |
| D10 | 递归复制放 crate，`_copy` 命名避让 | ❌ 让用户先改名（打断操作）/ ❌ 报错（多选粘贴体验差） |
| D11 | 内容搜索限制：`MAX_DEPTH=4` / 单文件 30 s / 500 条截断 | 草稿箱是临时区，不为超大目录做全量索引（保持"随手搜"的响应） |
| D12 | 命中高亮由**后端给区间** | ❌ 前端重算（要复刻正则与大小写语义，两处真相） |
| D13 | 树整行唯一滚动区（虚拟列表），引用/回收站底部固定限高 | ❌ 全列一起滚（草稿一多，引用/回收站被推走且无滚动条 → 实际不可达） |

## 9. 降级矩阵

| 场景 | 现状行为 | 用户可感知 |
| --- | --- | --- |
| 未打开项目 | 面板显示「未打开项目：草稿箱根即项目目录，请先打开项目」 | 引导去 M1 打开项目 |
| 项目只读 | 写操作全部拒绝 | 状态栏提示「只读模式：不允许…」 |
| 项目目录不可写（权限/网盘） | store 报 `CoreError::storage` | 面板顶部错误行显示原因 |
| 引用目标缺失 | 置灰 +「（丢失）」+ `⟲` 重新引用 | 可自行修复 |
| 文件非 UTF-8（`read_file`/搜索） | 读取失败 → 该文件跳过（搜索计入 scanned，不 panic） | 搜索命中里看不到它 |
| 同名新建/重命名/粘贴 | `create_entry` 拒绝 | 内联编辑保留、可改名重试 |
| 大目录展开 | 只拉一级（懒加载） | 展开有短时耗时，无全量卡顿 |
| 命中超过 500 条 | 截断并标记「已截断」 | 结果头部可见 |
| 单个超大文件搜索 | 30 s 超时后跳过该文件 | 命中数不含它（不阻塞整体） |

## 10. 性能与可观测

| 措施 | 数值 / 位置 |
| --- | --- |
| 全异步加载 | 模块根 / 子目录加载都在 `scratchpad::jobs` 工作线程；渲染期零 I/O；在途期间状态行显示「加载中…」，且不用空态占位闪现 |
| 重操作后台化 | 导入 / 粘贴 / 清空回收站 / 搜索 / 替换都在工作线程执行，UI 线程只做入队与回填；长任务期间界面可继续交互（可切换面板、浏览其他内容） |
| 懒加载 | 首次只取模块根 `depth=0`；展开时 `list_directory_entries` |
| 虚拟化 | `v_virtual_list` 只渲染可视区行；行高逐行给出（重命名行更高） |
| 搜索预算 | `MAX_DEPTH=4`、单文件 30 s、总数 500、每行命中区间 ≤16 |
| 配置缓存 | `config_cache` + `Mutex`，避免每帧解析 JSON |
| 复制预算 | 名称避让 ≤1000 次尝试；递归深度 ≤`MAX_DEPTH` |
| 过期结果防护 | 模块根加载带自增 `seq`；项目关闭/切换时 `invalidate_loads()` 推进序号，旧结果一律丢弃 |
| 外部改动去抰 | 只监听内容目录（不含 `.RSmeta`）；1.2 s 轮询标记合并事件风暴；重拉时保留已展开子目录 |
| 可观测 | 面板底部状态行：加载中 / 文件数 / 文件夹数 / 引用数 / 回收站数 / 排序；搜索结果头部：命中数 / 扫描文件数 / 开关标记 |

## 11. 测试策略

- **crate 层（已自动化）**：`#[tokio::test]` + 临时项目目录 + 面板纯函数，覆盖 36 项：
  模块根与元数据隔离、内部路径拒绝、回收站来源与原路径、跨模块还原拒绝、旧布局迁移、引用状态与重命名校验、引用重定位、绑定往返、绑定与最近执行回读（含预选优先级）、绝对路径→模块内相对路径、回执→草稿映射（模块过滤 + 后到者胜）、搜索正则与大小写、命中区间（含 Unicode 变宽回退）、递归复制与重名避让、字面量/正则替换与 `$` 语义、Diff 行分类与两侧行号、面板纯函数（排序/压平/模板后缀/搜索结果映射/脏点判据/Diff 行前级）、后台任务（根加载/目录加载/导入与清空回收站/粘贴/搜索替换共用载荷）。
- **面板层（窗口级仍人工）**：多选（Ctrl/Shift/Ctrl+A）、剪贴板、模板与落点、虚拟列表滚动与键盘导航、替换栏、只读拒绝、**打开草稿预选连接**、**脏点生灭**、**冲突条与 Diff 面板**——见 `scratchpad-user-guide.md` §9 验收清单。
- **未自动化原因**：窗口级交互需要窗口与主题环境；按架构约定（窗口测试走宿主入口）应在 `workbench` 侧补，属后续工作项（§13.4）。

## 12. 实现映射

| 设计点 | 代码落点 |
| --- | --- |
| 根 / 元数据 / 回收站路径 | `crates/scratchpad/src/store.rs::{new, ensure_dir, META_DIR_NAME, MODULE_DIR_NAME}`、`trash.rs::ProjectTrash::new` |
| 旧布局迁移 | `store.rs::migrate_legacy_layout` |
| 路径防护 | `store.rs::{resolve_path_impl, validate_name, relative_path_of}`（`relative_path_of` = 反向：绝对路径 → 模块内相对路径，供「这个路径是不是草稿」的判定） |
| 列表 / 懒加载 | `store.rs::{list_local_entries, list_directory_entries, scan_dir_tree}` |
| 递归复制 / 移动 | `store.rs::{copy_entry, copy_dir_contents, move_entry}` |
| 搜索 / 替换 / Diff | `store.rs::{search_file_content, literal_match_spans, replace_in_file, diff_with_content}` |
| 外部引用 | `store.rs::{add/remove/rename/update_external_reference_path, external_reference_status}` |
| 文件元数据 | `store.rs::{file_meta, bind_connections, update_file_meta}` + `models.rs::{FileMeta, FileMeta::preferred_connection}`（读侧 → 打开预选；写侧 → 绑定与执行回存） |
| 回收站 | `trash.rs`（manifest 与服务） + `store.rs::{delete_entry, list_trash, restore_from_trash, empty_trash}` |
| 面板视图与编排 | `scratchpad/src/scratchpad_view.rs`（`ScratchpadView`：`render_scratchpad` / `scratchpad_row` / `render_scratchpad_edit_row` / `render_scratchpad_empty_state` / `scratchpad_move` / `scratchpad_open_selection` / `create/replace…`；**2026-09-16 由 workbench 下沉，宿主能力走 `ScratchpadHost`**） |
| 宿主端口与装配 | `scratchpad/src/host.rs`（trait）+ `workbench/src/components/scratchpad_host.rs`（实现）+ `workbench/src/panels/mod.rs`（`SidebarPanel` 持 `Entity<ScratchpadView>`） |
| 搜索结果与替换栏 | `scratchpad/src/scratchpad_view.rs::{render_scratchpad_search_pane, run_scratchpad_search}` + `workbench/src/panels/editor.rs::replace_scratchpad_all` |
| 快捷键与尺寸 | `scratchpad/src/commands.rs`、`workbench/src/commands.rs`、`workbench_shell/src/ui.rs`、`app/src/main.rs` |
| 文件类型色点 | `scratchpad/src/scratchpad_view.rs::scratchpad_icon_color` |
| 后台加载（K1） | `scratchpad/src/jobs.rs`（`enqueue_root_load` / `enqueue_dir_load` / `drain_loads` / `drain_dirs` / `invalidate_loads`）+ `scratchpad_view.rs::{request_scratchpad_load, ensure_scratchpad_pump, apply_scratchpad_loads, apply_scratchpad_dirs}` |
| 文件监控（Phase A5） | `scratchpad/src/watch.rs`（`ScratchpadWatcher` / `ChangeFlag`）+ `scratchpad_view.rs::{ensure_scratchpad_watch, ensure_scratchpad_watch_poll}` |
| 重操作后台化（K1b） | 同上模块的 `enqueue_import` / `enqueue_paste` / `enqueue_empty_trash` / `enqueue_search` / `enqueue_replace_all` / `drain_ops` + `scratchpad_view.rs::apply_scratchpad_ops`（侧栏）与 `EditorPanel::replace_scratchpad_all`（仅入队） |
| 冲突 Diff（C-4） | `scratchpad_view.rs::{detect_scratchpad_conflicts, render_scratchpad_diff_pane, ScratchpadDiffView}` + `jobs.rs::enqueue_diff` + `host.rs::{draft_content, reload_draft, show_diff}` + `workbench/src/panels/editor.rs::set_scratchpad_diff` |
| 执行后回写连接（C-2 后半） | `editor::shared::{ExecReceipt, drain_exec_receipts}`（编辑器的公告）+ `workbench/src/services/scratchpad_meta.rs`（1 s 一拍、`draft_targets` 过滤）+ `store.rs::update_file_meta` |
| 脏点（Phase C-3） | `host.rs::ScratchpadHost::dirty_files`（宿主取编辑器 `EditorService::dirty_ids`）+ `scratchpad_view.rs::{dirty_seen, refresh_dirty_cache, scratchpad_shows_dirty_dot}` |
| 打开草稿（Phase C-1）/ 连接预选（C-2 前半） | `scratchpad_view.rs` 发请求 → `host.rs::open_in_editor` → `Shared::request_open_in_editor` → `view.rs::open_in_editor`（同路径只激活；草稿带 `file_meta` 绑定时 `EditorService::set_connection` 预选） |

## 13. 已知问题（权威清单）

### 13.1 已修（保留条目与结论，供回归对照）

| # | 问题 | 处理 |
| --- | --- | --- |
| K1 | ~~`render` 期做 I/O~~ ✅ **已修（2026-09-16）** | `render_scratchpad` 首次进入不再同步读盘，改为 `request_scratchpad_load`（只入队）+ `ensure_scratchpad_pump`（60 ms 轮询回填）；新增 `scratchpad/src/jobs.rs`（单工作线程 + tokio 运行时 + 结果队列 + 请求序号防过期；初版在 workbench，同日随解耦移入 crate）。展开文件夹同理走 `enqueue_dir_load`。渲染期只剩「读状态 + 压平 + 算行高」 |
| K1b | **搬运字节 / 遍历全树的操作已后台化** ✅ **已修（2026-09-16）** | 导入、粘贴（剪切与复制）、清空回收站、内容搜索、批量替换均入队 `scratchpad::jobs` 并回填：新增 `Import` / `Paste` / `EmptyTrash` / `Search` / `ReplaceAll` 任务与 `OpResult` 回填，统一由 `apply_scratchpad_ops` 处理文案/刷新/通知 |
| K1c | **仅改元数据的操作保持同步**（有意保留） | 新建 / 重命名 / 删除入回收站 / 回收站还原 / 引用增删改 / 打开所在位置仍是事件路径同步 `block_on`：它们是单次系统调用（微秒~毫秒级，即 rename/write 小 JSON），迁到后台反而增加状态同步成本。**判定标准：看操作是否可能搬运字节或遍历全树** |
| K2 | **同一文件两处状态** | 搜索视图在 `Shared`，替换输入在 `EditorPanel`，查询词在侧栏输入框——目前靠约定同步；若将来支持多搜索会话需要收敛 |

### 13.2 一致性风险

| # | 问题 | 说明 |
| --- | --- | --- |
| K3 | ~~文件监控缺失~~ ✅ **已修（2026-09-16）** | `scratchpad::ScratchpadWatcher`（`notify` 递归监听模块根）+ 面板侧 1.2 s 去抖轮询重拉；监控失败降级为手动 `↻`；`.RSmeta` 不在监听范围内，不会自激 |
| K4 | ~~搜索结果可能过期~~ ✅ **已修（2026-09-16）** | 结果面板是快照；替换在**同一次任务内**重搜；外部改动由监控拍发现后，若面板仍开着则用**同一套查询/开关**重跑一次搜索（与重拉列表同步发生） |
| K5 | 大小写不敏感高亮的回退 | 小写化改变字节长度（如 `İ`）时**不高亮**但**仍命中**——行为正确但不完美（有单测锚定） |

### 13.3 待拍板（产品决定）

| # | 事项 | 影响 |
| --- | --- | --- |
| K6 | 引用目录是否可在树内展开浏览 | 现在只作入口（`↗` 打开位置）；展开需要"跨根路径树"的读写策略 |
| K7 | 多文件 Tab 与草稿箱的关系 | 已接首片（双击/Enter/右键「打开」→ 编辑器；同路径只激活不重读）；**脏点（C-3）与冲突 Diff（C-4）均已接**（前者经 `dirty_files` 回读，后者经 `draft_content` / `reload_draft` / `show_diff` 端口）；Tab 体系仍属编辑器侧 |
| K8 | 提升（Phase D）时引用与 `file_meta` 的处置 | 归档是"连引用一起冻结"还是"只冻结内容"，需与 M6 语义裁决书对齐 |

### 13.4 工程债

| # | 问题 | 说明 |
| --- | --- | --- |
| K9 | 面板自绘控件 | 工具栏按钮（`tool_btn`）、树展开字符（`▸/▾`）、分组头仍是自绘 div；按组件选型规范应逐步换 gpui-kit `Button`/`Collapsible` |
| K10 | 面板层缺自动化测试 | 见 §11 |
| K11 | `ScratchpadState` 无生产调用方 | 要么在 Phase C 接到 watcher，要么按"无使用方即删"清理 |
| K12 | 占位视图仍在 | `workbench` 的「资源」「插件」面板仍是占位（Phase D / 后续模块） |
