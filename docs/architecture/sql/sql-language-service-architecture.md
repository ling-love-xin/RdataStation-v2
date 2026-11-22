# SQL 语言服务架构（语义走 sqlglot，排版走 lax）

> 落点：`crates/sql`（`rds-sql`）。本文回答两件事：**这个 crate 是谁的唯一接入点**，以及
> **排版为什么从 `sqlglot-rust::generate_pretty` 换成 `lax-sql`**、换完之后有哪些能力与边界。

---

## 1. 定位：SQL 语言服务唯一接入点

`crates/sql` 是全仓**唯一**允许接触 SQL 语法/文本的 crate。上层（editor / settings / app /
result-panel）不得直接依赖 `sqlglot-rust` 或 `lax-sql`。

```text
                  app / workbench
                        │
              editor ───┼─── settings
                        │
                   crates/sql          ← 唯一接入点
              ┌─────────┴─────────┐
        sqlglot-rust          lax-sql
        （语义半边）           （排版半边）
```

职责分工：

| 半边 | 引擎 | 承担 | 不承担 |
| --- | --- | --- | --- |
| **语义** | `sqlglot-rust =0.11.1` | 解析 / 语句切分 / 词法高亮 / 关键字判定 / 作用域与血缘 / 转译 / 方言体检 / 参数绑定 | 出文本（`generate_pretty` 已退出排版路径） |
| **排版** | `lax-sql =0.3.0` | 空白：换行、缩进、行宽、子句布局、换行符 | 语义、大小写（见 §5.1）、方言 |

依赖方向只向下：`editor → settings`（不是反向），`crates/sql` 不依赖任何 UI crate。

---

## 2. 为什么换掉 `generate_pretty`

`generate_pretty` 是**生成器**（AST → 文本），不是格式化器。它在真实 SQL 上有三个硬伤，
其中前两条是**实测**（探针留在 `crates/sql/tests/sqlglot_capabilities.rs` 的 #9）：

1. **注释只有"语句前"位置能活**。行内块注释（`select a, /* c */ b`）、尾随行注释
   （`select a from t -- c`）、关键字之间（`select /* c */ a`）、语句末尾的注释，**都会让
   整条语句解析失败**；旧实现解析失败 → 原样返回 → 用户按了格式化"没反应"。
2. **"解析失败就整条不动"**。这条对**选区格式化**是致命的：用户选中
   `where x = 1 AND b = 2` 这种半截片段，sqlglot 必然 `Err`，于是**选区格式化从来没真正生效过**
   —— 框架（`crates/editor/src/support/format.rs::plan` 的选区分支）早就在，只是被解析门槛卡死。
3. **生成器会重写文本**。它按目标方言出引号风格、把 MySQL `#` 注释改成 `--`。这是"改写语义
   表面"而不是"调空白"，与"格式化只该动空白"的期望不符。

`lax-sql` 是 dprint 生态的 **lossless formatter**：只调整空白，**永不拒绝输入**。以上三条
一次性消掉，且第二条直接解锁选区片段格式化。

---

## 3. 引擎事实（一手核验）

| 项 | 值 |
| --- | --- |
| crate | `lax-sql =0.3.0`（**crate 名 ≠ 仓库名**，仓库是 `github.com/bartlomieju/lax`） |
| 许可 | MIT |
| 底层 | `dprint-core =0.67.4`，只用其 `formatting` 默认 feature；`async_runtime` / `communication` / `process` 全在 optional，**不引入 tokio** |
| 入口 API | `lax_sql::format_text(_path: &Path, text: &str, config: &Configuration) -> anyhow::Result<Option<String>>` |
| `_path` 参数 | 实现里是 `_path`，**完全不使用**；传 `Path::new("statement.sql")` 即可，无文件系统交互 |
| 返回值语义 | `Ok(Some(s))` = 排版结果；`Ok(None)` = **输出与输入完全一致**；`Err` = 内部异常（当前不发生） |
| 配置类型 | `lax_sql::configuration::{Configuration, ClauseStyle, KeywordCase, resolve_config}` 全部 `pub` |
| 已知坑 | `Configuration.new_line_kind` 的类型是 `dprint_core::configuration::NewLineKind`，`lax_sql` 只 private `use` 未 `pub use` → **`crates/sql` 必须直接依赖 `dprint-core`** 才能构造配置 |
| 性能（上游 README，Apple M 系列） | SQL corpus `~38 MB/s`，对照 `sqlformat-rs ~9 MB/s` |
| 源码规模 | `lax-sql` 13 个 `.rs` / 约 40 KB（printer 7.2 KB、parser 6.8 KB、configuration 4.3 KB、tokenizer 7.5 KB） |

本质与 `lax-core`：`tokenize → parse →` 用 `dprint_core::formatting::format` 打印
（`PrintOptions{ indent_width, max_width: line_width, use_tabs, new_line_text }`）。

---

## 4. 数据流

```text
编辑器（Ctrl+Shift+F / 保存时）
   │  editor_snapshot() → (text, Option<selection>)
   ▼
crates/editor/src/support/format.rs::plan(text, selection, dialect, options)
   │   ├─ 有选区：clamp_range → expand_to_statements（吸附到完整语句）
   │   │            → 切 slice → format_report(slice) → 拼回三段
   │   └─ 无选区：format_report(text)
   ▼
crates/sql/src/engine.rs::format_report
   ▼
crates/sql/src/formatter.rs::format_with_report
   │  1. effective_newline(sql, options.new_line_kind)   ← 换行符先解析成具体值
   │  2. rewrite_statements_with_gap_nl(sql, blank_lines, newline, |body| …)
   │        ├─ split_statements 拿每条语句字节区间
   │        ├─ gap（注释/空行/分号）→ normalize_gap（只含空白+分号才规整）
   │        └─ body → format_single
   │              ├─ lax_config(options, newline)
   │              ├─ lax_sql::format_text(…)
   │              ├─ 空串防御（纯注释片段不得被清空）
   │              └─ apply_options（关键字大小写 → 前导逗号迁移）
   │  3. same_content_signature 保真校验，不一致整篇回退原文
   ▼
FormatReport { text, formatted, kept_verbatim }
   ▼
apply_rewrite（replace_all，保留撤销历史）+ 恢复选区/光标 + 状态栏回执
```

要点：

- **语句为单位**回填，区间外的内容（注释、空行、分隔符）由 gap 规整处理，一条排不好不牵连整篇。
- **换行符在 `formatter` 层先落地**。lax 自己的 `NewLineKind::Auto` 看的是"单条语句体最后的
  换行"，而语句体通常单行 → 恒选 LF；如果 gap 又按原文输出 CRLF，就会得到**混排文件**。
  所以 `effective_newline` 按**整篇多数换行**判定后，同时喂给 lax（语句体内）与 gap（语句之间）。
  实测：`"select 1;\r\nselect 2;"` → `"select\r\n  1;\r\n\r\nselect\r\n  2;"`，无裸 LF。
- **不补尾分号**。分号属于原文（`split_statements` 把分号留在 gap 里），格式化不改写用户没写的东西。

---

## 5. 设置项 × lax 映射

`crates/settings/src/model/sql_format.rs::SqlFormat`（12 项）→ `crates/editor/src/support/format.rs::options_from_settings`
→ `crates/sql/src/formatter.rs::FormatOptions` → `lax_config()`（7 项真进 lax `Configuration`）；
另 5 项不进 lax：`keyword_case`（sqlglot 词法器）、`comma_style` / `normalize_operators`（本地后处理）、
`blank_lines_between`（gap 规整）、`format_on_save`（UI 层）。

| 设置 key | 落盘值 | `FormatOptions` | lax `Configuration` | 默认 |
| --- | --- | --- | --- | --- |
| `sql_format.line_width` | 40–200 | `line_width: u16` | `lineWidth` | 120（= dprint 推荐值） |
| `sql_format.clause_style` | `fill` / `expanded` | `ClauseStyle` | `clauseStyle` | `expanded` |
| `sql_format.indent_width` | 2–8 | `indent_width: u8` | `indentWidth` | 2 |
| `sql_format.indent_char` | `space` / `tab` | `IndentChar` | `useTabs` | `space` |
| `sql_format.new_line_kind` | `auto` / `lf` / `crlf` | `NewLineKind` | `newLineKind` | `auto` |
| `sql_format.ignore_node_comment_text` | 文本（非空，≤64，无换行） | `String` | `ignoreNodeCommentText` | `dprint-ignore` |
| `sql_format.ignore_file_comment_text` | 同上 | `String` | `ignoreFileCommentText` | `dprint-ignore-file` |
| `sql_format.comma_style` | `trailing` / `leading` | `CommaStyle` | **无**（本项目行级后处理） | `trailing` |
| `sql_format.blank_lines_between` | 0–2 | `u8` | **无**（gap 规整负责，与 lax 不冲突） | 1 |
| `sql_format.keyword_case` | `preserve` / `upper` / `lower` | `KeywordCase` | **固定 `Preserve`**（见 §5.1） | `preserve` |
| `sql_format.normalize_operators` | bool | `bool` | **无**（本地 token 后处理，见 §5.3） | `false`（默认关） |
| `sql_format.format_on_save` | bool | —（UI 层） | — | `false` |

空白值防护：`ignore_*` 为空会 `contains("")` 命中任何注释（等于"全篇不排版"），所以
**设置层拒绝空值**（`service/sql_format.rs::apply_slot`），映射层再兜一次"手工改坏 → 回退默认"。

### 5.1 例外一：`keyword_case` 不交给 lax（词表差一个数量级）

lax 的关键字判定是 `generation/keywords.rs` 的 `pub const KEYWORDS: &[&str]`，**85 个通用词**，
`is_keyword()` 硬引用，**无注入点**。sqlglot 的 `TokenType` 是 **200+ 变体的跨方言并集**。

所以大小写走既有路径：`crates/sql/src/highlight.rs` 的 `Tokenizer::with_comments(sql)`
（不接受方言）产出 span → `TokenClass::Keyword` 区间做**同长 ASCII 改写**（从后往前，跳过
String/Comment）。实测 `select id, 'select' as s from t` + upper →
`SELECT\n  id,\n  'select' AS s\nFROM\n  t`：字符串里的 `select` 不动，`as` 改，标识符不动。

### 5.2 排版不按方言

lax 的 tokenizer 方言无关（`Tokenizer::with_comments(sql)` 也不接受方言）。排版是纯空白决策，
与 `LIMIT` / `TOP` 这类语法无关。`SqlEngine::format(sql, dialect)` 的 `dialect` 参数保留（调用方
契约不变），但**不参与排版**。连带结果：`# 注释` 不再被改写成 `--`，十个方言排出来一字不差
（`sqlglot_capabilities.rs` #9 有断言）。

方言仍然必要——只是用在**转译**与**语义分析**（`transpiler.rs` 要求源方言严格可判定）。

### 5.3 例外二：`normalize_operators` 不是 lax 能力，是本项目后处理

`x=1` → `x = 1` 这类 token 间距归一是 lax **明确不做**的事（它只调结构性空白），
所以这一项从一开始就不是"释放 lax 能力"，而是本地新增：

- **白名单 13 个**（`SPACING_OPERATORS`）：`= <> != <= >= < > + - * / % ||`。
  形态全部按 2026-11-22 `probe_operator_tokens` 实测确定：`<>` `!=` `<=` `>=` `||` `::` `->` `->>`
  `#>` `#>>` 各是**一个** token，`@@version` 是**两个** `@`。因此"惯例不空格"的 `::` / `->` / `@`
  不进白名单，天然不动。
- **一元 / 通配判据只看前一个 token**（`is_unary_or_wildcard`）：无前 token（行首）/ 关键字 /
  运算符 / 标点 `(` `[` `,` `.` → 视为一元或通配。实测：`count(*)` 的 `*` 前是 `(`、
  `select *` 的 `*` 前是 Keyword、`t.*` 的 `*` 前是 `.`、`a=-1` 的 `-` 前是 Operator。
  该判据只对 `- + *` 生效；其余白名单项恒为双目。
- **只补不删**：`a  =  1` 保持原样（不重排用户手工对齐）；`select a\n=1` 补成 `select a\n= 1`
  （换行是空白，不替换成空格）。
- **执行位置**：`apply_options` 第 3 步（在关键字大小写与逗号迁移之后，因此**重新取 spans**），
  从后往前插入字符，原始偏移不失效。默认关——它会改变用户字符，一元判定依赖上下文。

---

## 6. 保真机制（三道）

用户的 SQL 是真实数据，排版引擎再可信也不裸用：

1. **空串防御**：`lax` 的 `format_text_inner` 有 `if statements.is_empty() { return Ok(String::new()) }`
   分支。若输入非空而输出被清成空串，返回原文（防"纯注释片段被吞"）。当前实测 `-- 只有注释`
   原样返回，未触发，但防线保留。
2. **内容签名比对**：`comment_mark_count`（`--` / `/*` / `#`）+ `comma_mark_count`（`,`）+
   `commas_in_line_comments`（落在行注释里的逗号数）。三者与原文不一致 → **整篇回退原文**，
   报 `formatted = 0`。
3. **为什么需要第 3 个计数**（实测边角）：`select a -- 注释\n, b from t` 交给 lax 会排成
   `a -- 注释,` —— 逗号被行注释吞掉，**总逗号数一点没变**，只有"行注释里的逗号数"变了。
   这是唯一能识别"SQL 已变坏"的廉价判据。

签名的比较对象是**整篇原文 vs 最终产物**（`format_with_report` 里 `same_content_signature(sql, &text)`），
而 `apply_options` 的后处理（含运算符补空格）发生在 `format_single` 内部、早于这一步 ——
所以后处理不会绕过防线（补空格不改逗号、不改注释、不改行注释内逗号数，恒通过）。

### 6.1 上游缺陷规避：CRLF 行注释的 `\r` 会逐轮累加

用 85 个真实迁移文件对拍时暴露的**大面积非幂等**（修复前 `default` 68 / `ops-on` 68 / `all-on` 67，共 85 个），
根因不在本项目管线：

- 直连 `lax`（绕开 `formatter`）复现：`-- a\r\nselect 1\r\nfrom t` + `CarriageReturnLineFeed` 配置
  → `-- a\r\r\nselect\r\n  1…`，**每轮多一个 `\r`**；同一输入**去掉注释**、或换成**块注释**都正常。
  → `lax 0.3.0` 的行注释把行尾 `\r` 当成注释文本的一部分，再按配置追加 `\r\n`。
- sqlglot 的 span 形态给了修复判据：`"-- a\r\nselect 1"` 的行注释 span 是 `0..4`（`"-- a"`），
  **不含 `\r`**；块注释 span 则含内部 `\r\n`。

修复 `strip_line_comment_cr(body)`：只在 `TokenClass::Comment` 且**不以 `/*` 开头**的 span 上判断，
若紧跟该 span 之后的字节是 `\r`（说明这一行是 CRLF 结尾，而 lax 会把这个 `\r` 当成注释内容），
就删掉这个 `\r`（从后往前删，偏移不失效）。
它跑在 `format_single` 里、交给 lax **之前**——修的是 lax 的输入，不是它的输出。
词法失败时 `highlight_spans` 为空 → 函数原样返回，不误改。

实测：`default` 的 `idem_fail` 从 68 降到 **0**（85 文件），`all-on` 从 67 降到 1。

### 6.2 不动点迭代（后处理会改长度，第二轮的 lax 可能再折行）

`normalize_operators` 补空格会让行变长。`tokenize = 'trigram'` 本来就贴着 `line_width`，
补一个空格就超宽 → **第二轮**的 lax 再折一次行（`tokenize =\n  'trigram'`），于是第二轮 ≠ 第一轮。
`\r` 修完之后 `ops-on` 仍剩 **1** 个（`011_fts_content_and_trigram.sql`）就是它，`default` 反证为 0。

所以 `format_single` 在末段加**不动点迭代**：`lax → apply_options` 最多再来 2 轮，
仅当 `options.affects_text()`（后处理真的会改文本）且 `out != formatted` 时触发，两轮一致即停。

**只在 `comma_style == Trailing` 下迭代**：首版无条件迭代把 `all-on` 的非幂等从 1 恶化到 35 ——
`leading` 与 lax 冲突（lax 把行首逗号拉回行尾，后处理又挪到行首，每轮重新迁移）。收窄后
85 文件的最终数据是 `default` 0 / `ops-on` 0 / `all-on` 1（剩下的 1 个就是 `leading` 固有摆动，见 §7 #9）。

---

## 7. 已知差距（全部实测，不是推测）

| # | 现象 | 原因 | 影响 | 处置 |
| --- | --- | --- | --- | --- |
| 1 | `x=1` 默认仍是 `x=1`，不会自动变 `x = 1` | lax 只调**结构性空白**（换行/缩进），不规范化 token 之间间距 | 默认观感与 DataGrip 有差 | **已实现可选开关** `sql_format.normalize_operators`（默认关，只补不删，见 §5.3） |
| 2 | `select case when` 挤在一行 | lax 的 CASE 规则把 `case when` 当子句起始 | 长 CASE 可读性一般 | 暂不做 |
| 3 | `x = (select y from u)` 子查询内联不展开 | lax 不展开括号内子查询 | 对比 DataGrip 不够"重" | 暂不做（展开需 AST 级决策，成本高） |
| 4 | 前导逗号 + 行尾注释 → **整篇回退原文** | 见 §6 第 3 项 | 该篇"按了没反应" | 可接受（不产出坏 SQL 优先） |
| 5 | 无效/半截输入也被重排（`NOT VALID SQL` → `NOT\n  VALID SQL`） | lax 永不拒绝输入 | 正在输入的半截语句会被动 | 见 §9 建议 4 |
| 6 | `dprint-ignore` 写在**行尾**不生效；写在**独立行**生效 | 节点忽略匹配的是"忽略该节点的注释"，注释必须是该节点的**前导**注释；行尾注释是后置注释 | 用户以为写了就跳过 | 已实测三种写法（见 §7.2），用法写进提示即可 |
| 7 | **lax 没有选区 / range API**，整篇是唯一入口 | `lax-sql/src/lib.rs` 只 `pub use format_text`，`format_text(path, text, config)` 无 range 参数；内部 `Statement.span` 在私有模块 `generation` 里，外面拿不到；`dprint-core` 的 "range" 只用于诊断高亮（`formatting/utils/string_utils.rs::format_diagnostic`） | 选区语义只能由应用层承担 | 已由 `plan()` 切 slice 实现，并补 `expand_to_statements` 吸附（见 §4 / 下表） |
| 8 | 词法失败的文本，`apply_options` 的三步后处理**全部静默失效** | 后处理依赖 `highlight_spans`（sqlglot 词法器）；词法失败时它返回**空 span**。实测 `select !a, ~a from t` 一个 span 都没有 → 关键字大小写 / 逗号迁移 / 运算符补空格都不生效 | 这类文本仍会被 lax 排版，只是后处理不参与（不会报错、也不会产出坏 SQL） | 记录在案；词法失败属极端输入，静默降级优于误改（见 §9 建议 7） |
| 9 | `comma_style: leading` + 折行时，**幂等不成立** | `leading` 要求逗号在行首，lax 折行时会把行首逗号拉回行尾；每次格式化都是重新迁移一次 | 85 个真实迁移里有 1 个（`connection_metadata/005_normalized_fts_and_cascade.sql`）在两轮之间摆动 | 不动点迭代**只在 `Trailing` 下执行**（见 §6.2）；设置页 hint 已注明"前导不承诺幂等"，不另加运行时拦截 |

另外记一条**语义漂移**：`FormatReport.kept_verbatim` 已从"解析失败、逐字保留"变成
"引擎异常兜底"，正常输入恒 0。界面文案已同步（`format_document` 不再说"解析不了"），
但字段名本身没改（改动面涉及 `FormatPlan` / UI / 测试，留待后续统一）。

### 7.1 选区怎么处理（lax 不管，我们管）

| 环节 | 做什么 | 不做什么 |
| --- | --- | --- |
| `clamp_range` | 只把越界偏移钳回 `0..len`，保证不 panic | **不做**语义对齐——它可以把选区切在字符串 / 注释 / 半个 token 中间 |
| `expand_to_statements` | 与选区**相交**的语句首尾相接（跨两条就两条一起排）；用 `split_statements` 的区间，保证落在字符边界 | 选区只落在语句之间的空白 / 纯注释区时**不吸**（那是用户真的只选了空白） |
| 切 slice + 回填 | `text[..start] + formatted + text[end..]` | 选区之外一个字节都不动 |

注意一个由 `split_statements` 语义带来的**已知行为**：语句前的注释算该语句的一部分，所以吸附后
选区首行是那条注释（注释文本不变，只是被纳入选区）。要排除注释需要额外词法判断，当前不做。

### 7.2 `dprint-ignore` 三种写法实测（2026-11-22）

| 输入 | 输出 | 结论 |
| --- | --- | --- |
| `select    1; -- dprint-ignore` | `select\n  1; -- dprint-ignore` | **行尾不生效**（后置注释不匹配） |
| `-- dprint-ignore\nselect    1;` | `-- dprint-ignore\nselect    1;` | **独立行生效**，原样返回 |
| `/* dprint-ignore */\nselect    1;` | 原样返回 | 块注释独立行同样生效 |
| `-- dprint-ignore\nselect    1;\nselect 2;` | 第一条原样，第二条正常排版 | 只作用于**紧跟的第一条**语句 |

所以正确用法唯一条：**忽略标记独占一行，紧贴要保护的那条语句上方**。这条已写进设置页
「节点忽略标记」的 hint（`crates/settings/src/registry/entries/sql_format.rs`）。

---

## 8. 验证

```powershell
# 排版 crate：单元 + 集成（含 sqlglot 能力台账 #9）
cargo test -p rds-sql -j 2

# 编辑器：格式化链路（plan / 选区 / 光标映射 / 视图回执）
cargo test -p rds-editor --lib format -j 2

# 设置：登记表 / 持久化 / 迁移
cargo test -p rds-settings --lib -j 2

# 看排版真实输出（探针，换 lax 版本时先跑它眼睛对一遍）
cargo test -p rds-sql --lib formatter::tests::probe_lax_output -j 2 -- --nocapture

# 看运算符 token 形态（改白名单 / 一元判据前先跑）
cargo test -p rds-sql --lib formatter::tests::probe_operator_tokens -j 2 -- --nocapture

# 真实语料对拍（85 个迁移文件 × default / ops-on / all-on，查幂等 + 保真签名）
cargo test -p rds-sql --lib formatter::tests::probe_real_corpus -j 2 -- --nocapture

# 直连 lax 的 CRLF 组合（上游缺陷回归用：8 组注释形态 × LF/CRLF 配置）
cargo test -p rds-sql --lib formatter::tests::probe_lax_crlf -j 2 -- --nocapture
```

本轮基线（2026-11-22 第四轮实测）：`rds-sql` lib **232** + 集成 `sqlglot_capabilities` **10** = **242** 通过；
`rds-editor --lib format` **33** 通过；`rds-settings` lib **47** 通过；`cargo check -p rds-sql -p rds-settings -p rds-editor` 零 error / 零 warning；
`rustfmt --edition 2024 --check crates/sql/src/formatter.rs` clean。
（`rds-editor` 整包未复跑，本轮只动 `crates/sql` 与 settings 的 hint 文案，format 链路 33 条已覆盖改动面。）

**真实语料对拍结果**（`crates/store/migrations` 85 个手写 DDL 迁移，全部 CRLF + 中文注释）：

| options | formatted | fallback | `idem_fail` | `sig_fail` |
| --- | --- | --- | --- | --- |
| default | 81 | 4 | **0** | 0 |
| ops-on（`normalize_operators`） | 81 | 4 | **0** | 0 |
| all-on（大写 + leading + 80 宽 + 补空格） | 80 | 5 | **1**（`connection_metadata/005`，`leading` 固有摆动） | 0 |

（表内 `005` = `connection_metadata/005_normalized_fts_and_cascade.sql`，
`011` = `connection_metadata/011_fts_content_and_trigram.sql`。）

修复前的对照（同一探针、同 85 文件）：`default` 68 / `ops-on` 68 / `all-on` 67（`\r` 累加覆盖全部）；
只修 `\r` 之后：`default` 0 / `ops-on` **1**（`011`）/ `all-on` 1；
加无条件不动点迭代后：`ops-on` 0，但 `all-on` 被 `leading` 抬到 **35**；
再收窄到 `Trailing` 才回到 `all-on` 1（`005`）。四步演进就是两处缺陷 + 一处自身回归的定位过程。
`fallback` 是"保真签名不一致 → 整篇回退原文"，属设计内行为，不是失败。

第三轮基线（2026-11-22 第三轮实测）：`rds-sql` lib **226** 通过（其中 formatter **25**）+ 集成 10 通过；
`rds-editor` lib **832** 通过 / 1 ignored（其中 format 相关 **33**）；`rds-settings` lib **47** 通过。
`cargo check -p rds-sql -p rds-settings -p rds-editor` 零 error。

> 环境注记：16 GB 机器上 `cargo test -p rds-settings -p rds-editor --lib -j 2` 会在 codegen 阶段
> **OOM**（`memory allocation of N bytes failed` → 连带 `could not compile rds-store`、
> `E0463 can't find crate for gpui_kit`）。有效做法是**拆单包 + `-j 1`**，必要时叠加
> `CARGO_PROFILE_TEST_DEBUG=0` / `CARGO_PROFILE_TEST_CODEGEN_UNITS=4`（注意这会改变 test profile
> 指纹，之后回常规构建需要重编一次）。

断言全部来自 2026-11-22 对 `lax-sql 0.3.0` 的**实测输出**（探针打印后校准），没有按记忆写期望值。

---

## 9. 后续优化建议

1. **运算符间距归一** —— **已实现**（2026-11-22 第三轮）：`sql_format.normalize_operators`（默认关），
   白名单 13 项、一元/通配按前 token 判定、只补不删，15 条实测用例 + 幂等断言 + 整链路断言。
   **真实语料对拍已补**（2026-11-22 第四轮）：85 个人工迁移文件 × 3 组 options，`idem_fail`
   default 0 / ops-on 0 / all-on 1，`sig_fail` 三组恒 0（见 §8）。
2. **`FormatReport` 语义收口**：把 `kept_verbatim` 改名 `unchanged`（或加 `unchanged` 并保留旧字段
   标 deprecated），同步改 `FormatPlan` 与状态栏文案。现在"恒 0 的字段"容易被后来人误用。
3. **幂等性守门** —— **已实现**（2026-11-22 第四轮）：`crlf_line_comments_do_not_accumulate_cr`
   （3 轮相等 + 不得出现 `\r\r`）与 `real_migration_fragment_is_idempotent`（真实迁移片段 +
   保真签名）已进单测；`probe_real_corpus` 作为回归探针保留。**仍未覆盖**的只剩
   `comma_style: leading`（§7 #9，前提不成立，不承诺）。
4. **`format_on_save` 的"半截语句保护"**：保存时格式化最怕动到没写完的语句。可选加
   `sql_format.format_on_save_requires_parseable`（默认关）：保存路径先用 sqlglot 探一次可解析性，
   不可解析则跳过整篇。**Ctrl+Shift+F 保持不拦**（用户是主动要求排）。
5. **选区格式化的可见反馈**：能力已生效（这是本轮最大收益），选区也已在 2026-11-22 第二轮补了
   **吸附到完整语句**（§7.1）。但状态栏仍只报"已格式化 N 条语句"——建议选区场景补一句
   "（已排整条语句）"并把前后 diff 行数报出来，用户才知道这段真被重排了。
6. **`dprint-ignore` 用法提示** —— **实测已完成**（2026-11-22 第四轮，§7.2）：独立行生效、
   行尾不生效、块注释独立行也生效、只作用于紧跟的第一条。**设置页 hint 已同步**
   （`registry/entries/sql_format.rs` 的节点忽略标记 / 整篇忽略标记两项都写明"独占一行、紧贴语句上方"）；
   **仍未做**：格式化回执里的提示。
7. **词法失败要可见**（§7 #8）：`highlight_spans` 返回空时，后处理三步全静默跳过。可在回执里区分
   "排版完成、后处理跳过"与"全部完成"，让用户知道为什么某些开关没反应。当前静默降级优先于误改，
   暂不实现。

---

## 10. 变更记录

| 日期 | 变更 |
| --- | --- |
| 2026-11-22 | 排版从 `sqlglot-rust::generate_pretty` 换成 `lax-sql 0.3.0`；确立"语义走 sqlglot，排版走 lax"；设置项 6 → 11；`rewrite.rs` gap 规整补换行符参数（修 CRLF 混排）；`FormatOptions` 去 `Copy`；`formatter.rs` 测试按 lax 实测输出重写 |
| 2026-11-22（第二轮） | 核证 lax **无选区 API**（整篇是唯一入口，`generation` 私有）；选区补 `expand_to_statements` 吸附到完整语句（`clamp_range` 只做边界钳制）；新增 4 条选区测试；`rds-editor` lib 831 全绿 |
| 2026-11-22（第三轮） | 设置项 **11 → 12**：新增 `sql_format.normalize_operators`（"运算符两侧补空格"，默认关）；`formatter.rs` 加 `normalize_operators` + `SPACING_OPERATORS`（13 项）+ `is_unary_or_wildcard`（前 token 判据），在 `apply_options` 第 3 步执行并重新取 spans；新增 3 条测试（15 用例 / 幂等 / 整链路开关）；实测记录两处边角——`select !a, ~a` 词法失败导致后处理静默失效、`@@` 是两个 `@` token；`rds-sql` lib 226、`rds-editor` lib 832、`rds-settings` lib 47 全绿 |
| 2026-11-22（第四轮） | 真实语料对拍（85 个迁移文件 × default / ops-on / all-on）暴露两处非幂等：①`lax 0.3.0` 行注释把行尾 `\r` 留在 token 里 → CRLF 输入每轮多一个 `\r`（修复前 68 / 68 / 67），新增 `strip_line_comment_cr`（按 sqlglot 注释 span 判据、跳过块注释、在喂给 lax 前删），修复后 `default` 0；②补空格撑宽 `line_width` → 二轮 lax 再折行（修完 `\r` 后 ops-on 仍剩 1 个 `011`），`format_single` 加不动点迭代（仅 `affects_text()` 且 `comma_style == Trailing`，最多 2 轮），修复后 0；无条件迭代的首版把 `all-on` 抬到 35，收窄后回到 1（`005` 的 `leading` 固有摆动，§7 #9）。新增 2 条永久测试（CRLF 不累加 / 真实片段幂等）；`dprint-ignore` 三种写法实测完成（§7.2）；`rds-sql` lib 232、`rds-editor --lib format` 33 全绿，`rustfmt --check` clean |
