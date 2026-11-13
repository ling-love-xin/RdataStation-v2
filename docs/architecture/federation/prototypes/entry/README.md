# 联邦查询公共入口 · 原型索引

本目录是「入口」的界面原型。按模块拆成独立文件，**每个文件覆盖该模块的全部状态**（空态 / 门控 / 失败 / 恢复），避免实现时缺分支。

配套设计文档：`../federation-entry-design.md`（模型、形态、开发计划、Oracle 第三方插件 ATTACH 契约）。

| 文件 | 对应模块 | 覆盖的子功能与状态 |
| --- | --- | --- |
| `00-overview.html` | 总览 | 四屏串联：洞察现状 → 洞察+联邦 → 编辑器通道 → 结果集分析 |
| `01-source-picker.html` | `workbench/src/components/source_picker.rs` | 三分法 tab + 可折叠对象树 + 跨根搜索 + 最近使用 + 右侧元信息 + 可见范围标注 |
| `02-insight-dialog.html` | `insight_new_dialog.rs` + `insight/src/service/persistence.rs` | 取样参数（数据量 / 采样方式 / 种子 / 探查列 / 过滤 / 规模预估）+ 会话段三态（默认本地 / 联邦可用 / 源不足） |
| `03-result-analysis.html` | `result_panel/src/analysis.rs` + `editor/src/ports/result_service.rs` | 分析菜单四项、自定义 SQL 与 `{table}` 替换、事实档 / 降级档、20,000 行上限与超出计数、会话三态、会话失效提示 |
| `04-editor-channel.html` | `editor/src/view/host/toolbar` + `editor/src/ports/editor_channels.rs` | 三档执行位置、源清单仅在联邦档可见、三档对照、门控、只读拒绝 |
| `05-session-panel.html` | `engine/src/duckdb/federation/{session,registry}.rs` | 会话头（owner / 源数 / 主源 / 会话内表 / 内存）、7 种源状态、失败原因 → 用户动作、生命周期、会话内表读数 |
| `06-object-tree.html` | `workbench_shell/src/tree.rs` + `ui.rs` | 树的层级规则、复用 / 不复用清单、搜索排序档位、键盘、懒加载、与 M4/M5/M6 的差异、Phase 5 落地切片 |

## 来源分类：根 3 / 叶 5

根分类只回答一个问题：**这份数据怎么读**。它决定执行路径，也决定物化方式与错误类型。叶子只表达「在哪、还能活多久」，不开独立 tab。

| 根 | 叶子（`SourceKind`） | 代码当前标签 | 组内标题 | 怎么读 | 寿命 / 可见性 |
| --- | --- | --- | --- | --- | --- |
| 数据库 | `Connected` | 已连接数据库 | 连接 → catalog → schema → 表 | 驱动 + 连接，SQL 下推 | 连接在，表就在 |
| DuckDB | `ResultSet` | 结果集 | 结果集 · `tmp_q_` | 本地 SQL / 会话 SQL | 随结果集；联邦档下仅该会话可见 |
| DuckDB | —（无枚举） | — | Mock 数据 · `tmp_m_` | 本地 SQL | 随会话 / 单例 |
| DuckDB | —（无枚举） | — | 插件输出 · `tmp_p_` | 本地 SQL | 随会话 / 单例 |
| 文件 | `Scratchpad` | 草稿箱 | 草稿箱 | `read_*` reader 函数 | 应用管理，相对路径，随项目 |
| 文件 | `Analysis` | 资源分析 | **资产库 · 分析表** | 本地 SQL（`analytics.duckdb` `main`） | 持久，只读，靠定义重建 |
| 文件 | —（无枚举） | — | **资产库 · 归档文件** | `read_*` reader 函数 | 持久，只读，带版本（`resources/`） |
| 文件 | —（无枚举） | — | **资产库 · 全局（跨项目）** | 全局库连接（单写者，不得另开） | 持久，只读，`G_` 前缀，跨项目唯一法定通道 |
| 文件 | `External` | 外部文件 | 外部文件 | `read_*` reader 函数 | 一次性引用，文件可能被移走 |

**枚举不变**：`SourceKind` 仍是 5 个叶子，只在 UI 层加一个 `group_of(kind) -> Database / DuckDb / File` 纯函数与分组标题。这样 `PickerConfig.kinds`、`has_target()`、`confirm_prefix` 都不用改。

### 三个根其实是一张二维表

| | 本机 | 外部 |
| --- | --- | --- |
| **持久** | 文件（草稿 / 资产） | 数据库（连接的表） |
| **临时** | DuckDB（结果集 / Mock / 插件 / 会话表） | — |

"持久 vs 临时"决定**能不能引用**，"本机 vs 外部"决定**怎么读**。九个可能的组合里用户能感知的只有这三格，所以是三个根而不是九个。

### 三处措辞已经落定

1. **结果集并入 DuckDB 组**（不单开 tab）：它本来就是引擎里的临时表，与「分析库表」同一条引擎；洞察只要当前可见也能读它，独立 tab 反而制造两套入口。
2. **「资源分析」正名为「资产库」**：`ArchiveKind::Analysis` 的注释明确「本体在 `{项目}/.RSmeta/analytics.duckdb`，靠定义重建，第二期」；`ArchiveKind::File` 的本体在 `{项目}/resources/`。两者都是**本机持久物**，与「文件」同类，所以资产库挂在文件根下，而不是 DuckDB 根下。
3. **草稿箱与外部文件同属「文件」**：读取方式相同（reader 函数），差异只在持久性，用组内一行说明表达即可。

### 一处必须保留的区分

结果集在两种档位下的**可见范围**不同：本地档 `tmp_q_` 建在单例，本机任何入口都能引用；联邦档建在会话里，只有该会话能查，会话重建即失效。列表里用 pill 标注「本机可见 / 仅会话 xxx」，否则用户点了会得到 `table does not exist`。

### 数据来源注意

`collect_analysis_tables` 的查询是 `SELECT table_name FROM information_schema.tables WHERE table_schema = 'main'`（`source_picker.rs:364`）。限定 `main` 是必要的：`temp` schema 里是引擎临时表，不加限定资产库就会混进 `tmp_*`。

资产库要**并列两段**，不是「无项目时回落」：本项目 `{项目}/.RSmeta/analytics.duckdb` 与全局 `{system}/analytics.duckdb`（表名 `G_` 前缀，登记在 `{system}/global.db`，`scope='global'`）。全局库是**单写者**（`GlobalDatabaseManager::duckdb_conn()` 持有），picker 不得再 `Connection::open` 一份——现状正是直接 open，接全局前必须换通道。详见 `06-object-tree.html` §09。

## 对象树与搜索

三个 tab 内都是**可折叠的树**（不是现状的单列追加）。结构靠树定位，名字靠搜索直达：

| 能力 | 解决什么 | 落在哪 |
| --- | --- | --- |
| 可折叠树 | 我知道它在哪，还想看同层有什么 | `01-source-picker.html` |
| 跨根搜索（前缀 > 子串 > 子序列） | 我知道它叫什么，但记不清在哪一栏 | `01-source-picker.html` + `06-object-tree.html` §03 |
| 最近使用（置顶 3–5 条） | 重复用同一个对象 | `01-source-picker.html` |
| 右侧元信息 | 「这是什么、多大、能不能用」 | `01-source-picker.html` |
| 复用行原语 | 与导航栏 / 草稿箱视觉一致 | `06-object-tree.html` §02 |

复用的是 `workbench_shell::tree` 的行原语（`active_bar` / `disclosure_slot` / `disclosure_icon` / `indent_spacer` / `indent_rem` / `count_badge` / `RowHeight`）与 `ui::TREE_INDENT`；**不复用 `tree::Tree`**——外壳已裁决「这一行长什么样归各面板」，picker 是第 5 个消费方，同理。

## 视觉口径

沿用 `docs/architecture/insight/prototypes/workspace/workspace.html` 的暗色 token，不另造色板：

```
--chrome #181818  --editor #1F1F1F  --popover #202020  --border #2A2A2A
--text #CCCCCC    --muted #858585   --dim #6E6E6E      --white #F2F2F2
--coral #A84A38   --coral-bright #C25B46  --coral-soft #3A2521
--success #4A9B6B --warning #C9A23A --danger #D0534A  --info #4A8BC9
--radius 6px      正文 13px / 次要 11px / 标签 10px
字体 Segoe UI + Microsoft YaHei UI；等宽 Cascadia Mono
```

## 尚未画到的部分（实现前要补）

- 联邦会话**首次挂载的进度条与取消**（源多时首次 ATTACH + 装扩展会很久）
- **扩展安装进度与失败**（含第三方插件 `allow_unsigned_extensions` 的设置页与风险文案）
- 洞察样本因**会话重建失效**后的提示条（设计文档 §3.4 已定义语义，界面未画）
- 联邦档下**写语句被拒**的完整对话框（`04` 只在状态行做了最小示意）
- 结果集**「表已失效，可重新分析」**的条目态（`03` 只在说明里写了文案）
