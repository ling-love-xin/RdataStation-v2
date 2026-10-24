# #54 语义层剩余接线设计（C4 / C6-C7 / B 批）

> 状态：**设计定稿，待编译恢复后实施**（2026-10-24，编译被并行线 result_panel 重构中间态阻塞期间产出）
> 前置：engine 四能力已提交（诊断 `c7de3e06` / hover `129eb656` / 签名 `d69a971f` / 手册 `2f90e066` / 跳转+refs `1136862f`）；
> editor 侧已接：语义诊断 `96be41a8`、SQL hover `cd23301e`、函数手册 Dialog（代码已接待验）。
> 验证清单：`sql-semantic-verification.md` 批次 B / C。

---

## 一、C6/C7 跳转定义（表 / 列）——内核机制全链路可用，工作量最小

### 内核已有（vendor gpui-base `input/editor/lsp/definitions.rs`，零改造）

- 槽位：`EditorState::lsp_mut().definition_provider: Option<Rc<dyn DefinitionProvider>>`（`lsp/mod.rs:47`，pub 字段，类比 `install_hover` 直接赋值）
- `DefinitionProvider::definitions(&Rope, offset, Window, App) -> Task<Result<Vec<LocationLink>>>`
- **hover 预取自动**：hover 处理路径 `hover.rs:100` 已调 `handle_hover_definition`——SQL hover 已接后，**挂上 definition_provider 即自动获得**「悬停出下划线 + Ctrl+Click hitbox」
- Ctrl+Click：`handle_click_hover_definition`（secondary modifier + 非 alt + hover_definition 同 offset）→ `go_to_definition(location)`
- 键盘：`GoToDefinition` action（`on_action_go_to_definition`）——**键位待确认**：内核 `input/base/state.rs` init 是否自带 F12 绑定，没有则 app 补 `F12 → editor context`
- `go_to_definition` 分派：**外部 URI**（http/https）→ `cx.open_url`；**其他 scheme** → 先问 `show_document` handler（宿主返回 true = 自己处理）；默认 = 当前文档内移动光标（`target_selection_range`）

### editor 侧实施（新文件 `lsp/sql_definition.rs`，类比 `sql_hover.rs`）

```
SqlDefinitionProvider { dialect: SqlDialect }   // 无状态，与 SqlHoverProvider 同构

impl DefinitionProvider for SqlDefinitionProvider {
    fn definitions(&self, text: &Rope, offset, window, cx) -> Task<Result<Vec<LocationLink>>> {
        // 后台 background_spawn → engine::sql::jump_at(sql, dialect, &[], offset)
        // JumpTarget { kind, table, column: Option<String>, start, end }
        // → LocationLink：
        //   origin_selection_range = start..end（内核自动画下划线 + Ctrl+Click hitbox）
        //   target_uri = rds://navigator/<限定表名>       ← 虚拟 scheme，标识「导航树定位」
        //   target_selection_range = 0..0                 ← 表/列位置由宿主展开时决定
    }
}
```

安装（host Sql 分支，与 hover 同点）：`bridge::install_definition`（仿 `install_hover` 泛型化）。

### workbench / database 侧（并行线区域，本期只设计不实施）

`show_document` handler（`lsp_mut().show_document`）：解析 `rds://navigator/<限定名>` →

1. 按**文档绑定连接**（editor 已有 `bound_connection()`）在导航树定位该限定名
2. 命中 → 展开树 + 选中该节点 + 返回 `true`
3. 未命中（连接未绑 / 表不在目录）→ 返回 `false`（内核默认：当前文档内移动光标到词，无副作用）

### 口径与边界

- **无目录退化**：`jump_at` 无目录时表仍可用（裸名 → `public.xxx` 限定名）；列需要目录（同 hover 口径，先退化）
- 函数不参与跳转（engine 已定：`JumpKind` 仅表/列）
- 命中符号的**下划线样式**由内核 `hover_definition_style`（`link_text` highlight style）控制，无需自绘

---

## 二、C4 签名帮助——内核无槽位，自建；选「独立 Dialog」而非「跟随光标 Popover」

### 事实

- 内核 **无** SignatureProvider 槽位（`lsp/` 下 8 个文件只有 code_actions/completions/definitions/document_colors/hover/overlay/semantic_tokens，无 signature_help；`overlay.rs` 是补全菜单状态，非签名）
- 签名数据 engine 已就绪：`functions.rs` 双源（在线系统表 + `builtin_signatures` 约 50 个）；函数手册 Dialog 刚证明「Dialog 基建 + engine 数据」组合可行

### 方案：`Ctrl+Shift+Space` 打开签名 Dialog（首版手动触发）

- **触发**：命令 `SignatureHelp`（actions! 注册）+ 键 `Ctrl+Shift+Space`（context `editor`，核对内核未占用——`Ctrl+Space` 已被补全占用，Shift 档空闲概率高，实施前 grep `input/base/state.rs`）
- **数据**：取**光标前的函数调用**（向前扫标识符 + 紧邻 `(`）→ engine 签名（`functions::signature_of(name)`）→ 参数列表 + **当前参数高亮**（按光标位置在括号内的逗号数推断）
- **UI**：复用 `dialogs.rs` 基建（`open_function_manual` 同款）——Dialog 标题 = 函数签名，正文 = 参数表（参数名/类型/默认）+ 中文说明；Esc 关闭
- **不用**跟随光标 Popover：需要监听输入事件 + 光标像素位置 + 滚动跟随，与内核滚动/补全菜单叠加复杂，首版不做

### 增强（二期，记录不实施）

- 输入 `(` 自动弹出（观察 `InputEvent::Change` + 文本尾部是 `(`）——需要去抖，避免打字时闪

---

## 三、B 批：语义诊断运行时注入（目录快照 + 方言）

### 现状（已接 vs 缺口）

| 环节 | 状态 |
| --- | --- |
| `Request.dialect` + editor 内部方言映射（`connection::sql_dialect`） | ✅ 已接（`96be41a8`） |
| `catalog_to_semantic` 转换 + 诊断语义段（目录非空才跑） | ✅ 已接 |
| scan 提交处传目录 | ❌ `scan.rs` `run(job)` 仍传 `Catalog::default()`（注释「目录与连接是补全面的，给空的不参与」）→ 语义诊断运行时静默跳过 |
| 目录数据源 | 在 workbench NavCache（database crate），editor 不可达 |

### 设计：editor 开放目录端口，workbench 注入

1. **editor 侧**（本期实施）：
   - `EditorShared` / service 加字段 `catalog_snapshot: Option<Arc<SemanticCatalog>>`（`SemanticCatalog` = editor 自有的表/列容器，`CatalogTable` 来源）
   - host `submit_scan`（L5436 附近）构造 Request 时：`catalog: self.catalog_snapshot()`（None → `Catalog::default()`，语义段跳过——与无目录退化口径一致）
   - `EditorShared` 加 `set_catalog_snapshot(conn_id, Option<Arc<...>>)` 方法（幂等；连接切换时清空）
2. **workbench 侧**（并行线/后续实施）：
   - 文档绑定连接变化 / 导航树目录刷新时，把该连接的目录快照推给 editor 的 `EditorShared`
   - 数据转换：NavCache → editor 语义目录（表/列，列限定名聚合——`96be41a8` 的 `catalog_to_semantic` 反方向）
3. **方言**：已接，`connection_db_type() → sql_dialect` 已在 scan 提交链上

### 验证点（对应验证清单 B 批）

- B1 绑定连接 + 选 Schema → 写 `SELECT missing FROM no_such_table` → 红波浪线精确指向原文
- B3 同一 SQL 在 MySQL / PG / DuckDB 连接间切换 → 方言跟随

---

## 四、实施顺序建议（编译恢复后）

| 顺序 | 项 | 依赖 | 验证 |
| --- | --- | --- | --- |
| 1 | C5 函数手册 Dialog 编译验证 + 提交（已写好待验） | 无 | `cargo test -p rds-editor --lib` |
| 2 | C6/C7 跳转（`sql_definition.rs` + install + show_document 设计交并行线） | engine jump_at（已提交） | 单测：LocationLink 映射（表/列/无目录退化） |
| 3 | B 批目录端口（editor 侧 `catalog_snapshot`） | 无（workbench 注入另排） | 单测：注入后诊断产出、空目录退化 |
| 4 | C4 签名 Dialog | engine functions 签名（已提交） | 单测：当前函数识别 + 参数高亮 index |

> 并行线依赖：C6 的 `show_document` 导航树定位在 workbench/database 侧；B 批的目录注入在 workbench 侧。editor 侧先行（本设计 2/3 的 editor 部分），workbench 侧留端口与文档。
