# 编辑器模块遗留问题清单（Backlog）

> 维护约定：本文档只记录**当前仍未完成 / 未接线**的事项；已完成项移入
> `editor-impl-taskbook.md` 或对应提交。更新时同步任务板（#37 等）与
> 本仓 git 提交链。状态字段：`待排期` / `已批准` / `进行中` / `下期` / `已关闭`。
> 最后盘点：2026-11-09（逐单元补全/诊断/执行错误三条闭合后重排全量遗留，见 §16
> 「遗留总账」；§15.14 / §15.15 为本轮改法记录）。

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
| origin/main 推送 | ⚠️ **本地领先 71 个提交、未推送**（2026-11-08 复核：`git rev-list --left-right --count origin/main...main` = `0	71`，`origin/main` 停在 `9ce3c98e`）。旧记录「已推送，0 领先 0 落后（44 个）」已过期 | 用户口径：只做本地合并，推送另行安排（见 `../ui/gpui-kit-0.9.0-upgrade-report.md` §0） |
| insight 线未提交（外部线） | 2026-11-08 复核：未提交只剩 `docs/architecture/insight/**`（1 处修改 + 6 个未跟踪），`crates/insight/*` / `plugin/*` 均已落库 | ✅ **不再阻塞**：`cargo test -p rds-workbench --lib --no-run` exit 0（2.07s，已编过）；用例本次未跑 |
| Cargo.lock 未提交改动 | 2026-11-08 复核 `git status` 不含 Cargo.lock | ✅ 已无改动 |

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

- ~~AGENTS.md 标「GPUI-kit 0.8.x」而 vendor / 代码按 0.7.1~~：✅ **口径已升到 0.9.0（2026-11-08 复核）**——`Cargo.toml` / `Cargo.lock` / `vendor/{gpui-base,gpui-component}` 全为 **0.9.0**、`gpui-pre` **0.3.11**（`../ui/gpui-kit-0.9.0-upgrade-report.md` §2）；`Cargo.toml` 用 `gpui-kit = { version = "0.9.0", features = ["gpui-fast"] }`。AGENTS.md 的「GPUI-kit 0.8.x」已同步改为 0.9.x；其「本地索引覆盖 0.7.1–0.8.0」逐版本枚举**未能从仓库内证实**（索引属工具侧），故未动该句
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
| 高（阻塞） | **gutter 书签入口 5 项**：① 已标记行常显书签轮廓 / 带横线书签 ② hover 淡色 + 单击切换 ③ Shift+单击弹说明 ④ 有说明的折叠行：箭头常显 + 箭头下方一道横线（「无说明」的可折叠行仍只在 hover / 当前行 / 已折叠时才显箭头）⑤ 指针停在有说明书签行时显示说明气泡（设置 `editor.bookmark_hover_note` 可关） | **已按授权改 vendor 落地（2026-11-08；第一轮 4 项 + 第二轮按像素验收反馈调整）**：内核 `vendor/gpui-base/src/input/base/state.rs` 的 `GutterMarker{line, kind, note}`（`note: SharedString`，空串 = 无说明；`Ord/PartialOrd` 只按 `(line, kind)` 比较，`set_gutter_markers` 仍按 `(line,kind)` 排序去重）；`GutterMarkerKind{Bookmark,BookmarkNote,FoldNote}` + `set_gutter_markers` / `gutter_markers` + 上报事件 `InputEvent::GutterMarkerClick{line,shift}`（切换 / 弹框语义留在应用层）。`element.rs`：书签列移到 gutter **最左**（`GutterMarkerLayout.column_width`，`marker_column_x = origin_x`，行号文本整体右移一列）；`layout_line_numbers` 仅在 markers 非空时留宽 18px、图标 13px，**无标记时列宽 0 且 hitbox 零宽**（不吞行号点击、未用书签的编辑器行号栏宽度不变）；折叠说明改为「箭头下方一道横线」（`GUTTER_FOLD_NOTE_UNDERLINE_WIDTH 9` / `HEIGHT 1.5`，画在箭头 hitbox 内不占新空间，`FoldIconLayout.fold_note_rows` 让该行箭头常显），原 9px 笔形角标 SVG 已删；悬停说明 `HoverNote`（`shape_line` 截断到 `GUTTER_NOTE_POPOVER_MAX_WIDTH 420px` 并补 `…`、内边距 6/3、画在书签列右侧该行下方、半透明底 `fill`）；hover 已标记 0.5 / 未标记 0.3，点击 `cx.stop_propagation()` 后上报。editor 侧：`view/host/bookmarks.rs::push_gutter_markers`（1 起→0 起，判据 `label == 行号`，有说明同时附 `BookmarkNote` 与 `FoldNote` 并下发同一 `note`；设置关掉时下发空串）+ `on_gutter_marker_click`（Shift → 说明框，否则切换）；`view/host/mod.rs` 订阅分派 + `editor_bookmark_hover_note_or_default` 读取包装；`view/host/render.rs` 新增 `show_bookmark_hover_note` 缓存比较 → 重新投影。设置 crate 全链新增 `editor.bookmark_hover_note`（`model/editor.rs` 字段 + 默认 true / `registry` Slot + slot_for + slot_kind(Bool) / `service/editor.rs` apply_slot + `bookmark_hover_note`/`set_bookmark_hover_note` / `service/mod.rs` by_key 分发 + value_by_key / `registry/entries/editor.rs` 登记项，设置页用 Switch）。测试：`crates/editor/src/view/tests/bookmarks.rs` 3 例（标记与说明投影、单击 / Shift 分派、设置关闭后说明不下发） | ✅ 已实现并人工验收（2026-11-08 用户回「验证基本没有问题」） |
| 中 | **折叠末行收起（统一 `end+1`）** | 内核 `fold_map.rs::rebuild` 只藏 `[start+1, end-1]`（首末行都强制可见），所以 `SELECT (` 折起来后 `)` 还挂着、`SELECT / a / b` 折起来 `b` 还挂着。用户口径：**不动内核**，在候选侧加一——`crates/editor/src/support/fold.rs` 的 `FoldSpan.end_line` 记**块真实末行**，`kernel_end_line()` 统一返回 `end_line + 1`，`view/fold.rs::to_kernel` 走它：内核因此藏 `[start+1, end]`，**两类块都只留首行**（`(`…`)` / `CASE`…`END` / 多行注释 / 缩进块最后一条内容行一起收起，对齐 VS Code）。缩进块末行恰是文档最后一行时也安全（内核 `rebuild` 越界兜底、`line_start_offset(end+1)` 落回文本末尾）。另：同起始行只发一条并留**范围最大**者（内核 `set_candidates` 的 `dedup_by_key(start_line)` 只留一个，否则折一次能收几行取决于排序） | ✅ 已实现并人工验收（2026-11-08 用户回「ok，没有问题了」；`3cdf651f`） |
| 中 | 书签行号漂移不校正 | **已在 `8234474b` 实现，本轮只校准记录**：`view/host/bookmarks.rs::adjust_bookmarks_for_edit` 在 `InputEvent::Change` 里 `take_last_edit_delta`（vendor 私有 patch）→ `support/bookmark.rs::adjust_for_edit` 按「区前不动 / 区内锚到首行 / 区后整体平移」改书签并回写 sidecar；本轮补端到端用例 `bookmarks_follow_an_inserted_line`，并修掉 `bookmarks.rs:61` 那句已过时的「暂不校正」注释。原证据（`:61` 注释）已不成立 | ✅ 已实现（记录已校准） |
| 中 | 标签方案 B / C | 现只有 A「说明即标签」（`#tag` 约定，schema 与 sidecar 零变更）；B = 结构化 `tags: Vec<String>`（需 schema + sidecar 迁移）、C = 全局 Tag System（远期） | 待排期 |
| 低 | 行内悬停信息 + 设置开关 | 用户口径「跳转后鼠标位于当前行时显示信息，设置可关闭」。已交付：指针停在有说明书签 / 折叠行时出说明气泡，设置 `editor.bookmark_hover_note` 可关（随 gutter 5 项，`8234474b`）；**跳转后自动弹出已实现（2026-11-08，`set_gutter_note_pin`，见 §15.10）**——书签管理器双击与 F8 落地即显示该行说明，指针一动不动也可见。仍缺：hover 气泡无 300ms 延迟（`gutter_hovered_row` 当帧计算，实现路径见 §15.10 末） | 自动弹出已交付；延迟待排期 |
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

## 十四、本轮技术债清除（2026-11-08）

| 项 | 缺口 | 处理 | 状态 |
| --- | --- | --- | --- |
| Toolbar 埋点刷日志 | `view/host/toolbar/render.rs` 每帧一条 `tracing::info!`，光标闪烁每 0.5s 出帧 → 启动 13 秒积 93 行 | 改 `static TOOLBAR_TRACE: std::sync::Once` + `call_once`，只记首帧一次，无运行时成本 | ✅ 已清 |
| `rds-mock` 两条 warning | ① `mock_view/mod.rs` `use crate::history::{self, …}` 的多余 `self`；② `pub(crate) fn spawn_history` 收私有 `HistoryTask`（`private_interfaces`） | ① 去掉 `self`；② `HistoryTask` 提 `pub(crate)` 并注明白为何不留在私有 | ✅ 已清 |
| `generators` 测试是孤儿 | `144c4c0b` 拆文件时把内联 `mod tests` 提成 `crates/mock/src/generators/tests.rs`，却没在 `generators/mod.rs` 声明；同时 `text.rs` 末尾留着指向不存在 `generators/text/tests.rs` 的 `#[cfg(test)] mod tests;` → `cargo test -p rds-mock --lib` 自那时起 `E0583` 编译失败，**69 个生成器用例从未跑过**（`cargo check` 不带 `--all-targets` 不编译 `cfg(test)`，所以 workspace check 一路绿灯） | `mod.rs` 补 `#[cfg(test)] mod tests;`（附原因注释）、删 `text.rs` 断链与文件末尾多余的模块闭合括号；`cargo test -p rds-mock --lib` 首跑 243 例 | ✅ 已修，243 passed |
| 类型不匹配警示用例走错入口 | `column_dialog_shows_type_mismatch_warning` 用「换生成器」触发警示，但 `apply_generator_config_to_draft` 会顺带把 `data_type` 同步成生成器产出类型 → 恒不匹配；真实触发是「改类型下拉、生成器不动」 | 把下拉回调抽成 `MockDetailView::select_column_type`（UI 与用例同一入口），用例改走它 | ✅ 已修 |

## 十五、全模块遗留盘点（2026-11-08，调用链核对）

盘点口径：文档声明 + 代码内 `暂不/后续/尚未/未实现` 声明（58 处）+ 设置项消费核对
（`settings/registry/entries/editor.rs` 17 个 `editor.*` key 逐个回 `crates/editor/src` 查消费）。
任一条按**调用链五段**（定义 → 生产者 → 通道 → 宿主消费 → 落盘/回放）核对，缺一段才算真遗留。

边界：本表只记**代码侧调用链缺段**。gpui-kit 0.9.0 + gpui-fast 合并后需要**人工目视**验收的 8 项
（判的是"画面有没有"，自动化观察层读不到）记在 `../ui/ui-todo.md` §五，不进本表；`rds-editor --lib`
的两条已知失败用例（`legacy.rs:58`、`notebook_win.rs:347`）见 `../ui/gpui-kit-0.9.0-upgrade-report.md` §12。

### 15.1 真遗留（调用链缺段，带证据）

| 优先级 | 项 | 缺在哪一段 | 证据 |
| --- | --- | --- | --- |
| **高** | **折叠候选的后台路径不吃 `editor.fold_open_keywords` / `fold_close_keywords`** | 生产者：`Job` 无关键字字段，`run()` 写死默认表 | ✅ **本轮已修，见 15.4**。原证据：`support/scan.rs:88-97`（`Job{revision,text,channel,scope,catalog,dialect}`）、`support/scan.rs:236-242`（硬编码 `CASE,BEGIN,IF,LOOP,WHILE,FOR,REPEAT` / `END`）；消费在 `view/host/exec/pump.rs:98-105`（`fold::refresh` 覆盖候选）。前台正确口径：`view/host/render.rs:49-55`（设置→shared）+ `view/host/ops.rs:420-431`（`sync_fold`→`fold::install`）。**原后果**：改设置后首帧正确，编辑一次即被后台候选覆盖回默认关键字（设置静默失效） |
| 高 | 右键菜单无 SQL 执行项 | 宿主消费分支：`menu_entries` 无执行条目 | ✅ **本轮已修**，见 15.5。原证据：`view/editor_menu.rs:63-110`（只有剪切/复制/粘贴/全选/添加书签说明/转到定义/代码操作） |
| ~~中~~ | ~~书签行号漂移不校正~~ | 回放：sidecar 行号**会**随编辑平移（方案 A 已实现，`8234474b`；本轮补端到端用例） | `view/host/bookmarks.rs:149`（`adjust_bookmarks_for_edit`） |
| 中 | 分析模式逐单元补全未装 provider | 装配：cell 编辑器只建实例不装 provider | ✅ **补全已修（15.8）、诊断已修（2026-11-09，见 15.14）**。原证据：`view/host/notebook.rs:44-49`（`EditorState::new(..).language(..)` + `set_value`，无 completion / diagnostics）；`completion::install` 仅两处主编辑器（`view/host/mod.rs:633`、`view/host/session.rs:498`）。逐单元诊断的落点：`view/cell_diagnostics.rs`（新增）+ `ensure_cell_editor` 首帧与 Change 后重建 |
| 中 | 标签方案 B（结构化 `tags`）/ C（全局 Tag System） | 数据层：现只有 A「说明即标签」 | §十二 |
| ~~低~~ | ~~跳转后自动弹说明~~ / ~~hover 300ms 延迟~~ | ✅ **两条都已修（2026-11-08）**：跳转后自动弹说明 = 内核「跳转 pin」（`set_gutter_note_pin`，`jump_to_line` 落地即 pin 给内核），见 §15.10；hover 300ms 延迟 = 内核 `gutter_hover_note_since` 跨帧计时 + `request_animation_frame` 推帧，见 §15.13 | 原证据：`jump_to_line` 不回传行号；`gutter_hovered_row` 当帧计算 |
| 低 | 修饰符高亮未映射（readonly / deprecated） | 渲染：主题侧无对应 token | `view/highlight.rs:217` |
| ~~低~~ | ~~脚本执行超时不可配~~ | ✅ **本轮已修（2026-11-08）**：设置项 `editor.env_timeout_ms`（默认 60000，1000–3600000）+ 超时随 `ScriptRequest::timeout_ms` 进执行线程，见 §15.9 | 原证据：无 `editor.env_timeout`；`ports/editor_env.rs` 只有写死常量 |
| 低 | R 内核单元占位（只有语言标签） | 执行端：无 R 解释器 | `notebook/cell.rs:49` |
| 下期 | P3 钻取关联数据（右键值→查关联表） | 入口：菜单未接 | §一（用户明确下期） |
| 下一版本 | 执行上下文模型（per-context 独立连接/事务/取消） | 整体未开工 | §二 + `../connection/execution-context-model.md` |
| 记录在案 | 高亮 token 自定义 A+B | 配置层：无入口 | §十一 |
| 已排除 | 不可见字符替换字形 | 内核 `RangeDecorationStyle` 无 replacement 字形 | §六（⛔ 不硬做） |
| 已排除 | 三通道 `尚未接入 / 尚未注册外部源` 文案 | 设计如此（宿主没注入端口就如实报不可用） | `exec/channel.rs:59/278`、`view/host/render.rs:555` |

### 15.2 文档漂移（声明仍写「未实现」，调用链已闭合）—— ✅ 已全部修（2026-11-08，逐条处置见 §15.11）

| 位置 | 旧声明 | 实际情况 |
| --- | --- | --- |
| `lsp/mod.rs:19` | 「其余 hover / definition 等 provider 槽位暂未实现」 | 已实现并挂载：`lsp/sql_hover.rs`（`view/host/session.rs:503-506`）、`lsp/sql_definition.rs`（工具栏/右键「转到定义」）、`lsp/sql_code_action.rs`、`lsp/document_color.rs` |
| `view/analysis.rs:9`、`:520` | 「单元正文按纯文本渲染，编辑内核复用属后续」 | 已复用内核：`analysis.rs:273-300` 挂 `Editor::new(&editor)`（定高 + 中文右键菜单）；纯文本只剩 `when_none` 回退分支（`:260-272`，编辑器实例没建出来时） |
| `view/host/notebook.rs:28` | 【遗留 #10】「为一个单元确保内核编辑器实例存在」 | #10 已交付，措辞应改 |
| §五 | insight 未提交阻塞 workbench 整包测试 | `cargo test -p rds-workbench --lib --no-run` exit 0（2.07s） |
| §九 | AGENTS.md 0.8.x 与 vendor 0.7.1 口径未确认 | 全链 0.8.1（`Cargo.toml` / `Cargo.lock` / `vendor`），AGENTS.md 正确。**盘点当日晚间已升 gpui-kit 0.9.0 / gpui-pre 0.3.11**，见 `../ui/gpui-kit-0.9.0-upgrade-report.md` |

### 15.3 优化点（不影响可用性）

| 项 | 观察 | 建议 |
| --- | --- | --- |
| 折叠候选算两遍 | 前台 `sync_fold`（`fold::install`，全文）与后台 scan（`fold::spans`，全文）各算一次；后者每编辑一拍 | **关键字已贯通（15.4 已做）**；"后台成为唯一候选来源、前台 install 只在设置/模式/换文本变化时跑"**未做**（会改候选刷新时机，等折叠区人工验收完再动） |
| 设置项消费面已核 | 17 个 `editor.*` 除 `inline_completion` 外都有代码消费；`default_line_ending` / `default_tab_size` / `default_hard_tabs` 在新文档创建时读（`view/host/mod.rs` 的 `apply_editor_defaults`） | `inline_completion` 是**刻意置灰**（B1 预留），放开 `disabled` 时必须同步接 provider，否则即静默失效 |
| 大文件 | ✅ `ports/editor_exec.rs` **已拆（2026-11-08）**：2058 行 → `ports/editor_exec/` 8 个文件（`mod.rs` 128 / `conn.rs` 83 / `accel_exec.rs` 102 / `federation.rs` 384 / `runner.rs` 741 / `convert.rs` 83 / `history.rs` 45 / `tests.rs` 532），最大块从 2058 降到 741。✅ `quick_open/model.rs` **已拆（2026-11-08）**：1299 行 → `quick_open/model/` 6 个文件（`mod.rs` 26 / `types.rs` 220 / `groups.rs` 241 / `sources.rs` 131 / `scoring.rs` 113 / `tests.rs` 588），最大块从 1299 降到 588。✅ `view/dialogs.rs` **已拆（2026-11-08）**：1303 行 → `view/dialogs/` 9 个文件（`mod.rs` 83 / `confirm.rs` 221 / `analysis_sql.rs` 65 / `value_preview.rs` 77 / `bind.rs` 92 / `connection_picker.rs` 199 / `schema_picker.rs` 210 / `bookmarks.rs` 298 / `picker_tests.rs` 95），最大块从 1303 降到 298。✅ `view/host/mod.rs` **已拆（2026-11-08，实测 1478 行——本节旧记的 1366 已过期）**：1478 → 留在 `mod.rs` 668（模块文档 + 导入 + `PendingExport`/`PendingBind`/`EditMode`/`EditorStatusSnapshot`/`EditorHostEvent`/`EditorHostPanel` 结构定义 + 子模块声明 + 构造以外的 impl 方法 + 尾部 re-export）/ `construct.rs` 645（`EditorHostPanel::new` 那一个 636 行单体，独立 `impl` 块）/ `settings_defaults.rs` 180（15 个「设置 → 行为」读取辅助，全部 `pub(crate)` 并由 `pub(crate) use settings_defaults::*;` 转出，调用点零改动），最大块从 1478 降到 668。**四处大文件全部拆完** | 沿 `view/host/` 已验证的按域拆分惯例切完。`editor_exec/runner.rs`（741）与 `model/tests.rs`（588）已到无痛上限：`impl QueryRunner for EngineQueryRunner` 同一类型只能有一个 trait impl 块，**不能再按方法拆文件**；单文件用例要再降得按域拆 `tests/` 目录——两者都属改结构，不再是无损搬运 |
| 大文件拆分纪律 | 四批拆分实证：目录化后模块路径不变（`ports::editor_exec::attach` / `quick_open::model::Row` / `view::dialogs::open_close_confirm` 等对外签名零改动）；子模块里同名 `mod accel` 会与 `use engine::duckdb::accel` 撞车（E0255），须改名 `accel_exec`；`use` 经 `use super::*` 被后代模块使用**不会**报 unused（`dialogs/mod.rs` 一份统一导入供 8 个子文件共用、`host/mod.rs` 一份供 `construct.rs` / `settings_defaults.rs` 共用，均零 warning）；`view/host/mod.rs` 拆出 `construct.rs` 后仍是同类型多处 `impl EditorHostPanel` 块（合法，`new` 独占一块） | 后续拆包按此八条走：① 不动对外路径（`pub use` 子模块通配，别逐个枚举）；② 子模块名避开外部 crate 同名；③ inherent 方法跨文件调用一律 `pub(crate)`；④ 用例要跨文件访问结构体私有字段时**字段也要 `pub(crate)`**（只提方法不够：读字段报 E0616、直接 struct literal 构造报 E0451，两批才全暴露）；⑤ 批量 `Replace("    X: T,")` 会误伤函数形参——同一名字常在 `fn new(..)`/`open_xxx(..)` 的形参里也出现，改完必须 grep 逐处确认；⑥ 搬走 `impl` 内的一段方法时 **`impl` 头跟着搬、留在原处的部分各自补一行 `impl` 头**（否则 `unexpected closing delimiter`）；⑦ 切片按 1-based 闭区间核对末行是不是 `}`（少取一行就 `unclosed delimiter`）；⑧ 新子文件必须有 `use super::*;`，父模块的导入到不了子模块（缺它一片 `cannot find type` E0425/E0433）。**`view/host/` 还剩七块 500 行以上**（2026-11-09 实测，旧记的 848/782/645 已随 §15.14/§15.15 增补漂移）：`render.rs` 833 / `session.rs` 759 / `notebook.rs` 715 / `toolbar/render.rs` 648 / `construct.rs` 633 / `mod.rs` 632 / `format.rs` 561，都属"单一职责但行数大"，要再降得拆方法体（属改结构，见 §16.2 B4） |
| `view/signature_help.rs:17` | 「DBeaver 式完整目录留给后续」 | 已有 C5 函数手册 + #54 B 目录注入，复核这句是否还成立 |

### 15.4 本轮已修（2026-11-08，15.1 第 1 条）

**折叠关键字贯通后台扫描**（`crates/editor` 内 2 文件）：

- `support/scan.rs`：新增 `FoldKeywords { open, close }`（`defaults()` 与 settings 默认同值；
  `Default` 刻意取内置表而非空表——空表会让扫描一个块候选都不给）；`Job` 增加 `fold_keywords`
  字段；`run()` 删掉硬编码关键字表，改用提交时带进来的值。
- `view/host/exec/scan.rs::submit_scan`：从 `shared.fold_open_keywords()` / `fold_close_keywords()`
  取设置值（与前台 `sync_fold` 同一份 shared，同源），随 `Job` 进扫描线程。
- 新增用例 `the_scan_uses_the_fold_keywords_it_was_given`：同一段 `WHEN … END` 文本，
  默认口径 0 个块候选、自定义口径 1 个 `(0, 2)` 候选，钉住"后台真的吃了设置"。
  **选词注意**：`fold::spans` 只在词法归 `Keyword` 的 token 上判块，`DO` 这类词会落进
  `Identifier`，写这类用例必须用真关键字。

验证：`cargo check -p rds-editor --all-targets -j 1` → 0；`cargo test -p rds-editor --lib -- --test-threads=1`
→ 758 passed / 2 failed / 1 ignored（两条失败即本节开头边界里记的基线）；`cargo test -p rds-workbench --test ui_contract` → 9 passed。

**附带修复**（不在本模块范围，但阻塞 `--all-targets` 编译）：vendor `gpui-base` 的
`text/node.rs` / `test_support.rs` 仍按 `Rc::as_ptr(window.text_system())` 取身份指针，而
gpui-pre 0.3.11 起 `text_system()` 返回 `&Arc<WindowTextSystem>` → E0308，必须改 `Arc::as_ptr`。

**未做（有意）**：前台/后台候选双算的合并（会改候选刷新时机，等折叠区人工验收完再动），见 15.3 第 1 条。

### 15.5 本轮已修（续）：右键菜单执行项（15.1 第 2 条）

**改法**（`crates/editor` 内 3 文件）：

- `view/editor_menu.rs`：新增 `MenuExec`（`Hidden` / `Ready` / `Unavailable`）——内核
  `EditorState` 不知道文档模式，执行项能力位不能从内核读，改由宿主判定后传进纯函数；
  `menu_entries` 在组首插入「执行」（`ExecuteSql`）与「执行全部」（`ExecuteAll`）。
- `view/host/editor_body.rs`：`exec = 能力表允许通信 ? (通道就绪 ? Ready : Unavailable) : Hidden`；
  通道未就绪时**保留项但置灰**（灰的原因由状态栏与执行提示解释，不在菜单里猜）。
- `view/analysis.rs`：分析单元显式传 `Hidden` —— 单元运行走笔记本自己的入口，不接主执行链。

用例 3 条：`sql_document_offers_execute_items_before_edit_items` /
`unready_channel_keeps_execute_items_visible_but_greyed` / `plain_text_editor_has_no_execute_items`。

验证：`cargo check -p rds-editor --all-targets -j 1` → 0；`view::editor_menu` → 8 passed；
`rds-editor --lib` → 761 passed / 2 failed / 1 ignored（仍只有那两条基线失败）。

### 15.6 本轮校准：书签行号校正**早已实现**（15.1 第 3 条原是过时记录）

盘点发现 15.1 第 3 条与 `view/host/bookmarks.rs:61` 注释都写着「行号漂移暂不校正」，但
**`8234474b` 就已经做了方案 A**，调用链完整：

`InputEvent::Change`（`view/host/construct.rs:129`）→ `adjust_bookmarks_for_edit`
（`bookmarks.rs:149`）→ `state.take_last_edit_delta()`（vendor 项目私有 patch，
`vendor/gpui-base/src/input/base/state.rs:3590`）→ `bookmark::adjust_for_edit`
（`support/bookmark.rs:105`，5 条单测）→ `shared.set_bookmarks` + 重绘 + `sync_bookmarks` 回写 sidecar。

本轮做的两件（不改行为）：

1. `bookmarks.rs:61` 注释改成「行号由 `adjust_bookmarks_for_edit` 校正，本函数只投影」——
   原句会让后来的人在 `refresh_bookmark_decorations` 里重算一次平移（那边拿不到 delta）。
2. 新增端到端用例 `view/tests/bookmarks.rs::bookmarks_follow_an_inserted_line`：真实换行插入
   → 书签 3→4、说明文本不变；再删行 → 4→3。原单测只证明算法，这条证明**接线通**。

headless 经验：正文编辑要走 `editor.read(cx).focus_handle(cx)` + `focus` 再 `cx.simulate_input("\n")`；
`simulate_keystrokes("enter")` 和面板自己的 `focus_self` 都到不了内核 `EditorState`（前者没焦点、
后者是异步切 tab），文本不会变——用例里成了「前提不成立」的假失败。

### 15.7 代码编辑器常显书签列（+ 设置可关；用户选定「改内核」方案）

**问题**（用户报「查看数据」打开的编辑器没有书签位置）：书签列宽度由「当前有没有书签」决定，
不是由「这份文档支不支持书签」决定——`element.rs:800` 原为
`let has_column = !gutter_markers.is_empty()`，`:805` 零标记 → `column_width = px(0.)`。
零宽列既不报 hover 也不吞点击（点击落到行号），所以「查看数据」那类**未命名文档**
（`workbench/src/view.rs:491` `OpenRequest::untitled`，`bookmark_path()` 为 `None` →
`load_bookmarks` 早退，`bookmarks.rs:28`）永远零书签 → 永远没有那一列。
普通 `.sql` 只是此前存过书签，才「看起来一直有」。

**决策**：用户明确选「改内核让 code-editor 常显」，并追加「这块可以在设置中关闭」——
原先「我们不动内核」的约束由用户本轮亲自放开。

**内核**（vendor/gpui-base，两文件）：

1. `state.rs`：新增 `gutter_marker_column: bool`（默认 `true`，`Default` 初始化处补一行）+
   `pub fn set_gutter_marker_column(show, cx)`（值不变直接 return，不 notify）+
   `pub(crate) fn shows_gutter_column(&self) -> bool`：
   有标记任何模式都出列；零标记时只有 `gutter_marker_column && matches!(mode, CodeEditor)` 才留空列。
   `set_gutter_markers` 的 doc 原写着「空列表…行号栏也不为标记留宽」，同步改写。
2. `element.rs`：三处判据统一走 `shows_gutter_column()`——`:802` 的 `has_column`、`:1850`
   的 `line_number_width + GUTTER_MARKER_HITBOX_WIDTH`；另把 `:836` 的早退从
   「markers 空就 return」收紧为「markers 空 **且** 列宽 0 才 return」——否则常显列
   没有行映射，空行既不出悬停淡书签也不上报点击，列就成了摆设。

**设置链路**（照 `editor.bookmark_hover_note` 的模板，五处 + 模型）：

| 位置 | 改动 |
| --- | --- |
| `settings/src/model/editor.rs` | 字段 `bookmark_column: bool`（`#[serde(default)]`）+ `default_bookmark_column() -> true` + `Default` |
| `settings/src/registry/entries/editor.rs` | 登记 `editor.bookmark_column`（label「常显书签列」，Bool，默认 true，`Immediate`，`Entry::Page` 即设置页自动出项） |
| `settings/src/registry/mod.rs` | `Slot::EditorBookmarkColumn` + `slot_for` 映射 + `slot_kind` 归 Bool 组 |
| `settings/src/service/mod.rs` | `value_by_key` 读分支 + `apply_by_key` 的 editor 域分支 |
| `settings/src/service/editor.rs` | `apply_slot` 分支 + `bookmark_column(cx)` / `set_bookmark_column(on, cx)`（改 global → `persist` → `refresh_windows`） |

**宿主**（crates/editor，三处）：`host/settings_defaults.rs` 加 `editor_bookmark_column_or_default`（无全局回退
`true`）+ 面板缓存字段 `show_bookmark_column`（**初值取内核默认 `true`**——若初值取设置值，
「设置为关」时缓存与设置相等，render 永不调用内核 setter）；`host/render.rs` 在行号开关同款
同步块里比对后调 `set_gutter_marker_column`。

**语义**：关闭 ≠ 隐藏书签。关闭只是回到「有书签才出列」（`shows_gutter_column` 首行仍按
`!gutter_markers.is_empty()` 放行），已有书签照旧显示，`Ctrl+Shift+B` / `F8` / `Shift+F8`
不受影响。环境代价：代码编辑器行号整体右移一列 `GUTTER_MARKER_HITBOX_WIDTH = px(18.)`
（`element.rs:58`），折叠图标随之右移（`:845` 的 x 由 `line_number_width` 推）。

**验证**：

- `cargo check -p rds-editor --all-targets -j 1` → exit 0（含 vendor / settings 重编）。
- `view::tests::bookmarks` → 4 passed（相机口径不变）。
- `rds-editor --lib -- --test-threads=1` → **761 passed / 3 failed / 1 ignored**。第 3 条是
  `view::tests::results_a::the_result_header_sorts_from_its_arrow`（`editor-result-th-1` 无坐标），
  **来自 `crates/result_panel` 的在途未提交改动**（`grid/fit.rs` / `grid/header.rs` / `ui.rs` 同时被改，
  `header.rs` 把 early-return 分支的 `debug_selector` 挪走）：把本轮内核判据临时回退成原式重跑，
  该条**仍失败**，故与本次改动无关。
- `cargo test -p rds-settings` → 33 passed / 1 failed：`tests::invalid_locale_is_rejected`
  断言「初始 locale = Zh」，但 `SettingsService::init` 会 `load_settings()` 读本机配置，
  本机存的是 `en` → **环境性既有失败**，与本轮无关。

**未覆盖**：列宽属布局层，headless 下没有可断言的接缝（`gutter_markers()` 只反映数据不反映宽度），
常显效果靠人工验收：打开任一 `.sql`（无书签）与「查看数据」文档，行号左侧应有一列空白槽，
悬停出淡书签、点一下即加书签；设置页关掉「常显书签列」后，那列在零书签时消失。

### 15.8 分析模式逐单元补全（15.1 第 4 条已修）

**问题本体是「能力位空转」**：`Capabilities::for_mode(Analysis)` 的 `completion` 一直是 `true`
（`core/model.rs:219`，原注释还写着「1c 预留，尚未装 provider」），`completion_enabled` 因此对
`.rdsnote` 一律回 true——但 provider 只装在两处**主编辑器**（`view/host/construct.rs:66`、
`view/host/session.rs:498`）。笔记模式下主编辑器的正文是单元列表，那个 provider 一辈子没人调用；
单元自己的内核（`view/host/notebook.rs:44-49`）只 `EditorState::new(..).language(..)` + `set_value`。
`choose_completion`（`lsp/host.rs:86`）对 `Analysis` 走 `_` 分支、只认 LSP，所以分析模式连 SQL
目录补全的选择都走不到。

**改法**（`crates/editor` 三文件 + 一处用例）：

| 位置 | 改动 |
| --- | --- |
| `notebook/cell.rs` | `CellLanguage::uses_local_sql_completion()`（只有 `Sql` 为真）+ 单测 `only_sql_cells_use_the_local_sql_completion` |
| `view/host/notebook.rs` | `ensure_cell_editor` 建实例时：`uses_local_sql_completion()` **且** `shared.completion_enabled(&document)` → `completion::install(&mut state, shared, document)`，装在单元自己的内核上 |
| `core/model.rs` | `completion` 字段注释改写：分析模式为真值，由单元语言门控落到单元内核 |
| `view/tests/legacy.rs` | 新用例 `sql_cells_of_a_note_get_the_completion_provider`：SQL 单元 `lsp_mut().completion_provider.is_some()`、文本单元 `None` |

**为什么按单元语言门控**：本地目录补全给的是表名 / 列名候选，Python / R / Julia 单元吃这一位
就是满屏错候选。它们的补全要各自的语言服务会话，而笔记模式还没给单元建会话（一条 URI 一个会话，
单元没有自己的路径）——**如实不装**，不拿 SQL 候选冒充。

**验证**：

- `cargo check -p rds-editor --all-targets -j 1` → exit 0
- `cargo test -p rds-editor --lib notebook::cell` → 11 passed（含新单测）
- `view::tests::legacy::sql_cells_of_a_note_get_the_completion_provider` → 1 passed
  —— 顺带证明 `completion_enabled` 对 `.rdsnote` 真的回 true（分析文档可编辑、非大文件档位）
- `rds-editor --lib -- --test-threads=1` → **763 passed / 3 failed / 1 ignored**（比 15.5 的 761
  多出的 2 条即本轮新增）。三条失败全部是既有基线：
  `legacy::opening_an_analysis_document_builds_a_cell_editor_per_cell`（渲染树找不到
  `editor-nb-cell-source-c1`，见 `../ui/gpui-kit-0.9.0-upgrade-report.md` §12）、
  `notebook_win::analysis_cell_renders_multiple_output_blocks`（同上 §12）、
  `results_a::the_result_header_sorts_from_its_arrow`（`editor-result-th-1` 无坐标）。
  第三条上轮归因于 `result_panel` 在途改动，**那笔改动已由他人提交（`a24d9c18`，改
  `grid/header.rs` / `ui.rs` / `delegate.rs`）且失败照旧**——与本轮四个文件零交集（本轮只碰
  `notebook/` / `view/host/notebook.rs` / `view/tests/legacy.rs` / `core/model.rs` 注释）。

**本轮未做（如实记录）**：

1. ~~**逐单元语义诊断（diagnostics）**~~：15.1 原文把 completion 与 diagnostics 写在同一格。诊断要
   每单元独立的 SQL 解析上下文与 catalog 位置映射，`sql_diagnostics` 目前只装在主编辑器上；
   未做。→ **已做（2026-11-09），见 §15.14**
2. **Python / R / Julia 单元的补全**：需要 per-cell 语言服务会话（虚拟 URI），是一条独立改造。
3. `choose_completion` 对 `Analysis` 仍可能把 LSP provider 装到主 state —— 笔记模式下没人调用，
   无害但无用；改它会动"唯一开关处"的语义，留到与 per-cell LSP 一起做时收口。

### 15.9 脚本执行超时可配（15.1 第 5 条已修）

**问题**：`ports/editor_env.rs` 只有写死的 `SCRIPT_TIMEOUT_MS = 60_000`，`run_script` 直接把常量
塞进 `run_script_process`——用户改不了，长跑脚本（训练、批量导出）一律被 60 秒杀掉。原注释也自认
「超时可配列为后续项」。

**改法：配置在主线程读出、随请求进执行线程**（不是让工作线程读设置——它拿不到 `App`）。

| 层 | 位置 | 改动 |
| --- | --- | --- |
| 模型 | `settings/src/model/editor.rs` | `env_timeout_ms: i64`（`#[serde(default = "default_env_timeout_ms")]` = `60000`）+ `Default` |
| 登记 | `settings/src/registry/entries/editor.rs` | `editor.env_timeout_ms`（Number，组 `exec`/「执行」，presets 10 秒 / 30 秒 / 60 秒（默认）/ 5 分钟，`Immediate`） |
| 槽位 | `settings/src/registry/mod.rs` | `Slot::EditorEnvTimeoutMs` + `slot_for` + `slot_kind = Number` |
| 读写 | `settings/src/service/{mod.rs,editor.rs}` | `value_by_key` / `apply_by_key` 分发 + `apply_slot`（范围 1000–3600000）+ `env_timeout_ms(cx)` / `set_env_timeout_ms`（`clamp` 兜底，不调 `refresh_windows`——不牵渲染） |
| 请求 | `editor/src/exec/env.rs` | `ScriptRequest` 加 `timeout_ms: u64`（必填：逼每处调用方想清楚） |
| 生产构造 | `editor/src/view/host/env.rs::run_script` | 提交前读设置（设置服务未装载时回退 `SCRIPT_TIMEOUT_MS`，不取 0）→ 随请求传入 |
| 消费 | `editor/src/ports/editor_exec/runner.rs::run_script` | `request.timeout_ms` 取代常量（文件位置：2026-11-08 目录化后由 `ports/editor_exec.rs` 移入 `runner.rs`） |
| 常量 | `editor/src/ports/editor_env.rs` | 注释改为「兜底」：只服务设置未装载的宿主，值与设置默认同值（两处同改） |

**为什么不在执行线程里读设置**：后台线程不该碰 `App`（`Settings` 是 `App` 全局）。设置值必须在
提交边界由主线程快照后传入——这与 §15.4「折叠关键字贯通后台扫描」同一条纪律。

**验证**：

- `cargo test -p rds-settings --lib` → **34 passed / 0 failed**（含 `every_model_leaf_is_registered`、
  `registered_defaults_match_the_model`、`page_rows_are_scalar_and_writable`、
  `number_presets_are_consistent`、`groups_are_contiguous_and_consistently_named` 五项契约）
- `cargo check -p rds-editor --all-targets -j 1` → exit 0
- `rds-editor --lib -- --test-threads=1` → **763 passed / 3 failed / 1 ignored**（与 §15.8 基线逐条一致）
- 新增真机用例 `script_timeout_comes_from_the_request_not_the_constant`（`workbench/tests/editor_exec_real.rs`）
  → 1 passed：脚本 `time.sleep(3)`、请求只给 800ms → `timed_out == true`、`stdout` 不含 `done`、总耗时
  1.17s。**这是区分性证明**：若执行器仍读 60 秒常量，脚本会正常跑完（`timed_out == false`）而失败。
- `real_python_script_runs_through_the_editor` 回归 → 1 passed（原链路未被改坏）
- `cargo test -p rds-workbench --test ui_contract` → **9 passed / 0 failed**
- `cargo build -p rds-app` → exit 0

**边界与未做**：

1. 只覆盖**本地脚本**（Python / R / Rustc）。数据库查询的超时走连接默认值 / 取消通道，不归此项。
2. 不提供「0 = 不限」语义：范围硬约束 1 秒–1 小时。低到 1 秒以下会把正常解释器启动误杀，
   高于 1 小时等同没超时——两档都不做成可选项，避免"配了等于没配"。
3. `SCRIPT_TIMEOUT_MS` 与设置默认值是**两处同值**（`60000`），注释里已写死这条约束；
   改一处必须改另一处，否则「用户没配过」时前后台口径又会分叉。

**顺带修复（上轮遗漏，本轮门禁照出来）**：`crates/workbench/tests/ui_contract.rs::every_panel_module_is_registered_in_the_manifests`
红——`src/panels/overview.rs`（上一轮工作台概览卡片新增）没登记进尺寸 / 颜色两份契约清单，
等于这两份契约对该文件静默失效。已在两处清单各补一条登记；补后 `ui_contract` 9 passed
（同时证明 overview.rs 无裸色值、无裸尺寸字面量）。

### 15.10 跳转后自动弹书签说明（15.1 第 3 条已修一半）

**问题本体**：内核的 gutter 说明气泡只在「指针停在书签列上」时画——
`vendor/gpui-base/src/input/base/element.rs::layout_gutter_markers` 里
`gutter_hovered_row(&layout.marker_hitbox, ..)` 用当帧 `window.mouse_position()` 算命中行，
命中且该行 `note` 非空才 shape 出气泡。而 `jump_to_line` 只 `set_cursor_position`，
内核不知道"刚跳到哪一行"——于是从书签管理器双击跳转、或 `F8` / `Shift+F8` 循环跳转后，
用户看不到自己刚落到的那条说明写了什么，必须把鼠标移到行号栏再悬停一次。

**改法：给内核加一个「跳转 pin」（应用设定、指针一动即让位）**。三处，约 30 行：

| 层 | 位置 | 改动 |
| --- | --- | --- |
| 内核状态 | `vendor/gpui-base/src/input/base/state.rs` | 新字段 `gutter_note_pin: Option<usize>`（buffer 0 基）+ `set_gutter_note_pin(Option<usize>)` + `gutter_note_pin()` 读取器；`set_gutter_markers` 内容变化时清 pin（标记变了，pin 的行可能已无说明） |
| 内核渲染 | `vendor/gpui-base/src/input/base/element.rs::layout_gutter_markers` | `note_row = gutter_hovered_row(..) 的结果.or_else(pin 在该帧可视行里的位置)`；气泡的 y / 文本查找都改用 `note_row`，绘制不变 |
| 让位 | 同上 `state.rs::on_mouse_move` | 函数**开头**（任何早退分支之前）`if self.gutter_note_pin.take().is_some() { cx.notify() }`——鼠标一动就交给 hover 逻辑，两份说明不争同一行位置 |
| 应用 | `crates/editor/src/view/host/bookmarks.rs::jump_to_line` | 算出"该行是否有说明"（判据与 gutter 标记一致：`label != 行号`）→ `set_cursor_position` 之后 `set_gutter_note_pin(line_1 - 1)` |

**覆盖的两个入口**（都汇到 `jump_to_line`，实测确认）：
`open_bookmark_manager` 的 `on_jump` 回调（`view/host/bookmarks.rs:368`，双击列表行）、
`jump_to_bookmark`（`F8` / `Shift+F8`，同文件 `:243`）。

**为什么不给 `jump_to_line` 加返回值**：原证据写的是「不回传行号」，但真正的缺口不在返回值——
调用方本来就知道自己要跳的行。缺的是「内核拿这个行没有用来显示说明」。所以修法是把行交给内核
（pin），而不是把行回传给一个已经有行号的人。

**尊重既有设置**：`editor.bookmark_hover_note` 关闭时 `push_gutter_markers` 下发空 `note`，
内核判据要求 `!note.is_empty()`（与 hover 同一处），所以 pin 也只画书签不弹说明——两处口径一致。

**验证**：

- `cargo check -p rds-editor --all-targets -j 1` → exit 0（vendor 改动联动重编，27s）
- 新用例 `view::tests::bookmarks::jumping_to_a_bookmarked_line_pins_its_note` → ok：
  第 3 行带说明、第 2 行纯书签 → 跳 3 行 pin = `Some(2)`；再跳 2 行 pin = `None`（无说明不 pin）
- `bookmarks_follow_an_inserted_line` 回归 → ok（标记刷新清 pin 的路径没打断编辑平移）
- `rds-editor --lib -- --test-threads=1` → **764 passed / 3 failed / 1 ignored**
  （= §15.9 基线 763 + 本轮新增 1 条；三条失败仍是既有基线
  `legacy::opening_an_analysis_document_builds_a_cell_editor_per_cell`、
  `notebook_win::analysis_cell_renders_multiple_output_blocks`、
  `results_a::the_result_header_sorts_from_its_arrow`）

**同一条后半：hover 300ms 延迟 —— 已做（2026-11-08，§15.13）**。原判「代价不同、留作
独立一项」的两点都已落地：跨帧记忆放进了 state（`gutter_hover_note_since`），到点那一帧靠
`window.request_animation_frame()` 推进。**内核单测跑不了的老结论已被推翻**——vendor 不在
workspace members 但可以复制成独立副本跑（口径见 §15.13）。

### 15.11 文档漂移逐条处置（2026-11-08，闭合 §15.2 五条 + 顺手两条）

判定方法：每一条都先回到**当前源码**确认「声明」与「实际」是否还差，差的才改——
不照抄 §15.2 的旧描述（那份表本身是上一轮写的，也可能已经跟着漂）。

| # | 位置 | 旧声明 | 处置 |
| --- | --- | --- | --- |
| 1 | `crates/editor/src/lsp/mod.rs` 模块文档 | 「其余 hover / definition 等 provider 槽位暂未实现」 | **已改**。所有槽位都在挂载：completion / hover / definition / showDocument 经 `lsp/bridge.rs`（`install_completion` `:309`、`install_hover` `:322`、`install_definition` `:337`、`install_show_document` `:355`），`document_color`（`:141`）、`sql_code_action`（`:189`，槽位可多挂）、`semantic_tokens`（`view/highlight.rs:301`）是 SQL 自研不经 LSP。新文写清「hover 是单槽、LSP 转发与 SQL 自研二选一」，避免下次又被读成"没实现" |
| 2 | `crates/editor/src/view/analysis.rs` 模块文档 | 「单元正文按纯文本渲染，编辑内核复用属后续」 | **已改**。正文是逐单元内核编辑器：`analysis.rs:63` 的 `cell_editors: &HashMap<String, Entity<EditorState>>`，`:273` `when_some(cell_editor, ..)` 渲染 `Editor`，`:260` `when_none` 才是纯文本退化。新文明确「纯文本不是默认形态」 |
| 3 | 同上 `cell_body_text` 文档注释（原 `:522`） | 「本切片按纯文本渲染；编辑内核复用属后续」 | **已改**为「只在面板还没建出该单元内核实例时用的退化内容」，并指向正常路径 `cell_card` |
| 4 | `crates/editor/src/view/host/notebook.rs::ensure_cell_editor` | `【遗留 #10】` | **已改**：编号删掉（#10 已交付，函数内已用「【15.1 第 4 条】」指向 backlog 真实条目），只留函数语义 |
| 5 | §五（本文）origin/main 行 | 「✅ 已推送（2026-10-28）：44 个 commit 同步（0 领先 0 落后）」 | **已改**：实测 `git rev-list --left-right --count origin/main...main` = `0	71`，`origin/main` 停在 `9ce3c98e`。改为「本地领先 71、未推送」并注明用户口径（只做本地合并） |
| 6 | §九（本文）首条 | 「全链 0.8.1、gpui-pre 0.3.10，AGENTS.md 正确」 | **已改**为 0.9.0 / gpui-pre 0.3.11 口径（依据 `../ui/gpui-kit-0.9.0-upgrade-report.md` §2 的 vendor / lock 实测） |
| 7 | `AGENTS.md` L5 | 「GPUI-kit 0.8.x」 | **已改**为「当前 vendor 为 `0.9.0`，即 `GPUI-kit 0.9.x`」。**未动**L7 的「全部源码（0.7.1/0.7.2/0.7.3/0.8.0）已本地索引」——索引覆盖属 MCP 工具侧事实，仓库内无从证实，不猜 |

**没有改的东西**（避免下次重复判断）：`view/signature_help.rs:17`「DBeaver 式完整目录留给后续」
仍属 §15.3 的待复核项（C5 函数手册 + #54 目录注入是否已覆盖该措辞，未定论）；`§九` 其余
未决项（执行上下文模型 / analyze 无列类型 / 函数手册 URL / `annotate_types` Unknown）复核后
**仍然成立**，原样保留。

验证：`cargo check -p rds-editor --all-targets -j 1` → exit 0（只动注释与文档，无行为变化）；
`rds-editor --lib` 不重跑（本轮无代码语义改动，上一次全量即 §15.10 的 764/3/1）。

### 15.12 `view/host/mod.rs` 拆分（§15.3 大文件第 4 批，也是最后一批）

**拆前实测 1478 行**（§15.3 旧记的 1366 是更早的盘点值，已过期——这也是本轮先量再切的原因）。

切法与边界（按顶层结构切，不动任何方法体）：

| 文件 | 行数 | 内容 |
| --- | --- | --- |
| `mod.rs` | 668 | 模块文档 + 导出的两段导入（原 `:9-28` 与 `:203-227` 合到一处）+ `mod construct;` / `mod settings_defaults;` / `pub(crate) use settings_defaults::*;` + `PendingExport` / `PendingBind`(+`impl`) / `EditMode`(+`impl`) / `EditorStatusSnapshot` / `EditorHostEvent` / `impl EventEmitter` / `EditorHostPanel` 结构定义 + 10 个子模块声明 + `apply_editor_defaults` 起的一段 impl 方法 + 尾部 `ops` / `render` re-export |
| `construct.rs` | 645 | `impl EditorHostPanel { pub fn new(..) }`——原 `:591-1226` 的 **636 行单体构造函数**整段搬家，独占一个 `impl` 块 |
| `settings_defaults.rs` | 180 | 原 `:30-201` 的 15 个「设置 → 行为」读取辅助（`editor_settings_or_default` / `editor_show_line_numbers_or_default` / `editor_fold_*` / `fact_storage_pref_or_default` / `active_fact_snapshot` / `active_set_index` 等），一律 `pub(crate)` |

为什么这么切：`new` 里的装配有严格顺序（建内核 → 装 provider → 订阅 → 恢复会话 → 应用设置默认），
拆成小函数会让"谁在谁之前"看不出，所以整段搬；设置辅助是纯读取、无状态，独立成文件最干净。
`settings_defaults.rs` 的名字刻意避开 `settings`（外部 crate 同名，E0255 风险）。

**零改动边界**：`mod.rs` 仍是 `view::host` 的模块根，`pub use ops::{…}` 与 `pub(crate) use render::*;`
原样保留，`view/mod.rs`、`workbench` 侧调用点一行没改；10 个子模块的 `use super::*` 继续吃到
`mod.rs` 那段落统一导入（与 `dialogs/mod.rs` 同一模式），**零 unused warning**。

**三轮编译错误的成因**（写下来防下次重犯）：
1. `unexpected closing delimiter`（`mod.rs:655`）——把 `impl EditorHostPanel {`（原 `:586`）连同 `new`
   一起搬给了 `construct.rs`，留在 `mod.rs` 的那段方法失去 impl 头。补一行 `impl EditorHostPanel {` 即可。
2. `settings_defaults.rs:177` `unclosed delimiter`——切片取了 `:30-201`，而 `active_set_index` 的收尾 `}`
   在 `:202`。按 1-based 闭区间重取 `30..202`。
3. 一片 `cannot find type App` / `EditorShared` / `SettingsService`（E0425/E0433）+ `unused import:
   settings::SettingsService`——新文件忘了 `use super::*;`，父模块的导入到不了子模块。

验证（全部实跑）：

- `cargo check -p rds-editor --lib -j 1` → exit 0，**零 warning**
- `cargo check -p rds-editor --all-targets -j 1` → exit 0
- `cargo test -p rds-editor --lib -- --test-threads=1` → **764 passed / 3 failed / 1 ignored**，
  与 §15.10 基线逐条一致（三条既有失败仍是 `legacy.rs` / `notebook_win.rs` / `results_a.rs`）——
  这就是"纯结构改动、零行为变化"的运行证据
- `cargo build -j 1 -p rds-app` → exit 0（45.71s）
- 重启应用（新二进制）**pid 23636**：stderr 92 行、`panic=0` / `ERROR=0`

**一次需要解释的噪音（已排除）**：第一次重启（`Stop-Process -Force` 后仅等 800ms 就起新进程）的
日志里出现 6 条 `ERROR … gpui-fast-windows … 无效的窗口句柄 (0x80070578 / 0x80040102)`，
集中在同一秒、之后不再出现。把间隔拉到 2s 再重启一次 → **0 条**。结论：这是强杀上一进程后窗口句柄
尚未回收的启动竞态，落在 `gpui-fast-windows` 依赖里，与本轮改动代码零交集，不是回归。
（顺带记下：重启脚本的 `Start-Sleep` 别压到 1s 以内。）

**没有做的**：没有继续把 `construct.rs` 的 `new` 拆成小函数，也没有动 `render.rs`（848）/ `session.rs`（782）
——那属于改方法结构，不是无损搬家，需要单独决策。

## 15.13 hover 300ms 延迟（2026-11-08，闭合 §15.10 后半）

**语义**（两个入口口径不同，别再混）：

| 来源 | 触发 | 是否等 300ms |
| --- | --- | --- |
| pin | 书签管理器双击「跳转」/ `F8`·`Shift+F8` 落到带说明书签 | **不等**，落地即显示；指针一动即让位 |
| hover | 指针停在行号右侧标记列某行上 | **要等**，同一行停满 300ms 才弹 |

**为什么必须把计时放进 state**：上一版 hover 靠 `GutterMarkerLayout::last_hovered_row`
（`Rc<Cell<Option<usize>>>`）自查"行变没变"，但那个 `Rc` 是**每帧 layout 新建**的，只记得住
"上一帧的行"，记不住"这一行从什么时候开始停"。计时跨帧存活只能落到 state：

- `vendor/gpui-base/src/input/base/state.rs`
  - 字段 `gutter_hover_note_since: Option<(usize, std::time::Instant)>`（紧接 `gutter_note_pin`）
  - `set_gutter_hover_row(row: Option<usize>, cx)`：换行或离开即重置计时；**行没变不 notify**
    （所以每帧重复调用不会自激重绘）
  - `gutter_hover_note_since() -> Option<(usize, Instant)>`（`pub(crate)`）
- `vendor/gpui-base/src/input/base/element.rs`
  - 常量 `GUTTER_NOTE_HOVER_DELAY = Duration::from_millis(300)`（紧接
    `GUTTER_NOTE_POPOVER_MAX_WIDTH`）
  - 纯函数 `gutter_note_hover_ready(since, row, now) -> bool`：行相同且已满 300ms。
    **时间从外部传入**——这是它能被单测覆盖的关键（不必 sleep）
  - `layout_gutter_markers` 拆成 `hovered_now`（指针当前行）与 `pinned_row`（pin 行），
    `note_row = hovered_now.or(pinned_row)`；说明来自 hover 且未满延迟时
    `window.request_animation_frame()` + `return layout`（本帧不画，下一帧继续判断；到点画出后
    不再申请，帧自然停）
  - `paint_gutter_markers` 的鼠标回调改为 `state.update(cx, |s, cx| s.set_gutter_hover_row(row, cx))`，
    `GutterMarkerLayout::last_hovered_row` 字段与 `use std::cell::Cell` 一并删除

**代价**：一次悬停最多多画约 18 帧（60Hz × 0.3s）。停住后不再申请帧，稳态零开销；
划过的行因计时不断重置而始终不弹，正是要的体感。

**vendor 单测怎么跑（推翻了「跑不了」的老结论）**：`gpui-base` 是 `[patch.crates-io]` 的 path 目标，
不是 workspace member，`cargo test -p gpui-base` 会报 `requires dev-dependencies and is not a member
of the workspace`；`--manifest-path vendor/gpui-base/Cargo.toml` 又报 `believes it's in a workspace
when it's not`。可行口径是**复制成独立副本**，两个坑都在这儿：

1. 在副本 `Cargo.toml` 末尾加空的 `[workspace]`，让它脱离父 workspace；
2. 副本里有个 `include_str!("../../../../README.md")`，按相对层级它要落在副本的
   **上上级目录**——把副本放在 `.rds/tmp/gpui-base-probe` 时就在 `.rds/` 放一份占位 `README.md`；
3. 必须带 `--features test-support`（`crate::test_support` 与 `TestAppContext` 都在该 feature 后）。

命令形态（`--target-dir` 必须隔离，别污染主 target）：

```powershell
Copy-Item -LiteralPath vendor\gpui-base -Destination .rds\tmp\gpui-base-probe -Recurse -Force
Add-Content -LiteralPath .rds\tmp\gpui-base-probe\Cargo.toml -Value "`n[workspace]`n"
cargo test --manifest-path .rds\tmp\gpui-base-probe\Cargo.toml --lib -j 1 `
  --target-dir .rds\tmp\gb-target --features test-support -- gutter
```

**验证**：

- 新内核用例 `input::element::tests::gutter_note_shows_only_after_the_pointer_rests_on_the_row` → ok
  （0ms/299ms 不显示、300ms 显示、换行后即使过了 5s 也不显示、无计时起点不显示）
- 同批 gutter 相关内核用例 **11 passed / 0 failed**（探针副本内实测）
- `cargo check -p gpui-base --lib -j 1` → exit 0，零 warning
- `cargo check -p rds-editor --all-targets -j 1` → exit 0
- `cargo test -p rds-editor --lib -- --test-threads=1` → **764 passed / 3 failed / 1 ignored**
  （与 §15.10/§15.12 基线逐条一致，说明内核行为改动未打断任何编辑器用例）
- `cargo build -j 1 -p rds-app` → exit 0（3m58s）

**未覆盖**：延迟本身是体感项，只有人工能判"300ms 是否偏慢/偏快"；自动化只钉住了判定函数与
"换行即重置"的语义。`gpui-base --all-targets` 仍会挂在 bench `text_view_scroll`
（缺 `gpui_fast_platform`），与本轮改动无关，属该 bench 的老问题。

## 15.14 分析模式逐单元诊断（2026-11-09，闭合 §15.8「本轮未做」第 1 条）

**问题**：诊断的三条来源（打字期词法 / LSP / 执行错误）**只在一处重建**——
`view/host/results/errors.rs::apply_error_marks`——而它只 `self.editor.update(..)`（主编辑器）。
笔记模式下主编辑器正文是**单元列表 JSON**：后台扫描（`support/scan.rs::run`）对整篇
`job.text` 调 `language_service::diagnostics`，算出来的是"这段 JSON 的括号/引号"，
单元里那段 SQL 的错永远报不出来。能力表给分析模式 `diagnostics: true`，但这个位一直没落地。

**为什么不能只把 provider 装到单元**：诊断在内核里不是 provider 槽位（`lsp_mut()` 只有
completion / hover / definition 等），而是"调用方算好再推"的集合：
`EditorState::diagnostics_mut() -> Option<&mut DiagnosticSet>`，集合只有 `clear` + `push`。

**改法**（新增 1 个模块 + 1 处接线 + 1 条用例）：

| 位置 | 改动 |
| --- | --- |
| `view/cell_diagnostics.rs`（新增） | ① `diagnostics_for(text, catalog, dialect)` 纯函数，内部就是 `language_service::diagnostics(&Request{text, cursor:0, selection:None, catalog, connection:None, channel:默认, dialect})`——与后台扫描同一填法，不另造判据；② `apply(state, found, cx)`：`clear` 后逐条 `push`，行列用 `Diagnostic::positions(&text)`（0 基），来源「语法」、严重度 Error、`code` 原样带给内核——与主编辑器词法层同一口径 |
| `view/host/notebook.rs::ensure_cell_editor` | ① 建实例时取一次目录快照（`shared.catalog_snapshot_for(&document)`）与方言（`self.sql_dialect_for_document()`），与 `submit_scan` 同源；② `cx.new` 里 `set_value` 之后立刻 `apply`（**首帧就有波浪线**，不必等敲键）；③ Change 订阅里重算，**经 `cx.defer`** 写回内核 |
| `view/tests/legacy.rs` | 新用例 `cell_diagnostics_are_per_cell_and_rebuild_on_typing`：好 SQL 单元 0 条、未闭合字符串单元 ≥1 条、文本单元 0 条；再点进好 SQL 单元敲 `/*` → 该单元自己出现诊断，另一个单元不受影响 |

**为什么必须 `cx.defer`**：Change 事件由单元编辑器**自己**发出，此刻它正在 `update` 中；
在订阅回调里再 `update` 同一实体会 panic（`cannot update … while it is already being updated`，
与 gpui-kit-dev「不要在回调里重入 update 当前实体」同一条）。延迟到本次更新结束再写，
订阅里只做"算"。

**门控**：只有 `CellLanguage::uses_local_sql_completion()`（即 SQL 单元）且
`completion_enabled(&document)` 的单元接诊断。Python / R / Julia 单元不接——它们的诊断要
per-cell 语言服务会话，与补全同一笔欠账（§15.8 未做 2）。

**验证**：

- `cargo check -p rds-editor --lib -j 1` → exit 0，零 warning
- 定向 `cell_diagnostics_are_per_cell_and_rebuild_on_typing` → 1 passed
- `cargo check -p rds-editor --all-targets -j 1` → exit 0
- `cargo test -p rds-editor --lib -- --test-threads=1` → **765 passed / 3 failed / 1 ignored**
  （= §15.13 基线 764 + 本轮 1 条；三条失败仍是既有基线 `legacy.rs:58` / `notebook_win.rs:347` /
  `results_a.rs:290`）
- `cargo build -j 1 -p rds-app` → exit 0；重启应用后日志 0 panic / 0 ERROR

**未覆盖（如实记录）**：

1. ~~**执行错误没有进单元诊断**~~：单元跑出错误时，错误信息仍以「单元输出块」呈现（
   `source「执行」` 那条只喂主编辑器与结果面板）。要把"这个单元的 SQL 在第 N 行出错"也画成
   波浪线，需要执行回执里带上 per-cell 错误位点，属执行端改造。→ **已做（2026-11-09），见 §15.15**
2. ~~**目录快照与方言是创建时快照**~~：用户切连接/切 Schema 后，已存在的单元仍按旧快照算语义诊断
   （词法层不受影响）。重建单元实例（增删单元、重开文档）即刷新；持续跟随要订阅连接变更，
   属另一条接线。→ **已随 §15.15 顺带修**：重建入口改成每次现取
   （`catalog_snapshot_for` + `sql_dialect_for_document`），切连接后**下一次敲键或执行**即用新快照，
   不必重建单元实例；仍不订阅连接变更（连接一变就主动重画是另一条接线）
3. 延迟/抖动未测：每次 Change 同步算一次是纯 CPU（单元文本远小于 `SCAN_MAX_BYTES`），
   但没有为"超大单元"设闸；如出现卡顿，可照 `scan_scope` 的做法按档位关掉。

## 15.15 单元执行错误进单元诊断（2026-11-09，闭合 §15.14 未覆盖第 1、2 条）

**问题**：单元执行失败时错误只以「输出块」呈现，编辑区里没有任何标记——用户要在结果卡片里
读一句驱动原文，再自己数第几行。主编辑器有这套（`error_site` + `apply_error_marks`），
单元没有：那是主编辑器坐标系，路径不同。另外单元诊断的目录快照/方言原本是**创建时快照**，
切连接后已存在的单元仍按旧快照算语义诊断。

**为什么必须做成"一条重建流程"**：内核的诊断集合只有 `clear` + `push`（不是 provider 槽位）。
词法诊断与执行错误要是各写一遍，后写的必然擦掉先写的——重跑成功旧错误还挂着，或一敲键
执行错误就消失。所以执行错误存成面板字段一条"记忆"，与词法诊断在同一个入口合并重建。

**改法**：

| 位置 | 改动 |
| --- | --- |
| `view/cell_diagnostics.rs` | 新增 `ExecMark { site, reason }`（位点 + 驱动原文）；`apply` 增第三参 `exec`：词法（来源「语法」）与执行错误（来源「执行」）在两趟 push 里合并，一次 `clear` 管全部 |
| `view/host/mod.rs` + `construct.rs` | 面板新增 `cell_error_marks: HashMap<String, ExecMark>`（键 = 单元 id；位点按单元存，因为单元正文与主编辑器正文是两套坐标系） |
| `view/host/notebook.rs::ensure_cell_editor` | 变化回调不再自带 catalog/dialect 闭包，改为 `cx.defer` 回面板 `rebuild_cell_diagnostics`（单元 id + weak 句柄）；建实例后立刻调一次（首帧就带波浪线） |
| `notebook.rs::rebuild_cell_diagnostics`（新增） | **唯一的重建入口**：现取 units 正文 + 目录快照 + 方言 → 算词法 → 合并 `cell_error_marks` → 写内核。每次重建都现取快照，因此切连接后下一次敲键/执行即刷新（不再依赖"重建单元实例"） |
| `notebook.rs::drain_cell_notes` | 回执到达时：判据与 `apply_cell_note` 一致（会话一致 + 单元仍在 Running）才认；失败→写 mark，成功或提交层失败→清 mark；然后重建该单元 |
| `notebook.rs::notebook_run_cell` | 提交成功即清该单元旧 mark 并重建（新结果没回来之前不该还挂着旧波浪线） |
| `notebook.rs::cell_error_mark_for_note`（新增） | 从回执取位点：`site_in_document(当前单元正文, 这次提交的 SQL, 错误原文)`——与主编辑器同一判据；正文在提交后被改过就认不出，**不给位置**（只在输出块里报原因，不猜） |
| `view/tests/legacy.rs` | 新用例 `a_failing_cell_run_marks_its_own_cell_and_a_successful_rerun_clears_it`：假执行器按 SQL 内容回"带 `at position N` 的解析错误"或成功 |

**验证**：

- `cargo check -p rds-editor --lib -j 1` → exit 0，零 warning
- 定向用例 → 1 passed
- `cargo check -p rds-editor --all-targets -j 1` → exit 0
- `cargo test -p rds-editor --lib -- --test-threads=1` → **766 passed / 3 failed / 1 ignored**
  （= §15.14 基线 765 + 本轮 1 条；三条失败仍是 `legacy.rs:58` / `notebook_win.rs:347` /
  `results_a.rs:290`）
- `cargo build -j 1 -p rds-app` → exit 0；重启后日志 0 panic / 0 ERROR

**未覆盖（如实记录）**：

1. **只覆盖 SQL 单元**：Python / R / Julia 的内核错误（`KernelEvent::Error`）仍是输出块，
   它们的位点要先有 per-cell 语言服务会话（§15.8 未做 2 / §15.14 未覆盖）。
2. **不订阅连接变更**：切连接后语义诊断要"下一次敲键或执行"才按新快照刷新；要做到连接一变
   就主动重画，需订阅连接事件（另一条接线）。
3. 结果卡片的「定位」按钮仍只作用于主编辑器路径（`jump_to_error_site`）；单元的诊断目前
   只画波浪线、不做跳转（单元里位置本来就在眼前，跳转需求未出现）。

## 15.16 §16.1 的 A4 / A2 落地（2026-11-09）

按 §16.1 里 assistant 自排的顺序（A4 → A2 → A1）推进，本条收口前两项。A1 需要先定会话成本
口径（每单元一份 server 会话 vs 每文档一份会话 + 虚拟 URI 多文档），不擅自开工。

### A4 删 `save_cell_update` 死代码

**问题**：`ports/result_service.rs` 的 `save_cell_update`（原 `:105-158`）拼单条 `UPDATE … WHERE`
逐格改；T2a 批量提交（`CommitPending` → `submit_pending_changes`）落地后全仓零调用方，只留一条
注释自述"遗留的单格更新路径"——读代码的人会误以为单格编辑还走这条路。

**改法**：删方法体与 `:144` 那句注释；模块文档首行清单里的"单元格回写与结果导出"改成
"结果导出"，并补一段口径：**单格回写不在这里**，要恢复单格编辑请接 T2a 批量提交链，不要再拼
单条 `UPDATE`。

**验证**：`cargo check -p rds-editor --lib -j 1` → exit 0（7.80s），零 warning；全仓
`grep save_cell_update` → 0 命中。

### A2 目录快照代际 → 切连接不必等敲键

**问题**（§15.15 未覆盖 2）：`rebuild_cell_diagnostics` 每次重建都现取目录快照，但**触发点只有
"单元正文变化"和"执行回执"**。切连接 / 切 Schema 后，宿主泵把新目录推给 `EditorShared`，已有
单元的语义波浪线却要等用户下一次敲键或执行才按新目录算。

**为什么是代际计数而不是订阅**：`EditorShared` 是 `Rc` 句柄，没有"文档激活 / 绑连"事件流（绑定
发生在 editor 侧、跨 crate 不推事件），加一套订阅是架构级改动。而宿主泵
（`ports/editor_catalog.rs::pump`）本来就每 1 秒推一拍快照，**"快照真的变了"这个事实只有 shared
自己知道**——那就让它记一个代际，面板在既有的 render 权威同步点上比对（与行号/书签列/软换行/
自动闭合等十余个设置同步点完全同款），零内核改动、零跨 crate 订阅。

**改法**：

| 位置 | 改动 |
| --- | --- |
| `shared/mod.rs` | 新字段 `catalog_revision: Rc<Cell<u64>>`（紧邻 `catalog_snapshots`；Clone / `new` 各一行） |
| `shared/env_lsp.rs::set_catalog_snapshot` | 用 `Arc::ptr_eq` 判"内容真变化"才 +1——宿主泵 1s 一拍推的是同一份 `Arc`，相等即不涨（否则面板每秒整批重建一遍） |
| `shared/env_lsp.rs::catalog_revision`（新增） | 读代际 |
| `shared/env_lsp.rs::retain_catalog_snapshots` | 只在实际删了条目（关文档）时才 +1 |
| `view/host/mod.rs` + `construct.rs` | 面板缓存字段 `catalog_revision: u64`（最近一次比对过的代际） |
| `view/host/render.rs` | 权威同步点：代际不等 → 更新缓存 → `cx.defer` 重建（**不在 render 里直接写内核**，同单元 Change 回调那条理由） |
| `view/host/notebook.rs::rebuild_all_cell_diagnostics`（新增） | 遍历 `cell_editors` 逐个走同一个 `rebuild_cell_diagnostics`——三源合并只在一处，不另写批量逻辑；非笔记模式 `cell_editors` 为空 → 安全空转 |
| `view/tests/legacy.rs` | 新用例 `a_new_catalog_snapshot_refreshes_cell_diagnostics_without_typing`：目录先含 `t_old`（c1 干净）→ 换成只含 `t_new` 的一份 → **不敲键**，只渲一帧 → c1 出现 `unknown_table` 诊断，c2 仍干净 |
| `shared/mod.rs`（单测） | 新用例 `catalog_revision_only_moves_when_the_snapshot_really_changes`：同一份 `Arc` 重复推不涨 / 换目录涨 / 清条目涨 / 空推不涨 / `retain` 没真删不涨 |

**区分性**：旧实现在这条用例上必红——不敲键就没有任何重建触发点。实测把 render 里那段同步点
临时改成 `if false && …`，该用例立刻红（0 passed / 1 failed），恢复后绿。

**验证**：

- `cargo check -p rds-editor --lib -j 1` → exit 0，零 warning；`--all-targets -j 1` → exit 0（42.77s）
- 定向：`catalog_revision_only_moves_when_the_snapshot_really_changes` → 1 passed；
  `a_new_catalog_snapshot_refreshes_cell_diagnostics_without_typing` → 1 passed
- `cargo test -p rds-editor --lib -- --test-threads=1` → **768 passed / 3 failed / 1 ignored**
  （= §15.15 基线 766 + 本轮 2 条；三条失败仍是 `legacy.rs:58` / `notebook_win.rs:347` /
  `results_a.rs:290` 那三条固定失败）

**未覆盖（如实记录）**：

1. **重建发生在"面板下一次重绘"**：render 是权威同步点，所以快照变了而面板一帧都不重绘时不会
   自发重建。真实路径里"切连接"的那次点击本身就会 `cx.notify()`（`notebook_switch_connection`
   末尾），切 Tab / 任何交互也同样触发，所以实际感知是"点完就刷"。1s 泵的静默推快照本身不产生
   重绘——这是本条与"订阅式即时刷新"的差别，选择它是为了零跨 crate 订阅（见上文理由）。
2. **只覆盖"目录快照变了"**：`sql_dialect_for_document` 的方言变化若不同时伴随目录变化（同一连接
   换方言这类极端组合），仍要等下一次敲键。把方言纳入代际是同一模式的第二条计数，等真出现再补。
3. **主编辑器不在本条范围**：主编辑器语义诊断走 `submit_scan`，文本不变就不重扫；切连接后主编辑器
   的未知表波浪线同样要等敲键。要同一口径需把代际也接到 `submit_scan` 的触发条件上（另一条接线）。
4. **1 秒粒度**：宿主泵一拍 1s，所以"切完连接"到"快照到达 shared"最坏 1s（与语义诊断本身晚一拍
   同一哲学）。


## 16. 遗留总账（2026-11-09 全量盘点）

**盘点口径**：文档声明（§一 ~ §15）+ 代码内 `TODO|FIXME|未接线|暂不支持|暂不|遗留|待接线`
全扫（`crates/editor/src` 命中 45 处，其中 39 处是【遗留 #N】历史编号注释、对应能力都已交付，
只剩措辞）+ 设置项消费核对 + 各文件行数实测。逐条按**调用链五段**（类型定义 → 生产者 →
通道/事件 → 宿主消费分支 → 落盘/回放）核对：整链闭合的不进本表（记进 §16.7），缺一段才算。

**本表按"谁能拍板"分档**，与旧文档的"按优先级"不同——因为剩下的项里，卡在代码上的
（§16.1）和卡在决定上的（§16.2）不该混在同一张优先级表里排。

### 16.1 可继续推进（代码侧，无需产品/成本决定）

| # | 项 | 证据 | 现在的影响 | 处置 |
| --- | --- | --- | --- | --- |
| A1 | **per-cell 语言服务会话**（分析模式的单元补全/诊断） | `ports/editor_lsp.rs:25-27`「v1 只服务 Python（`.py`）；分析模式（`.rdsnote`）暂不服务；Markdown 留后」；主编辑器装 provider 只在 `view/host/mod.rs` / `session.rs` 两处 | Python / R / Julia 单元没有补全与诊断；`choose_completion` 对 `Analysis` 仍可能把 LSP provider 装到主 state（笔记模式无人调用，无害但无用） | 一条改动同时收口三笔账：§15.8 未做 2、§15.14 未覆盖（语言面）、§15.15 未覆盖 1。需虚拟 URI + 每单元一份会话，**架构级，但不需要产品决定**——只要接受"每单元一个 server 会话"的资源成本 |
| A2 | **连接变更未订阅** | §15.15 未覆盖 2；`rebuild_cell_diagnostics` 每次现取快照，但触发点是敲键/执行 | 切连接 / 切 Schema 后，已有单元的语义诊断要下一次敲键或执行才按新快照刷新 | **已修（§15.16，2026-11-09）**：不订阅，改「目录快照代际 + render 权威同步点」——同一份 `Arc` 重复推不涨，真换目录才涨，面板重绘时整批重建 |
| A3 | **折叠候选双算未合并** | §15.3 第 1 条：前台 `sync_fold`（`fold::install`，全文）与后台 scan（`fold::spans`，全文）各算一次，后者每编辑一拍 | 纯 CPU 重复，未见卡顿；合并会改候选刷新时机（后台成为唯一来源），属行为变更 | **有意推迟**，等折叠区人工验收（`ui-todo.md` V-6/V-7）确认后再动 |
| A4 | `save_cell_update` 死代码 | `ports/result_service.rs:105`，全仓 `grep save_cell_update` 仅 1 命中（定义本身），零调用方；注释自述「遗留的单格更新路径（无调用方；T2a 批量提交已取代它）」 | 无运行时影响；留着会误导"单格更新还走这条路" | **已删（§15.16，2026-11-09）**；模块文档改为"单格回写不在这里，要恢复请接 T2a 批量链" |
| A5 | `inline_completion` 置灰（B1 预留） | §八 B1：设置项 `disabled` + 默认关 + hint「能力已预留」；内核 `CompletionProvider::inline_completion()` 默认返回空 | 用户看到开关但点不动（刻意的） | **放开 `disabled` 时必须同步接 provider**，否则从"刻意的灰"变成"静默失效" |

### 16.2 需你拍板（产品口径 / 成本量级 / 架构取舍）

| # | 项 | 证据 | 卡在哪 | 需要你定什么 |
| --- | --- | --- | --- | --- |
| B1 | 标签方案 B（结构化 `tags: Vec<String>`）/ C（全局 Tag System） | §十二：现只有 A「说明即标签」（`#tag` 约定，schema / sidecar 零变更） | 数据层：B 要 schema + sidecar 迁移；C 是远期系统 | 走 B 还是维持 A？C 是否进路线图 |
| B2 | 修饰符高亮（readonly / deprecated） | `view/highlight.rs:217`（`token_modifiers: Vec::new()` + 注释「暂不用：主题侧也未映射」） | 三处同时缺：内核 `decode_semantic_tokens` 不消费 `token_modifiers_bitset`；主题无 modifier token；SQL 词法没把 readonly/deprecated 判为独立 type | 值不值得改 vendor 内核 + 扩主题？**先得定"SQL 里哪些 token 算 readonly/deprecated"**（现在没有这个概念） |
| B3 | R 内核单元（只有语言标签） | `notebook/cell.rs:49` | 执行端：无 R 解释器接线 | 要不要真做？与 Python 内核（已通）成本量级不同 |
| B4 | `view/host/` 继续降到 500 行内 | 实测（2026-11-09）：`render.rs` 833 / `session.rs` 759 / `notebook.rs` 715 / `toolbar/render.rs` 648 / `construct.rs` 633 / `mod.rs` 632 / `format.rs` 561 | 都是"单一职责但行数大"，再降必须拆方法体（改结构，不是无损搬运） | 接受到哪一档？还是维持现状（拆分纪律已证明无损搬运的空间已用尽） |
| B5 | 高亮 token 自定义 A+B | §十一：三层全硬编码，无配置入口；内核 `HighlightStyleResolver` 支持包一层用户覆盖 resolver（可行） | 配置层：无入口 | 是否立项（scope 级匹配已明确不做） |
| B6 | 分析笔记的环境维度不可改 | `view/host/notebook.rs:380`「环境维度暂不在此菜单改（分析环境跟随通用模式解释器）」 | 菜单只有连接/会话级动作 | 分析笔记要不要独立环境选择（= 又一个 per-cell/每笔记的执行上下文问题） |

### 16.3 已排期（口径已定，不重复讨论）

| 项 | 口径 | 证据 |
| --- | --- | --- |
| P3 钻取关联数据（右键值 → 查关联表） | 用户明确「下期」；`ColumnDetail.references` 已就位，取数链路现成 | §一 P3 |
| 执行上下文模型（per-context 独立连接 / 事务 / 取消） | 讨论定稿未开工，**0.2.4 不实现**；方案 A/B/C 与待拍板项见 `../connection/execution-context-model.md` | §二 |

### 16.4 已知边界（记录在案，别当遗留重复立项）

| 项 | 证据 | 为什么不是遗留 |
| --- | --- | --- |
| analyze 路径结果无列类型 / 注释 | `ResultSet` 无类型字段 | P1 已按「无注释」如实处理（§四） |
| `explain_sql` 部分方言返回 None → 「暂不支持生成执行计划」 | `view/host/exec/tx.rs:45-53` | 方言能力边界，不是没接线：能出计划的方言已出 |
| 函数手册的官方文档 URL | workbench 按「引擎 + 函数名」生成 | 需真实环境验证后定口径（§九） |
| hover 补列类型 | `annotate_types` 多数表达式返回 `Unknown` | 依赖 sqlglot 语义层，暂不做（§九） |
| 补全候选被截断时界面未标注 | `language/completion.rs:93`（字段有、界面没摆） | 现有候选表远未触发上限；留着给状态栏/诊断用 |
| 尾随注释在格式化后丢失 | §十 边界 | sqlglot 生成器行为，已记录 |
| 未闭合字符串时签名帮助可能误弹 | §八 A 方案边界 | 补上闭合引号即正确 |
| 不可见字符「替换符号」渲染 | §六：内核 `RangeDecorationStyle` 仅 `Fill`/`Frame`，无 replacement 字形 | ⛔ 不硬做，等内核能力 |
| 单元诊断只画波浪线、不做跳转 | §15.15 未覆盖 3 | 位置就在单元里眼前，跳转需求未出现 |
| 书签交互无自动化断言（双击/编辑/删除、hover 像素） | §十二：headless 下对话框按钮点击分发不可靠 | 已知测试设施限制，靠人工验收 |
| 大文件无痛上限 | `ports/editor_exec/runner.rs` 741、`quick_open/model/tests.rs` 588 | 同一类型只能有一个 trait impl 块；再降只能改结构 |

### 16.5 测试与验收基线（现状，不是待办）

| 项 | 现状 |
| --- | --- |
| `cargo test -p rds-editor --lib -- --test-threads=1` | **768 passed / 3 failed / 1 ignored**（2026-11-09 §15.16 后）。三条固定失败：`view/tests/legacy.rs:58`（渲染树找不到 `editor-nb-cell-source-c1`）、`view/tests/notebook_win.rs:347`（多输出块）、`view/tests/results_a.rs:290`（表头排序箭头）——归因见 `../ui/gpui-kit-0.9.0-upgrade-report.md` §12（观察层口径，非产品缺失） |
| `ui-todo.md` §五 V-1 … V-8 | **8 项全 ⬜**（V-1 对话框关闭残影 / V-2 右 Dock 草稿行 / V-3 导航树虚拟行 / V-4 列类型警示行 / V-5 标题栏折叠还原 / V-6 书签图标在 gutter 最左 / V-7 有说明折叠行箭头常显+横线 / V-8 悬停说明气泡）。判的是"画面有没有"，自动化观察层读不到，只能人工目视 |
| `cargo check --workspace` | 不通过：`windows` crate `STATUS_STACK_BUFFER_OVERRUN`（环境限制，只用 `-p` 单 crate） |
| `cargo check -p gpui-base --all-targets` | 必失败：bench `text_view_scroll` 缺 `gpui_fast_platform`（只用 `--lib`） |

### 16.6 工程与协作

| 项 | 现状（2026-11-09 实测） | 处置 |
| --- | --- | --- |
| origin/main 未推送 | `git rev-list --left-right --count origin/main...main` = `0	106`（本地领先 **106** 个提交，`origin/main` 停在 `9ce3c98e`）。旧记录「71」已过期 | 用户口径：只做本地合并，推送另行安排 |
| 外部线在途 | `git status --short` 除本模块外有三条线：① insight 线 `docs/architecture/insight/prototypes/eda/eda.html`（M）+ 6 个未跟踪（`insight-correlation-l1-spec.md`、`col-bool/col-date/col-num/col-text/eda-by-type.html`）；② 导航树线 `crates/database/src/nav_view/{rows,chrome,nav_view,tests}.rs`（M）；③ 结果面板统计线 `crates/result_panel/src/{ports,grid/stats,panel/data,panel/lifecycle}.rs`（M，只加了 `StatsItem` 模型与字段、消费端未接，曾一度拦住 `rds-editor` 编译） | 一律不碰；被他们拦门禁时等其改完再重跑，不改别人的文件 |
| 结果面板遗留 | 维护在 `result-panel-handoff.md` §2.4，本模块不重复；剩余缺口是**渲染期尚未消费事实档** | 跨 crate，归结果面板线 |

### 16.7 本轮（2026-11-08 ~ 11-09）已闭合 12 项，别重复立项

| 项 | 落点 |
| --- | --- |
| 折叠关键字贯通后台扫描 | §15.4（原 §15.1 第 1 条） |
| 右键菜单 SQL 执行项 | §15.5（原 §15.1 第 2 条） |
| 书签行号漂移校正（记录校准） | §15.6 + §十二（原 §15.1 第 3 条本是过时记录） |
| 文档漂移 7 处注释 | §15.11（原 §15.2） |
| 四处大文件拆分（`editor_exec` / `quick_open` / `dialogs` / `host/mod`） | §15.3 + §15.12 |
| 跳转后自动弹书签说明（内核跳转 pin） | §15.10 |
| hover 300ms 延迟 | §15.13 |
| 脚本执行超时可配 | §15.9（原 §15.1 第 5 条） |
| 分析模式逐单元补全 | §15.8（原 §15.1 第 4 条） |
| 逐单元诊断（词法/语义） | §15.14 |
| 单元执行错误进单元诊断 + 快照每次现取 | §15.15（闭合 §15.14 未覆盖 1、2） |
| `save_cell_update` 死代码删除（§16.1 A4） | §15.16 |
| 目录快照代际 → 切连接不必等敲键（§16.1 A2） | §15.16（闭合 §15.15 未覆盖 2） |


