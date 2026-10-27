# 编辑器模块遗留问题清单（Backlog）

> 维护约定：本文档只记录**当前仍未完成 / 未接线**的事项；已完成项移入
> `editor-impl-taskbook.md` 或对应提交。更新时同步任务板（#37 等）与
> 本仓 git 提交链。状态字段：`待排期` / `已批准` / `进行中` / `下期` / `已关闭`。
> 最后盘点：2026-10-27（状态栏三项收尾：Ch 微标 / 注释修正 / 替换字形探查）。

## 一、结果网格（DBeaver 表格能力分期）

| 编号 | 项 | 内容 | 状态 |
| --- | --- | --- | --- |
| P0 | 引擎驱动上报列类型 | `QueryResult::with_batches` + 七驱动接线；`type_label` 已生效 | ✅ 已提交 `3b6e22d0` |
| P1 | 字段注释 | sqlglot 血缘映射（`projection_sources`）+ NavCache 列元数据 + 表头 tooltip 分块 | ✅ 已提交 `dbdc9f38` |
| P2 | 隐藏列 / 列统计 / BLOB / 转置 | P2-1 隐藏列（右键「隐藏此列/显示全部列」，视图列序↔数据列序折算，复制整行跳过隐藏列）+ P2-2 BLOB 摘要（`BLOB · N B`，悬停给全 hex）+ P2-3 列统计（count/NULL/min/max/avg/sum，复用值预览弹层）+ **P2-4 单行转置（DBeaver 式：面板级切换按钮，选中一行竖排成「# / 字段 / 值」，转置面板自带上一行/下一行导航，隐藏列跳过，只读视图，换结果自动退出）已提交** | ✅ 已提交（P2-1/2/3/4） |
| P3 | 钻取关联数据（右键值→查关联表） | `ColumnDetail.references` 已就位，取数链路现成 | 下期（用户明确） |

P1 边界（已实现，勿当缺口）：`SELECT *` 只展开实体表；表达式列无注释；
跨 schema 同名表取首个命中；解析失败 / 未绑定连接 / Federated 全列无注释且**不影响结果**。

## 二、已讨论、方案确认但未开工

| 项 | 方案要点 | 状态 |
| --- | --- | --- |
| DBeaver 式参数绑定 | SQL 里 `:name` → 弹参数表单 → 字面替换后执行（与结果区分期无关） | ✅ 已实现（B16，2026-10-24 盘点确认：`view/dialogs.rs::open_bind_dialog` + `host.rs` 挂起/替换，测试 `named_parameters_hold_execution_until_filled` 通过） |
| LSP hover 槽位 | 评估结论：本仓高亮/补全为自研（sqlglot + 关键字表），未接 LSP | 已关闭（#21）；2026-10-24 用户新定：通用模式接入 LSP（python，诊断+补全+hover），SQL 模式维持自研内核，见任务 #54 |
| 执行上下文模型（对标 DBeaver） | 现状物理连接 / 事务 / 取消令牌按 `conn_id` 跨文档共享、全局单忙位串行；目标 Source 档 per-context 独立连接 + 独立事务 + 独立取消，**明确不做通用连接池**；Accelerated / Federated 本地会话维持现有复用键。分阶段：① 事务状态 UI 归属文档级 + 清偿四通道同根因回执竞态 → ② 队列按连接分组并发、取消按作业句柄 → ③ 引入 ExecutionContext 独立连接 + 生命周期 + 独立元数据上下文。方案 A（per-context，推荐）/ B（单连接 + UI 归属修正）/ C（池化，不推荐）与待拍板项见 [`../connection/execution-context-model.md`](../connection/execution-context-model.md) | 讨论定稿未开工（下一版本，0.2.4 不实现） |

## 三、历史面板（右 Dock）缺口（2026-10-22 已全部接线）

现状：`crates/editor/src/view/history.rs` + `crates/editor/src/history.rs`；
数据源 `engine::persistence::history_store`。引擎侧 `HistoryFilter` 七维
（db_type / connection_id / success_only / failed_only / start_time / end_time / keyword）
匹配逻辑齐全（`HistoryStore::filter_records`），但对外只暴露了 keyword 一维。

| 缺口 | 现状 | 接线结果 |
| --- | --- | --- |
| 过滤维度 | UI 只有关键词搜索框；`search_sql_history(keyword, limit)` 只包 keyword | 引擎加 `query_sql_history(filter, limit, offset)`（含 `to_public_record` 公共投影）；面板加成功/失败筛选（TabBar segmented）+ 库类型下拉（DropdownMenu） |
| 翻页 / 加载更多 | `PAGE = 200` 每次加载最新 200 条，无更多入口 | 面板维护 offset，满页时出「加载更多」（`load_more` 追加下一页） |
| 多选批量删除 | 只有行内单条删除 + 清空全部 | 单击行 = 勾选（`HashSet` 选中集 + `ListItem::selected`），「删除选中 (N)」按钮逐条删 |
| 右键菜单 | 无 | `ContextMenuExt`：复制 SQL（写剪贴板）/ 删除该条；双击行 = 重放（对齐 DBeaver 对象列表口径） |

> 交互口径变更：行点击原为**单击重放**，现改**单击勾选 / 双击重放 / 右键菜单**（为多选让路）。
> 测试：`view::history` 7 例 + `history::tests` 7 例全绿（新增成败筛选 / 库类型筛选 / 多选删除 / 加载更多）。

## 四、结果网格其它

| 项 | 现状 | 说明 |
| --- | --- | --- |
| 列宽持久化 | DataTable/TableState 列宽拖拽现成，但不持久化 | 换文档/重启恢复默认宽；待评估是否落 NavCache/会话 |
| analyze 路径 | 分析/引擎内结果无列类型也无注释 | `ResultSet` 无类型字段；P1 已按「无注释」如实处理（已知边界） |

## 五、工程与流程

| 项 | 现状 | 影响 / 处置 |
| --- | --- | --- |
| origin/main 未推送 | 本地领先 14 个 commit（0.8.0 升级、P0、P1、多文档恢复、值查看器等） | 等用户拍板推送 |
| insight 线大量未提交 | 工作区 47 处：`crates/insight/*`、`plugin/*`、`workbench/tests/*`（3 个）、docs/insight 等 | 阻塞 `cargo test -p rds-workbench` 整包（test-cfg 编译错误）；P 线提交不碰 Cargo.lock / insight 文件 |
| Cargo.lock 未提交改动 | 被其他线改过 | 不随 P 线提交 |

## 六、已完结（盘点确认无遗留）

高亮修复（sql/python/rust/rlang/log）→ 分析模式（内核接入 / 渲染分发 / cell 升级 /
串行队列 / 笔记级动作）→ CSV 表格 → 多文档恢复 → 补全缓存回落实时内省 →
值查看器 → 结果区高度持久化 → P0 列类型 → P1 字段注释 → 工具栏/状态栏
对齐 + 图标化 + 微标拆分 + 行号槽收窄 + 左裁修复 → 通用/SQL 模式对齐核对 →
历史面板缺口（过滤 / 加载更多 / 多选删除 / 右键菜单，2026-10-22）→
P2 结果网格增强之隐藏列 / BLOB 摘要 / 列统计 / 单行转置（2026-10-22 全量落地）→
DBeaver 式参数绑定（B16，2026-10-24 盘点确认已落地）→
补全菜单对齐 tusk 补丁（kind 字形/捕获色/前缀粗体）+ 补全替换起点修复
（2026-10-24，`e016754a` / `3035dc74`）。

### 状态栏专项收尾（2026-10-27）

| 项 | 处置 |
| --- | --- |
| 光标全文偏移占位 0（`cursor_offset`） | ✅ **已接入真实值 + 右段 `Ch N` 微标**（`45ba0edd`）：内核字节偏移经 Rope `byte_to_char_idx` 换算字符（大文件 O(log n)），与 Ln/Col 相邻 |
| host.rs 状态栏控件注释过期（「重设」残留） | ✅ **已修正**（交互控件清单 = 编辑模式/缩进/换行/编码/不可见字符/事务/中断） |
| 不可见字符「替换符号」渲染（VSCode `·`/`↹`） | ⛔ **内核不支持，不硬做**：`gpui-base input/editor/decorations.rs` 的 `RangeDecorationStyle` 仅 `Fill`/`Frame` 两种几何样式，`RangeDecoration` 只有 range/style/color，**无 replacement 字形能力**；当前 Fill 填充标注是内核能给的几何表达上限。待内核 decoration 支持替代字形后再接 |

> 注：host.rs 的 cursor_offset 接入与注释修正与 HEAD 已一致（并行线曾提交同款），
> 本次仅 status_bar.rs 的渲染段与测试断言为新增并单独提交。

## 建议优先级

1. ~~阻塞阶段②：队列按连接分组并发 + 取消按作业句柄（任务 #52，进行中）~~ ✅ 已提交 `7ac86854`（2026-10-24）
2. 补全缓存 MetadataCache TTL/generation（任务 #53）：代码已提交 `75c8bb07`；2026-10-24 收口 workbench 编译后 `cargo test -p rds-workbench --lib editor_completion` **12 测试全绿**（TTL 边界 / 时钟回拨 / 过期重载代际递增）——✅ 已验
3. #54 语义层（engine 四能力全部完成：`c7de3e06` 诊断 / `129eb656` hover / `d69a971f` 签名 / `2f90e066` 手册 / `1136862f` 跳转 + refs）——**editor 侧接线全部完成**：
   - ① 语义诊断 editor 层已接（`96be41a8`：`Request.dialect` + Catalog→CatalogTable + diagnostics 语义段 + 3 测试）；B 批目录注入端口已提交（`76b59ac1`：`EditorShared::set_catalog_snapshot` + scan 提交链带目录与方言 + `sql_dialect_for_document` 统一口径）——**运行时语义诊断待 workbench 注入目录快照**（并行线设计；空目录 → 语义段跳过）
   - ② SQL hover 已接（`cd23301e`：`lsp/sql_hover.rs` + `install_hover` 泛型化 + Sql 分支挂载，函数签名/中文可测；表/列列列表待目录注入）
   - ③ 函数手册 Dialog 已提交（C5：`function_manual.rs` + `FunctionManual` action + `Ctrl+Shift+M` + 工具栏书本按钮，597→602 测试全绿）
   - ③ 签名帮助已提交（C4：`signature_help.rs` + `SignatureHelp` action + `Ctrl+Shift+Space`，当前参数位高亮；「输入 `(` 自动弹」留二期）
   - ④ 跳转定义已提交（C6/C7：`lsp/sql_definition.rs` + `install_definition`，`rds://navigator` 由 workbench show_document handler 解析定位导航树）
   - ④ 收尾：workbench 侧注入（文档绑定连接→目录快照推送 + show_document 的 rds://navigator 定位）属并行线设计，注入后 B1-B4 / C2 / C3 / C6 / C7 实测解锁
4. P3 钻取（下期）

## 七、文档残留（#54 相关，2026-10-24 已核实清理）

> 原表列 4 处（editor-dev-plan L29/L30/L199、README L86、prototype L601）经逐一核实：
> L29/L30 是 2026-09-18 的**历史日志**（转译「静默丢第二条」为当时发现的准确过去时叙述，保留）；
> prototype L600/L603 是当时 grep 操作的**历史记录**（含版本号，保留）；README L86 无相关措辞（行号已漂移）。
> 实际过时残留仅 1 处，已改：

| 位置 | 内容 | 处理 |
| --- | --- | --- |
| `README.md` L417 | 「sqlglot-rust 0.10.29（精确锁定）」 | ✅ 已改为 0.10.33 并补「SQL 语义层（optimizer）」用途 |

## 八、内核能力释放（2026-10-27，A1/A2/B2/B3/B4）

对照 vendor `gpui-base/src/input/` 能力面 vs `crates/editor/src/` 使用面盘点后，
用户拍板「释放全部能力、AI 置灰、1~4 项都实现」，本轮落地：

| 项 | 内容 | 落点 | 状态 |
| --- | --- | --- | --- |
| A1 | 代码操作（code_action_providers） | 新建 `lsp/sql_code_action.rs`（自研动作 ADD_WHERE/ADD_LIMIT 挂内核槽位）+ `lsp/bridge.rs` LSP 版转发 + host.rs 三分支接线（Sql / Lsp / None） | ✅ 3 单测全绿 |
| A2 | 文档颜色（document_color_provider） | 新建 `lsp/document_color.rs`：SQL 十六进制字面量 `#rgb/#rrggbb/#rrggbbaa` 色块；SQL 分支挂、通用 LSP 分支不挂（避免语言服务重复报色） | ✅ 4 单测全绿 |
| B2 | 编辑器正文右键菜单 | 新建 `view/editor_menu.rs`：能力位 → 中文菜单项纯函数（剪切/复制/粘贴/全选/转到定义/代码操作）；host.rs `Editor::context_menu` 定制（覆盖 Input 自带英文默认菜单），能力位从内核实体读（is_editable / selected_range / lsp providers） | ✅ 5 单测全绿 |
| B3 | 软换行开关（set_soft_wrap） | 设置模块加 `editor.soft_wrap`（默认 true，与内核一致）；render 权威同步点比对缓存后同步内核；设置页自动出开关（registry spec 驱动） | ✅ settings 35 全绿 |
| B4 | 语言编辑规则（set_language_config + 开关） | ① 设置模块加 `editor.auto_close`（自动闭合括号/引号）+ `editor.smart_indent`（智能缩进），render 同步 `set_auto_close`/`set_smart_indent`；② 新建 `lsp/language_config.rs`：`set_language_config("sql", …)` 注册 SQL 括号/引号配对；③ 主编辑器按高亮类型设语言键（`editor_language_key`：Sql→sql / Python→python / … / CSV/Log→空），cell 编辑器已用 `as_key()` 自动吃到 | ✅ 2 单测全绿 |
| B1 | Inline Completion（AI 置灰） | 内核槽位 = 覆写 `CompletionProvider::inline_completion()`（无独立装配方法）；「置灰」UX 未定（设置/状态栏开关 disabled + 默认关？还是预留安装口不挂？）——**下期定口径** | 待排期 |

已知取舍（记录，不硬做）：

- **SQL 的 `not_in([String, Comment])` 不生效**：syntax context 只来自全局 `LanguageProvider`
  （gpui-component 的 `ComponentLanguages`，查 tree-sitter grammar registry）；本仓 SQL 高亮走
  **自研词法**（`engine::sql::highlight`）不注册 grammar → `context_at` 恒 `Code` → 字符串/注释
  内输入引号/括号也会自动闭合。**不替换全局 provider**（会破坏 text/json/python 既有默认）；
  未来给 SQL 装 tree-sitter grammar 后自动生效，`lsp/language_config.rs` 无需改动。
- `view/mod.rs` 的 `indent_dialog` / `invisible_dialog` 模块注册属并行线（缩进/不可见字符），
  未随本线提交。

## 九、未决项（不编造，记录现状）

- AGENTS.md 标「GPUI-kit 0.8.x」而 vendor / 代码按 0.7.1（版本口径是否有意，未确认）
- 执行上下文模型（DBeaver per-context 独立连接）：讨论定稿未开工（0.2.4 不实现，见 `../connection/execution-context-model.md`）
- 列宽持久化（DataTable 拖拽现成，不持久化）：待评估是否落 NavCache/会话
- analyze 路径结果无列类型/注释（`ResultSet` 无类型字段，已知边界）
- 函数手册官方文档 URL：workbench 按「引擎+函数名」生成（真实环境验证后定）
- 列类型进目录后 hover 补类型：`annotate_types` 现多数表达式返回 Unknown，暂不做
