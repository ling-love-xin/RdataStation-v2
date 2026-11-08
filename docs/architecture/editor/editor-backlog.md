# 编辑器模块遗留问题清单（Backlog）

> 维护约定：本文档只记录**当前仍未完成 / 未接线**的事项；已完成项移入
> `editor-impl-taskbook.md` 或对应提交。更新时同步任务板（#37 等）与
> 本仓 git 提交链。状态字段：`待排期` / `已批准` / `进行中` / `下期` / `已关闭`。
> 最后盘点：2026-11-08（书签与折叠线、SQL 三执行通道核证、语义层 workbench 注入闭合；
> 新增 §十二 / §十三，漂移更正见文末「文档漂移更正」）。

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
| ~~列宽持久化~~ | ✅ 已交付（2026-11-08）：会话 `result_column_widths` + `列名=宽` 编解码 + `persist_column_widths` 开关 | 换文档/重启按列名恢复；列名对不上的列回默认档 |
| analyze 路径 | 分析/引擎内结果无列类型也无注释 | `ResultSet` 无类型字段；P1 已按「无注释」如实处理（已知边界） |

## 五、工程与流程

| 项 | 现状 | 影响 / 处置 |
| --- | --- | --- |
| origin/main 推送 | ✅ 已推送（2026-10-28）：44 个 commit 同步（0 领先 0 落后），含内核能力释放 / A 方案 / cell 菜单 / 结果集 tab 一致化等 | 完成 |
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
   - ① 语义诊断 editor 层已接（`96be41a8`：`Request.dialect` + Catalog→CatalogTable + diagnostics 语义段 + 3 测试）；B 批目录注入端口已提交（`76b59ac1`：`EditorShared::set_catalog_snapshot` + scan 提交链带目录与方言 + `sql_dialect_for_document` 统一口径）——✅ **workbench 注入已闭合（2026-11-08 核证）**：`workbench/src/view.rs:887 ensure_catalog_pump`（1 s 一拍、复用补全 L1 目录）+ `:1203` 挂载；空目录 → 语义段跳过
   - ② SQL hover 已接（`cd23301e`：`lsp/sql_hover.rs` + `install_hover` 泛型化 + Sql 分支挂载，函数签名/中文可测；表/列列表随目录注入已有数据源，**列级展示效果待人工验收**）
   - ③ 函数手册 Dialog 已提交（C5：`function_manual.rs` + `FunctionManual` action + `Ctrl+Shift+M` + 工具栏书本按钮，597→602 测试全绿）
   - ③ 签名帮助已提交（C4：`signature_help.rs` + `SignatureHelp` action + `Ctrl+Shift+Space`，当前参数位高亮；「输入 `(` 自动弹」二期已接入：`host/mod.rs:642-651` 在 `InputEvent::Change` 上判 `auto_call`，模态 Dialog 打开时不抢）
   - ④ 跳转定义已提交（C6/C7：`lsp/sql_definition.rs` + `install_definition`，`rds://navigator` 由 workbench show_document handler 解析定位导航树）
   - ④ 收尾：✅ **已闭合（2026-11-08 核证）**——目录快照推送（`ensure_catalog_pump`）与 `rds://navigator` 定位（`view.rs:238 attach_jump`）均已在 workbench 接线；B1-B4 / C2 / C3 / C6 / C7 转人工验收
4. P3 钻取（下期）
5. **SQL 格式化增强（2026-10-28 用户定：优先级高）**——见「十、SQL 格式化增强」

## 十、SQL 格式化增强（2026-10-28，优先级高）——已实施（本线提交）

**现状（已盘点）**：主链路完整——工具栏「格式化」按钮 → `format_document`（选区感知，只格式化选区）→ `format::plan` → 引擎 `formatter.rs`（sqlglot `generate_pretty` AST 排版，多语句安全：切分→逐条格式化→原位回填，一条失败不影响其它条；前导注释保留；解析失败原样返回并报条数）。

**已落地（2026-10-28）**：
1. **格式化选项**：`engine/sql/formatter.rs` 新增 `KeywordCase{Preserve,Uppercase,Lowercase}` + `FormatOptions{keyword_case,indent_width}`（Default = preserve / 2）；`format_with_report` / `format_single` 签名加 options。**安全后处理**：关键字大小写复用 `highlight_spans` 的 `TokenClass::Keyword` 区间改写（字符串/注释/标识符天然不碰）；缩进宽度逐行重排行首 2 空格倍数（行首落入 String/Comment 保护区间的行不碰）。**kept_verbatim 逐字保留语义高于选项**。
2. **设置入口**：`settings` 三个新项——`editor.format_keyword_case`（Enum：preserve/upper/lower）、`editor.format_indent_width`（Number：2/4/8 预设）、`editor.format_on_save`（Bool 默认关）；model / lib / registry 全接线，设置页按 registry 自动渲染。
3. **保存时自动格式化**：`host.rs::on_save`（Ctrl+S）开头按 `format_on_save` 开关先格式化再落盘；`Self::format_options(cx)` 异常档位回退 preserve/2 不猜。
4. **边界**：尾随注释会被 sqlglot 生成器丢弃（模块头已知边界，测试用前导注释）；行宽重排 sqlglot-rust 不支持（不做，已记录）。

**测试**：formatter 12 单测全绿（含关键字只改关键字、前导注释不改写、缩进含保护区不碰）；settings 契约除存量 `fold_open_keywords` 登记怪癖（Enum(&[]) 空候选 + 文本默认值）外全绿。

**缺口**（已解决）：
1. ~~无格式化选项~~ → 已实现 A/B（关键字大小写 + 缩进宽度）
2. ~~设置模块无格式化项~~ → 已登记三设置项（section editor / group "format"）
3. ~~保存时自动格式化~~ → 已实现 C（format_on_save 开关）

## 十一、高亮 token 自定义（2026-10-28 记录在案，优先级低于格式化）

**需求**：VS Code `editor.tokenColorCustomizations` 式——用户自定义「词 → token 类 / 颜色」。

**现状（已盘点）**：高亮三层全硬编码——① `engine/sql/highlight.rs::class_of`（TokenType 语义 + `TYPE_NAMES` 硬编码类型名表 + 函数启发式）；② `view/highlight.rs::token_name`（TokenClass → 14 个主题词汇，写死）；③ resolver = `theme.highlight_theme`（查 `rds-theme.json` highlight.syntax）。**无配置入口**。

**内核能力（已查证）**：`HighlightStyleResolver`（gpui-base `highlighting.rs:13`，`fn style(&self, name) -> Option<HighlightStyle>`）是应用侧 trait，`InputEditorStyle.highlight_styles: Arc<dyn HighlightStyleResolver>` 可任意提供——**支持包一层用户覆盖 resolver**（默认查主题 + 命中覆盖表先返回覆盖值）。

**方案（A+B）**：
- A 词级分类表：设置 JSON 声明「词 → token 类」，引擎 `class_of` 前先查覆盖表（TYPE_NAMES 同级数据驱动）
- B 颜色覆盖：设置声明「token 名 → 色值」，resolver 包一层用户覆盖表
- 新增 token 类（TOKEN_NAMES + 资产项）机制已成熟（14 项就是那么扩的）
- scope 级匹配（「字符串里的关键字」）需 token 携带 scope 链，大工程，**不做**

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
| B1 | Inline Completion（AI 置灰） | 设置模块加 `editor.inline_completion`（**disabled 置灰** + 默认关 + hint「能力已预留，放开后立即生效」）；`SettingSpec` 新增 `disabled` 字段、设置页 Bool 开关渲染为禁用态；model/lib 全接线（放开 disabled 即用，无需改设置侧）。**行为层天然置灰**：内核 `CompletionProvider::inline_completion()` 默认返回空数组，未挂任何 AI 源，恒不产出建议 | ✅ 已落地（随并行线 `56404b36` 提交落库；代码为本线所写，提交信息为并行线「布局设置项完整实现」） |

已知取舍（记录，不硬做）：

- **SQL 的 `not_in([String, Comment])` 已生效（A 方案，2026-10-27）**：syntax context 原只来自
  全局 `LanguageProvider`（gpui-component 查 tree-sitter grammar registry），本仓 SQL 走自研
  词法不注册 grammar → `context_at` 恒 `Code` → 字符串/注释内引号仍配对。**A 方案**（见下节）
  以 vendor patch 注册点 + 自研 provider 解除此取舍，`lsp/language_config.rs` 无需装 grammar。
- `view/mod.rs` 的 `indent_dialog` / `invisible_dialog` 模块注册属并行线（缩进/不可见字符），
  未随本线提交。

### A 方案：SQL 语法上下文自研封装（2026-10-27 落地，commit 见本线提交）

- **动机**：不装 tree-sitter-sql（避免「内核 grammar 理解 vs 自研 sqlglot 理解」两套偏差），
  字符串/注释判定**收口到自研词法器**，与高亮同源。
- **落地**：① vendor patch `gpui-component/input/syntax_context.rs` 新增
  `register_syntax_context_provider`（自定义 provider **优先于** tree-sitter grammar；
  `input/mod.rs` re-export，diff 备份 `tools/_archive/` 或 `.rds/tmp/patch-081/`）；② engine
  新增 `context_kind_at`（与 `highlight_spans` 共用同一 `Tokenizer`：行注释 `start..=end` 算
  Comment、块注释/字符串闭合后 Code、词法失败不猜 → Code）；③ editor 新建
  `lsp/sql_syntax_context.rs::SqlSyntaxContext` 实现内核 `SyntaxContextProvider`，
  `language_config::install` 注册 `"sql"`。
- **附带修复**：签名帮助 `current_call` 排除字符串/注释内的调用括号（判定 `open_at` 处上下文
  而非光标处，`SELECT 'substr('` 不再误弹）。
- **已知边界**：未闭合字符串（词法失败）整体 Code——签名帮助可能临时误弹，补上闭合引号即正确。

## 九、未决项（不编造，记录现状）

- AGENTS.md 标「GPUI-kit 0.8.x」而 vendor / 代码按 0.7.1（版本口径是否有意，未确认）
- 执行上下文模型（DBeaver per-context 独立连接）：讨论定稿未开工（0.2.4 不实现，见 `../connection/execution-context-model.md`）
- ~~列宽持久化~~：✅ 已交付（2026-11-08，落会话 `result_column_widths`，受 `result_panel.persist_column_widths` 开关控制）
- analyze 路径结果无列类型/注释（`ResultSet` 无类型字段，已知边界）
- 函数手册官方文档 URL：workbench 按「引擎+函数名」生成（真实环境验证后定）
- 列类型进目录后 hover 补类型：`annotate_types` 现多数表达式返回 Unknown，暂不做

## 十二、书签与折叠（2026-11-08 盘点）

**已交付基线（勿当缺口）**：v1——工具栏 `editor-bookmarks` 按钮（三模式通用）+ 右键
「添加书签/说明…」（`AddBookmarkNote`）+ 快捷键（`Ctrl+Shift+B` / `F8` 下一 / `Shift+F8` 上一）
+ 说明对话框 + 跳转（`jump_to_line`）+ sidecar 落盘；v4——书签管理器浮层
（`open_bookmark_manager`）：搜索定位框内嵌「全部 / 书签 / 折叠」类型按钮（`Input::suffix` +
`ButtonGroup`）、三栏列表（行号 ｜ 徽标+说明 ｜ 编辑 / 删除图标）、**双击行跳转**、空态区分
「无书签 / 过滤后无匹配」；工具栏图标固定 `IconName::Bookmark`（`23d6397d`，函数手册保持
`BookMarked`）。数据口径：`file_meta::Bookmark { line, label }`（1 起）；徽标判据
`label == 行号`；折叠无说明不记录、书签空说明如实保存（`apply_bookmark_label`）。

| 优先级 | 项 | 现状 / 缺口 | 状态 |
| --- | --- | --- | --- |
| 高（阻塞） | **gutter 书签入口 5 项**：① 已标记行常显书签轮廓 / 带横线书签 ② hover 淡色 + 单击切换 ③ Shift+单击弹说明 ④ 有说明的折叠行：箭头常显 + 箭头下方一道横线（「无说明」的可折叠行仍只在 hover / 当前行 / 已折叠时才显箭头）⑤ 指针停在有说明书签行时显示说明气泡（设置 `editor.bookmark_hover_note` 可关） | **已按授权改 vendor 落地（2026-11-08；第一轮 4 项 + 第二轮按像素验收反馈调整）**：内核 `vendor/gpui-base/src/input/base/state.rs` 的 `GutterMarker{line, kind, note}`（`note: SharedString`，空串 = 无说明；`Ord/PartialOrd` 只按 `(line, kind)` 比较，`set_gutter_markers` 仍按 `(line,kind)` 排序去重）；`GutterMarkerKind{Bookmark,BookmarkNote,FoldNote}` + `set_gutter_markers` / `gutter_markers` + 上报事件 `InputEvent::GutterMarkerClick{line,shift}`（切换 / 弹框语义留在应用层）。`element.rs`：书签列移到 gutter **最左**（`GutterMarkerLayout.column_width`，`marker_column_x = origin_x`，行号文本整体右移一列）；`layout_line_numbers` 仅在 markers 非空时留宽 18px、图标 13px，**无标记时列宽 0 且 hitbox 零宽**（不吞行号点击、未用书签的编辑器行号栏宽度不变）；折叠说明改为「箭头下方一道横线」（`GUTTER_FOLD_NOTE_UNDERLINE_WIDTH 9` / `HEIGHT 1.5`，画在箭头 hitbox 内不占新空间，`FoldIconLayout.fold_note_rows` 让该行箭头常显），原 9px 笔形角标 SVG 已删；悬停说明 `HoverNote`（`shape_line` 截断到 `GUTTER_NOTE_POPOVER_MAX_WIDTH 420px` 并补 `…`、内边距 6/3、画在书签列右侧该行下方、半透明底 `fill`）；hover 已标记 0.5 / 未标记 0.3，点击 `cx.stop_propagation()` 后上报。editor 侧：`view/host/bookmarks.rs::push_gutter_markers`（1 起→0 起，判据 `label == 行号`，有说明同时附 `BookmarkNote` 与 `FoldNote` 并下发同一 `note`；设置关掉时下发空串）+ `on_gutter_marker_click`（Shift → 说明框，否则切换）；`view/host/mod.rs` 订阅分派 + `editor_bookmark_hover_note_or_default` 读取包装；`view/host/render.rs` 新增 `show_bookmark_hover_note` 缓存比较 → 重新投影。设置 crate 全链新增 `editor.bookmark_hover_note`（`model/editor.rs` 字段 + 默认 true / `registry` Slot + slot_for + slot_kind(Bool) / `service/editor.rs` apply_slot + `bookmark_hover_note`/`set_bookmark_hover_note` / `service/mod.rs` by_key 分发 + value_by_key / `registry/entries/editor.rs` 登记项，设置页用 Switch）。测试：`crates/editor/src/view/tests/bookmarks.rs` 3 例（标记与说明投影、单击 / Shift 分派、设置关闭后说明不下发） | ✅ 已实现，待人工验收像素、hover 气泡与设置开关 |
| 中 | **折叠末行收起（统一 `end+1`）** | 内核 `fold_map.rs::rebuild` 只藏 `[start+1, end-1]`（首末行都强制可见），所以 `SELECT (` 折起来后 `)` 还挂着、`SELECT / a / b` 折起来 `b` 还挂着。用户口径：**不动内核**，在候选侧加一——`crates/editor/src/support/fold.rs` 的 `FoldSpan.end_line` 记**块真实末行**，`kernel_end_line()` 统一返回 `end_line + 1`，`view/fold.rs::to_kernel` 走它：内核因此藏 `[start+1, end]`，**两类块都只留首行**（`(`…`)` / `CASE`…`END` / 多行注释 / 缩进块最后一条内容行一起收起，对齐 VS Code）。缩进块末行恰是文档最后一行时也安全（内核 `rebuild` 越界兜底、`line_start_offset(end+1)` 落回文本末尾）。另：同起始行只发一条并留**范围最大**者（内核 `set_candidates` 的 `dedup_by_key(start_line)` 只留一个，否则折一次能收几行取决于排序） | ✅ 已实现，待人工验收 |
| 中 | 书签行号漂移不校正 | `view/host/bookmarks.rs:61`（「行号漂移暂不校正」）：编辑器插入/删除行后 sidecar 行号指向漂移 | 待排期 |
| 中 | 标签方案 B / C | 现只有 A「说明即标签」（`#tag` 约定，schema 与 sidecar 零变更）；B = 结构化 `tags: Vec<String>`（需 schema + sidecar 迁移）、C = 全局 Tag System（远期） | 待排期 |
| 低 | 行内悬停信息 + 设置开关 | 用户已定口径「跳转后鼠标位于当前行时显示信息，设置可关闭」——无实现痕迹（editor 源码无相关代码） | 待排期 |
| 低 | 书签交互无自动化测试 | headless 下对话框按钮点击分发不可靠（`simulate_click` 单独跑能中、全套跑中不了），只断言 `debug_bounds`；双击 / 编辑 / 删除靠人工验收；gutter 的 hover 淡色与图标像素亦无自动断言 | 已知限制 |

## 十三、SQL 三执行通道（2026-11-08 核证）

| 通道 | 入口 → 动作 | 状态 |
| --- | --- | --- |
| 快捷键 | `Ctrl+Enter` → `ExecuteSql` → `execute_preferring_selection`；`Ctrl+Shift+Enter` → `ExecuteAll` → `run_all`（`app/main.rs:268-272`） | ✅ 通 |
| 工具栏 | `editor-exec-run` / `editor-exec-all` / 执行位置族 5 项（`ExecMenuKind::ALL`，`exec/target.rs:442`）/ `editor-explain` | ✅ 通 |
| 宿主 | workbench `request.run` → `run_all` | ✅ 通 |

核心提交线唯一闭合：`execute` → `execute_labeled`（守卫拒绝留痕）→ 提交 → `ensure_exec_pump`
（`exec/pump.rs:12`）→ `drain_exec_results` 回填（`exec/pump.rs:93`）。

**唯一接通缺口**：右键菜单没有 SQL 执行项——`view/editor_menu.rs:65-107` 只有剪切 / 复制 /
粘贴 / 全选 / 添加书签说明 / 转到定义 / 代码操作。

## 跨 crate 指针：结果面板（rds-result-panel）

结果面板遗留清单在 [`result-panel-handoff.md`](result-panel-handoff.md) §2.4，本模块不重复维护。
2026-11-08 复核补一条更正：**「行级叠加层收尾」的 A2 依赖已解除**——事实档载体与构造链已交付
（`store/store.rs` 的「A2 事实档状态」槽位 + `FactFrame` 合并/降级；外部线 `7a9afb5c` 确认
「内存档/表档构造走 A2/B 批已交付链」）；剩余缺口是**渲染期尚未消费事实档**（注释「渲染第一期
不读它」）与行级叠加收尾本身。

## 文档漂移更正（2026-11-08，调用链核对）

以下声明仍以「遗留 / 未实现」措辞存在于代码注释，实际调用链已闭合；本批同步修正注释措辞：

| 位置 | 旧声明 | 实际情况 |
| --- | --- | --- |
| `view/host/toolbar/render.rs:408` | 【遗留】Markdown「预览 / 源码」切换 | 已闭合：`render.rs:160` 挂载 → `toggle_markdown_preview` → `editor_body.rs:28` 渲染 `TextView::markdown` |
| `core/model.rs:124` | 「笔记级动作当前未实现」 | 已闭合（2d）：`view/host/notebook.rs:315/338/378` 会话切换 / 全部运行 / 重启 + 工具栏挂载 |
| `view/signature_help.rs:12` | 「输入 `(` 自动弹」留二期 | 二期已接入：`host/mod.rs:642-651`（`InputEvent::Change` + `auto_call`，模态不抢） |
| `view/widgets/status_bar.rs:79` | 【2b/遗留 #14】脚本文件类型段 | 已接入，测试 `status_bar_shows_the_script_file_type_segment_in_general_mode` |
| `lib.rs:57` | 「其余控件（格式化 / 历史 / 更多 / 执行位置）未实现就不放」 | 已落地：「⋯ 更多」含方言转译 / 执行计划（`format.rs:91`），执行位置族 5 项在工具栏 |
| `result-panel-handoff.md:100` | 「列序拖拽 UI 未接线」 | 外部线 `7a9afb5c` 已删该条（五段链路闭合） |
