# 关于 / 版本信息弹窗 —— 扩展 6 个核心库（设计定稿）

> 状态：**设计稿，代码未改**。2026-11-25 按"只要一个简介信息页"收敛，上一版 1060×640
> 独立窗口 + 可搜索依赖表的设计**作废**（否决理由见 §1 末尾），仓库里以本文为准。

## §1 结论先行

现状弹窗（`components/version_dialog.rs`，560 宽）已经有 9 项：身份 5 + 构建 4。缺的是
"我验证的这个 exe，关键依赖到底锁在什么版本"——只列了 `lax-sql` 与 `gpui-kit` 两个。

本设计**只做一件事**：在同一个 `DescriptionList` 里把「构建与依赖」组从 4 项扩到 9 项，
补上用户点名的 6 个核心库（sqlglot / faker / gpui-fast / gpui-kit / SQLite / duckdb，
其中 gpui-kit 已存在）。弹窗宽度、组件、交互、入口全部不动。

为什么不做上一版的大窗口：版本页的用途是**真机验证前一眼确认对象**，不是浏览或检索；
560 宽弹窗 14 项一屏看完（约 635 高，1080 屏余量充足），加搜索框、分组 chip、1218 行全量
视图都属于给低频场景加常驻复杂度。验收时唯一要读的是「提交」和「工作树」两行。

## §2 尺寸与形态

- `Dialog::w(px(560.))` 保持——`Dialog` 默认 448，宽度必须在 dialog 上显式给，改内容容器
  上限没有用（这条是 2026-11-22 定稿时踩过的）。
- 高度不设固定值，交给内容自适应。实测同一套样式的 9 项版在本机约 495 高，14 项按行高
  28/项推算约 **635**；1280×720 笔记本屏仍放得下（可用高度紧时先压掉「核对只需两行」
  那句说明，不减字段）。
- 不做 min/max 高度、不做内部滚动：`DescriptionList` 一旦内容超框会裁掉下半截且没有滚动条，
  比多出 20px 更糟。

## §3 结构

```
关于 RdataStation                                              ✕
────────────────────────────────────────────────────────────────
0.2.12+9719e656   [CLEAN]

┌ DescriptionList（horizontal / label 120 / columns 1 / bordered）
│ 版本标识      0.2.12+9719e656
│ 版本号        0.2.12
│ 提交          9719e656 · main
│ 工作树        干净（与上面提交一致）
│ 提交时间      2026-11-25 01:44:49 +08:00
├ ─ ─ ─ ─ ─ ─ 构建与依赖 ─ ─ ─ ─ ─ ─
│ 构建档        debug
│ 目标平台      x86_64-pc-windows-msvc
│ SQL 语义      sqlglot 0.11.2
│ SQL 排版      lax-sql 0.3.0
│ 分析引擎      DuckDB 2.0.1（crate 1.20001.0）
│ 本地存储      SQLite 3.51.3（rusqlite 0.40.2）
│ UI 组件库     gpui-kit 0.10.0
│ 渲染内核      gpui-fast 0.1.17
│ Mock 数据     fake 5.1.0
└
核对只需两行：提交、工作树。要贴给别人或记进验证记录，点右下角「复制版本信息」。
                                              [关闭]  [⧉ 复制版本信息]
```

脏树时在 hero 与列表之间插一条 `Alert::warning`（现状已有，不动）。hero 用 `tag_base()`（不带
后缀），脏由 `Tag` 表达一次；明细的「版本标识」一行用 `tag()`，脏树时带 `-dirty`——两者刻意不同：
明细那行是给人复制、口述、贴进记录的完整串。

## §4 字段字典（14 项）

| 组 | 字段 | 值示例（本机实测） | 来源 | 缺失值 |
| --- | --- | --- | --- | --- |
| 身份 | 版本标识 | `0.2.12+9719e656` | `BuildInfo::tag()` | 无提交 → `0.2.12` |
| 身份 | 版本号 | `0.2.12` | `CARGO_PKG_VERSION` | — |
| 身份 | 提交 | `9719e656 · main` | `RDS_BUILD_COMMIT` / `RDS_BUILD_BRANCH` | `unknown` |
| 身份 | 工作树 | `干净（与上面提交一致）` | `RDS_BUILD_DIRTY` | `unknown（非 git 检出…）` |
| 身份 | 提交时间 | `2026-11-25 01:44:49 +08:00` | `RDS_BUILD_COMMIT_TIME`（Unix 秒，`chrono` 本地化） | `unknown` |
| 构建 | 构建档 | `debug` | `RDS_BUILD_PROFILE` | `unknown` |
| 构建 | 目标平台 | `x86_64-pc-windows-msvc` | `RDS_BUILD_TARGET` | `unknown` |
| 依赖 | SQL 语义 | `sqlglot 0.11.2` | 新增 `RDS_BUILD_SQLGLOT`（`Cargo.lock` 的 `sqlglot-rust`） | `unknown` |
| 依赖 | SQL 排版 | `lax-sql 0.3.0` | `RDS_BUILD_LAX_SQL`（已有） | `unknown` |
| 依赖 | 分析引擎 | `DuckDB 2.0.1（crate 1.20001.0）` | 新增 `RDS_BUILD_DUCKDB` + 解码得 `RDS_BUILD_DUCKDB_ENGINE` | `unknown` |
| 依赖 | 本地存储 | `SQLite 3.51.3（rusqlite 0.40.2）` | 运行时 `rusqlite::version()` + 新增 `RDS_BUILD_RUSQLITE` | `unknown` |
| 依赖 | UI 组件库 | `gpui-kit 0.10.0` | `RDS_BUILD_GPUI_KIT`（已有） | `unknown` |
| 依赖 | 渲染内核 | `gpui-fast 0.1.17` | 新增 `RDS_BUILD_GPUI_FAST` | `unknown` |
| 依赖 | Mock 数据 | `fake 5.1.0` | 新增 `RDS_BUILD_FAKE` | `unknown` |

放弃的字段（明确不做）：

- **arrow / mysql_async / tokio-postgres / gpui-base / gpui-component / gpui-pre / tokio /
  serde / chrono**：验证时不看，加了就是噪音。
- **构建时间 / Rust 版本 / Cargo 版本**（上一版曾列为新增）：构建时间对"这是哪次提交"没有
  增信息量（提交时间已有）；`rustc` 版本出问题时是看 `rust-toolchain.toml`，不必进弹窗。
- **许可**：弹窗里只出现一次库名，塞 `MIT OR Apache-2.0` 会把值撑到两行；许可属于发布物料。

## §5 值从哪来（沿用现有机制，不引入代码生成）

现状 `crates/workbench/build.rs` 已注入 8 个 `RDS_BUILD_*`，其中 `lax-sql` / `gpui-kit` 用
`lock_version(root, "x")` 从 `Cargo.lock` 按 `name = "x"` 后第一条 `version = "..."` 采集。
本设计就沿用它，加 5 个 key，`option_env!` 未注入时回落 `unknown`（与现有 `build_env!` 宏一致）：

| 环境变量 | `Cargo.lock` 里的包名 | 本机实测 |
| --- | --- | --- |
| `RDS_BUILD_SQLGLOT` | `sqlglot-rust` | 0.11.2 |
| `RDS_BUILD_FAKE` | `fake` | 5.1.0 |
| `RDS_BUILD_GPUI_FAST` | `gpui-fast` | 0.1.17 |
| `RDS_BUILD_DUCKDB` | `duckdb` | 1.20001.0 |
| `RDS_BUILD_RUSQLITE` | `rusqlite` | 0.40.2 |

### 5.1 两个引擎版本

**DuckDB**：crate 版本 `1.20001.0` 与引擎版本 `2.0.1` 不是同一个数，直接显示前者会让验证者
误判。`libduckdb-sys` 的 `build.rs` 里 `duckdb_version_from_pkg_version` 把第二段 `20001`
拆成 major 2 / minor 0 / patch 01 —— 这是纯字符串运算。构建侧只注入 crate 版本
（`RDS_BUILD_DUCKDB`），解码放在 `version.rs` 的 `duckdb_engine_from(pkg_version)` 里，照抄同
一套规则，**不建连接、不读盘**。与上游唯一的差别是失败姿势：上游直接 `panic`，这里
`encoded < 10_000` 或解析失败一律回 `None`（`unknown`），避免「关于」弹窗因锁文件异常而崩。
根 `Cargo.toml` 注释「`1.20001.0` ↔ DuckDB `v2.0.1`」已二次确认，动态链接目录
`third_party/duckdb/2.0.1` 与之一致。

**SQLite**：`libsqlite3-sys` 的 crate 版本（0.38.2）与引擎版本（3.51.3）同样是两个数，且
bundled 的 `sqlite3.h` 在 cargo registry 里，构建期读它不稳。改用运行时零成本 API：
`rusqlite::version() -> &'static str`（`rusqlite-0.40.2/src/version.rs:21`，直接返回
`SQLITE_VERSION` 常量，不查库、不开连接）。`version::sqlite_engine()` 直接把它返回，**不需要
`OnceLock` 缓存**（已经是 `&'static str`，拷贝一次就是 `String`）；crate 版本仍走
`RDS_BUILD_RUSQLITE`。`render` 只读结构体，守现有「render 不读盘」的约定。

### 5.2 精确到什么程度

值写成「库名 + 版本（次包版本）」两种括号形态：

- 单包：`gpui-kit 0.10.0` —— 库名与版本都来自同一变量。
- 双包：`SQLite 3.51.3（rusqlite 0.40.2）` —— 冒号前是人看的引擎版本，括号里是 `Cargo.lock`
  锁住的 crate 版本。验证时要对的是前一个，出问题时查的是后一个。
- 都拿不到：`SQLite unknown（rusqlite unknown）` 显式暴露，不回落到版本号或空串。

## §6 版式约束（组件源码实测，改版前必读）

`vendor/gpui-component/src/description_list.rs` 的实际行为：

- `DescriptionList::new()` 默认 `layout: Horizontal`、`label_width: 120px`、`bordered: true`、
  **`columns: 3`**。默认三列会把 14 项排成 5 行三列，label 列 120 挤成 40——**必须显式
  `.columns(1)`**（现状代码已写，别删）。
- `vertical()` 把每项拆成"label 行 + value 行"，行高翻倍：14 项 = 28 行，必然超框。明细一律
  horizontal。
- 行高 = 文本行高 + `py(4)` 上下各 4（Medium 尺寸），约 **28/项**；`bordered` 的行间加
  1px `border_b`，第一列 label 额外有 `border_e`。
- label 单元格底色走 `tokens.description_list_label`，文字色 `description_list_label_foreground`，
  边框 `cx.theme().border`——**不要手写 hex**，深浅色由 token 自动切换。

## §7 组件与交互

| 位置 | 组件 | 说明 |
| --- | --- | --- |
| 窗口 | `Dialog::w(px(560.))` | 保持现状；标题 `tr("关于 RdataStation")` |
| hero | `div().text_lg()` + `Tag` | `tag_base()` 大字 + `CLEAN/DIRTY/UNKNOWN` 徽标（`outline()`） |
| 脏树 | `Alert::warning` | 现状已有，位置在 hero 与列表之间 |
| 分组 | `Separator::horizontal_dashed().label(tr("构建与依赖"))` | 现状已有 |
| 明细 | `DescriptionList` ×2（身份 / 构建与依赖） | 各 `label_width(px(120.))`、`columns(1)` |
| 说明 | `div().text_xs().text_color(muted_foreground)` | "核对只需两行…" |
| 底部 | `standard_dialog_footer(copy, Some(close))` | 右侧 primary「复制版本信息」+ `IconName::Copy`，左侧「关闭」 |

交互只有三个，全部现状行为，不加新键位：

1. 点「复制版本信息」→ `app.write_to_clipboard(ClipboardItem::new_string(...))`，文本扩到
   14 行（§8）。
2. 「关闭」/ 标题栏 `✕` → `window.close_dialog(cx)`。
3. 状态栏右端 chip 点击打开（`view::actions::render_status_bar`）——入口不变，不新增菜单项。

不做：搜索、分组 chip、列头排序、行级右键、全量清单视图、窗口缩放记忆。

## §8 「复制版本信息」的文本（14 行，与 14 项一一对应）

键列宽 11（值从第 12 个字符起），粘到 issue / 聊天里能直接对号：

```
RdataStation 0.2.12+9719e656
version     0.2.12
commit      9719e656 (main)
dirty       no
time        2026-11-25 01:44:49 +08:00
profile     debug
target      x86_64-pc-windows-msvc
sqlglot     0.11.2
lax-sql     0.3.0
duckdb      2.0.1 (crate 1.20001.0)
sqlite      3.51.3 (rusqlite 0.40.2)
gpui-kit    0.10.0
gpui-fast   0.1.17
fake        5.1.0
```

14 行对 14 项（第 1 行是「版本标识」+ 标题的合体），顺序同 §4 表；`version.rs` 的
`clipboard_text()` 与 `rows()` 由同一份字段序生成，避免两处漂移。`dirty` 仍是 `yes/no/unknown`
三值（比 `-dirty` 后缀更好读，也和既有文本格式兼容）。

## §9 i18n

**实现状态（2026-11-25）**：本轮沿用既有做法——明细 label 仍是硬编码中文，只有标题与按钮走
`shared::i18n::tr`。下表是待接入的 key 清单；接入时要同时改 `shared::i18n` 资源与
`version.rs::rows()` 的 label 来源，并保证「复制版本信息」的文本仍是中文（那段是给人贴到
issue / 群聊里的，不随界面语言变）。

新增 7 个 label key（中 / 英成对）：

| key | 中文 | 英文 |
| --- | --- | --- |
| `version.label.sql_semantic` | SQL 语义 | SQL parser |
| `version.label.sql_format` | SQL 排版 | SQL formatter |
| `version.label.engine_analytics` | 分析引擎 | Analytics engine |
| `version.label.engine_local` | 本地存储 | Local store |
| `version.label.ui_kit` | UI 组件库 | UI kit |
| `version.label.entry_render` | 渲染内核 | Renderer |
| `version.label.mock` | Mock 数据 | Mock data |

值里的库名（`sqlglot` / `lax-sql` / `DuckDB` / `SQLite` / `gpui-kit` / `gpui-fast` / `fake`）
**不进 i18n**——它们是产品名，翻译反而误导。

## §10 验收

单测（`crates/workbench/src/version.rs`）：

- `rows.len() == 14`（现有 `rows_and_clipboard_carry_the_key_fields` 里的 `9` 要改）。
- 新断言：`rows` 含 `SQL 语义` / `分析引擎` / `本地存储` / `渲染内核` / `Mock 数据` 五个 key。
- `clipboard_text()` 行数 14，且含 `sqlglot    0.11.2` 这类对齐后的行。
- 引擎解码单测：`duckdb_engine_from("1.20001.0") == "2.0.1"`（旧线 `1.10506.1 == "1.5.6"` 也要保一条，挡「minor 被吞」）；
  异常输入（`unknown`、`1.1`）返回 `unknown` 而不是 panic。
- 未知兜底：把五个新常量置 `unknown` 时，值仍能渲染成 `unknown（…）` 而非空串。

版式常量单测（`crates/workbench/src/components/version_dialog.rs::tests::dialog_layout_is_pinned`）：

- 钉死宽度 `560`、label 列宽 `120`、切分点 `IDENTITY_ROWS = 5`，防止有人"顺手"改内容容器
  或把字段挪进错段；同时断言 `rows()` 第 5 / 第 6 项分别是「提交时间」「构建档」。
- 脏树 `Alert` 与徽标依赖运行期 `info.dirty`，属窗口行为，本轮未新增窗口测试；核对靠真机：
  在 `-dirty` 构建下打开弹窗，应看到 `0.2.12+xxxxxxx`（不带后缀）+ DIRTY 红标 + 警示条。

**落地验证（2026-11-25）**：`cargo check -p rds-workbench --lib -j 1` 通过；
`cargo test -p rds-workbench --lib -j 1 version` 10 passed / 0 failed；
`cargo clippy -p rds-workbench --all-targets -j 1` 0 error（128 条 warning 全部在其它文件，
本文件 0 条）；`rustfmt --edition 2024 --check` 三文件通过。真机渲染未核对——14 项在
1280×720 下是否触发内部滚动，需打开一次弹窗确认（原型按同版式估算为 635 高）。

## §11 与既有文档的关系

- `docs/architecture/ui/version-badge.md`：§3 注入字段表要加 5 个新 key，§5「弹窗」结构段
  要改成分组 5 + 9，§7 局限里"许可未进弹窗"保留。
- 本目录下的原型 `version-info-page-prototype.html` 已按本稿改为 **560 宽弹窗形态**（此前
  1060×640 大窗口那版同时废弃）。
- `docs/architecture/ui/version-dialog-prototype.html`（2026-11-22 的 9 项版）作为历史基线
  保留不动，本稿落地后它的字段清单就过期了，需在其顶部加一行指向本文。
