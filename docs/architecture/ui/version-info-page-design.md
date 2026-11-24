# 版本信息页 · 设计（版本与依赖）

> 状态：**设计稿（2026-11-25，未落地）** · 原型 `version-info-page-prototype.html`
> 前身：`version-badge.md` §5 的「关于」弹窗（`crates/workbench/src/components/version_dialog.rs`，560 宽 Dialog，9 项）
> 本文回答「版本信息长什么样、每格数据从哪来」；标识格式与注入机制仍以 `version-badge.md` 为准

## §1 为什么要更细

现有弹窗只回答了一半问题：

| 现状 | 缺口 |
| --- | --- |
| 身份 5 项（标识 / 版本号 / 提交 / 工作树 / 提交时间） | 够用 |
| 构建 4 项（构建档 / 目标平台 / lax-sql / gpui-kit） | **依赖只列 2 个**：验证时判断不了 sqlglot / DuckDB / SQLite 的真实版本 |
| — | **引擎版本不可见**：DuckDB 早已动态链接 `third_party/duckdb/1.5.6`，弹窗里只有 crate 版本 `1.10506.1`，两者要换算才知道；SQLite 同理 |
| — | **没有构建时间 / Rust 版本**：拿到别人贴的截图时无法判断这个 exe 是不是「今天这次」编的 |
| — | 没有许可与来源：`gpui-base` / `gpui-component` 是 **vendor + 私有 patch**，与 crates.io 发布包不是同一份，验证时容易误判 |

一次打开要能回答四个问题：**哪次提交 / 什么构建 / 什么引擎 / 用了哪些库**。

## §2 形态决策

| 方案 | 形态 | 优点 | 代价 | 结论 |
| --- | --- | --- | --- | --- |
| **A（推荐）** | 独立「版本与依赖」窗口，约 **980×620**，与设置窗口同档（`TitleBar::window_options()` + `open_window`） | 两栏放得下 16 行 × 6 列依赖表 + 左栏 13 项身份；可搜索、可切「全部依赖」；与设置页体验一致 | 比 Dialog 多一次窗口管理（复用 `settings_window` 的存活检测套路） | ✅ 主推 |
| B | 仍是 Dialog，加 `TabBar` 分「概览 / 依赖 / 许可」，宽 720 | 改动小，不新增窗口 | 560→720 仍装不下 6 列；切页丢上下文；`DescriptionList` 的 vertical 坑仍在 | 备选 |
| C | 并入全局设置「关于」节（不占 Dock，延续上一轮口味） | 零新窗口 | 真机验证要「一眼看到」，多一层导航不合适 | 不做 |

形态 A 的入口保持现状：**状态栏右端那枚版本 chip** 点击打开（`view::actions::render_status_bar`）。另加菜单「帮助 → 版本与依赖」一条，便于没看到 chip 的人找到。

> 依据（实测）：`version-dialog.rs` 头注释已记「`DescriptionList::vertical()` 会拆成两行一项，9 项就是 18 行，必然撑破 560 触发内部滚动」。依赖明细用表格而非 DescriptionList，页高由内容决定。

## §3 页面结构

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ 版本与依赖                                              [复制摘要] [复制全部] [关闭] │  ← 页头 48
├───────────────┬───────────────────────────────────────────────────────────────┤
│ 应用身份       │ [搜索 crate / 用途…            ] [全部] [SQL] [数据] [UI] [基础]  │  ← 工具条 40
│  标识 0.2.12+… │                                                               │
│  徽标 CLEAN    │ ┌──────────┬──────────┬────────────┬────────┬──────┬─────────┐ │
│  提交 / 分支    │ │ 名称      │ 版本      │ 引擎 / 上游 │ 来源   │ 许可 │ 用途     │ │
│  工作树        │ ├──────────┼──────────┼────────────┼────────┼──────┼─────────┤ │
│  提交时间      │ │ sqlglot   │ 0.11.2   │ —          │crates  │ MIT  │ 语义…    │ │
│ ─ 构建环境 ─    │ │ lax-sql   │ 0.3.0    │ —          │crates  │ MIT  │ 排版…    │ │
│  构建档/目标    │ │ …         │ …        │ …          │ …      │ …    │ …        │ │
│  构建时间(新)   │ └──────────┴──────────┴────────────┴────────┴──────┴─────────┘ │
│  Rust / Cargo  │ 共 16 项 · 6 组 · 全部依赖 1218 项（切「全部」看）            │  ← 页脚 36
├───────────────┴───────────────────────────────────────────────────────────────┤
│ 数据来源：版本 = 构建期锁定（Cargo.lock）；引擎版本 = 运行时自述（rusqlite / PRAGMA version）│
└───────────────────────────────────────────────────────────────────────────────┘
```

左栏固定 360（含 24 内距），右栏弹性；两栏之间 1px 分隔线，不做可拖拽（低频页面，避免记忆布局偏好）。

## §4 字段字典（左栏）

| 区域 | 字段 | 值示例（当前仓库实测） | 来源 | 缺失值 |
| --- | --- | --- | --- | --- |
| 身份 | 版本标识 | `0.2.12+ade87d3a` | `BuildInfo::tag_base()` | 无提交 → `0.2.12` |
| 身份 | 状态徽标 | `CLEAN` / `DIRTY` / `UNKNOWN` | `dirty`（`Option<bool>`） | `UNKNOWN` |
| 身份 | 版本号 | `0.2.12` | `CARGO_PKG_VERSION` | — |
| 身份 | 提交 · 分支 | `ade87d3a · main` | `RDS_BUILD_COMMIT` / `RDS_BUILD_BRANCH` | `unknown` |
| 身份 | 工作树 | `干净（与上面提交一致）` | `RDS_BUILD_DIRTY` | `unknown（非 git 检出，无法判定）` |
| 身份 | 提交时间 | `2026-11-25 03:10:44 +08:00` | `RDS_BUILD_COMMIT_TIME` | `unknown` |
| 构建 | 构建档 | `debug` | `RDS_BUILD_PROFILE` | `unknown` |
| 构建 | 目标平台 | `x86_64-pc-windows-msvc` | `RDS_BUILD_TARGET` | `unknown` |
| 构建 | **构建时间（新增）** | `2026-11-25 04:02:11 +08:00` | 新增 `RDS_BUILD_TIME`（build.rs 取 `SystemTime::now()`） | `unknown` |
| 构建 | **Rust 版本（新增）** | `1.98.0` | 新增 `RDS_BUILD_RUSTC`（build.rs 跑 `rustc -vV` 取 `release:`） | `unknown` |
| 构建 | **Cargo 版本（新增）** | `1.98.0` | 新增 `RDS_BUILD_CARGO`（同法取 `cargo -V`） | `unknown` |
| 运行 | DuckDB 引擎 | `1.5.6（动态链接 third_party/duckdb/1.5.6）` | 运行时 `Connection::version()`（`PRAGMA version`） | `unknown（取版本失败）` |
| 运行 | SQLite 引擎 | `3.51.3（bundled）` | 运行时 `rusqlite::version()`（C 库常量，零成本） | `unknown` |

放置：身份 6 项一组；`Separator::horizontal_dashed().label("构建环境")`；构建 5 项；再一条分隔；运行 2 项。

> 既有 `BuildInfo::rows()` 返回 9 项，本设计扩为 13 项；`clipboard_text()` 同步扩（见 §7.4）。`version.rs` 现有 4 个单测里 `rows.len() == 9` 需要改。

## §5 依赖清单（右栏）

### 5.1 列定义

| 列 | 宽 | 内容 | 说明 |
| --- | --- | --- | --- |
| 名称 | 148 | **显示名**（人念的名字），下挂 crate 名（等宽小字） | `sqlglot` / crate `sqlglot-rust`；`Faker` / crate `fake`；点名库与 crate 名不同时这一层是关键 |
| 版本 | 96 | 锁定版本 | 等宽字体，右对齐视觉锚点 |
| 引擎 / 上游 | 180 | 引擎真版本（DuckDB / SQLite）；无引擎的写「—」 | 引擎版本来自运行时自述 |
| 来源 | 96 | `crates.io` / `vendor+patch` / `path` | vendor 用 `Tag::secondary().outline()` |
| 许可 | 110 | SPDX 表达式 | `MIT` / `Apache-2.0` / `MIT OR Apache-2.0` |
| 用途 | 弹性 | 一句话（≤ 28 字） | 说明「本项目为什么用它」，不是 crate 的官方简介 |

### 5.2 核心清单（实测版本，2026-11-25）

| 组 | 名称（crate） | 版本 | 引擎 / 上游 | 来源 | 许可 | 用途 |
| --- | --- | --- | --- | --- | --- | --- |
| SQL 与分析 | **sqlglot**（`sqlglot-rust`） | 0.11.2 | — | crates.io | MIT | 语义：解析 / 转译 / tokenizer（`engine::sql` 唯一接入） |
| SQL 与分析 | lax-sql（`lax-sql`） | 0.3.0 | — | crates.io | MIT | 排版：lossless 空白格式化，永不动语义 |
| SQL 与分析 | arrow | 58.4.0 | 与 duckdb 同版 | crates.io | Apache-2.0 | 结果集 `RecordBatch`（两端传数据） |
| 数据引擎 | **DuckDB**（`duckdb`） | 1.10506.1 | **1.5.6** · 动态链接 `third_party/duckdb/1.5.6` | crates.io | MIT | 分析执行引擎（结果集直灌） |
| 数据引擎 | **SQLite**（`rusqlite` / `libsqlite3-sys`） | 0.40.2 / 0.38.2 | **3.51.3** · bundled | crates.io | MIT | 元数据与本地存储（静态编入，无外部 dll） |
| 驱动 | mysql_async | 0.37.1 | — | crates.io | MIT OR Apache-2.0 | MySQL 协议客户端 |
| 驱动 | tokio-postgres | 0.7.18 | — | crates.io | MIT OR Apache-2.0 | PostgreSQL 客户端 |
| UI 框架 | **GPUI-kit**（`gpui-kit`） | 0.10.0 | features `gpui-fast` | crates.io | Apache-2.0 | 组件门面（控件一律从此取） |
| UI 框架 | gpui-base | 0.10.0 | — | **vendor+patch** | Apache-2.0 | gpui 内核基座（项目私有 patch） |
| UI 框架 | gpui-component | 0.10.0 | — | **vendor+patch** | Apache-2.0 | 控件实现（项目私有 patch） |
| UI 框架 | gpui-pre | 0.3.12 | — | crates.io | Apache-2.0 | gpui 上游核心（`pre` 线，与 gpui-fast 同编） |
| UI 框架 | **GPUI-fast**（`gpui-fast`） | 0.1.17 | 渲染内核 | crates.io | Apache-2.0 | wgpu 渲染后端（Windows 需宿主 manifest） |
| 数据生成 | **Faker**（`fake`） | 5.1.0 | — | crates.io | MIT OR Apache-2.0 | Mock 数据（zh-CN / en / ja / zh-TW / fr 多 locale） |
| 基础设施 | tokio | 1.54.1 | — | crates.io | MIT | 异步运行时 |
| 基础设施 | serde / serde_json | 1.0.229 / 1.0.154 | — | crates.io | MIT OR Apache-2.0 | 配置与 IPC 序列化 |
| 基础设施 | chrono | 0.4.45 | — | crates.io | MIT OR Apache-2.0 | 时间格式化（提交时间 / 本地时区） |

覆盖点名清单：**sqlglot ✓ / faker ✓ / gpui-fast ✓ / gpui-kit ✓ / SQLite ✓ / duckdb ✓**（表内加粗行）。

### 5.3 「全部依赖」二级视图

`Cargo.lock` 共 **1218** 个 `[[package]]`（含 workspace 成员）。切到「全部」后同一张表换数据源、去掉「用途」列（全量没有人工用途文案），保留 名称 / 版本 / 许可；顶部加一行说明「全量清单由 `Cargo.lock` 生成，不含用途与引擎版本」。

## §6 数据从哪来（机制）

### 6.1 现状（`crates/workbench/build.rs`）

已注入 8 个 `RDS_BUILD_*`，其中两个依赖版本用 `lock_version(root, "lax-sql")` / `"gpui-kit"` 从 `Cargo.lock` 按 `name = "x"` → 第一条 `version = "..."` 采集。

### 6.2 新增：生成依赖清单源码（而不是堆环境变量）

核心清单是 16 项 × 6 字段，环境变量装不下，也不该塞。做法：

- `build.rs` 读 `cargo:manifest` 侧的白名单（见 6.3），对每项 `lock_version(...)` 取版本，拼成一段 Rust 源码写进 `$OUT_DIR/deps_generated.rs`：
  ```rust
  pub const CORE_DEPS: &[Dep] = &[Dep { display: "sqlglot", krate: "sqlglot-rust", version: "0.11.2", kind: DepKind::CratesIo, license: "MIT", purpose: "语义：解析 / 转译 / tokenizer" }, …];
  pub const ALL_DEPS: &[( &str, &str )] = &[("ab_glyph", "0.2.29"), …];   // 1218 条
  ```
- `version.rs` 里 `include!(concat!(env!("OUT_DIR"), "/deps_generated.rs"))`，`BuildInfo` 增 `core_deps` / `all_deps`。
- `rerun-if-changed=../../Cargo.lock` 已存在 → 版本变化会重跑（无需新增监听）。
- 拿不到版本的项：`version` 写 `"unknown"`，页面照常显示（不阻断构建）。

> 本仓库目前**没有** `include!(concat!(env!("OUT_DIR"), …))` 先例（`git grep` 为空），这是首个；备选是「白名单写死在 `version.rs`、只把版本用 8 个环境变量传」——但 16 项就要 16 个变量，不可维护。故采用生成源码。

### 6.3 白名单与元数据放哪

新建 `crates/workbench/deps-manifest.toml`（构建期读，不入 `version.rs` 常量）：

```toml
[[dep]] display="sqlglot" krate="sqlglot-rust" kind="crates-io" license="MIT" purpose="语义：解析 / 转译 / tokenizer"
[[dep]] display="Faker"  krate="fake"        kind="crates-io" license="MIT OR Apache-2.0" purpose="Mock 数据（多 locale）"
```

- 版本**不写**在 toml 里（避免与 `Cargo.lock` 漂移），只写「显示名 / crate 名 / 来源 / 许可 / 用途」。
- 许可与用途是人工事实，必须与 crate 的 `Cargo.toml` 一致；升级依赖时同步（见 §11 的测试）。
- `rerun-if-changed=crates/workbench/deps-manifest.toml` 需要新增一行。

### 6.4 引擎版本：运行时自述

| 引擎 | 取法 | 成本 | 位置 |
| --- | --- | --- | --- |
| SQLite | `rusqlite::version() -> &'static str`（`rusqlite-0.40.2/src/version.rs:21`） | 零（C 常量） | 可在 `version::info()` 内直接填 |
| DuckDB | `duckdb::Connection::version() -> Result<String>`（`duckdb-1.10506.1/src/lib.rs:709`，内部是 `PRAGMA version`） | 建一次内存连接 | **只能在打开页面的动作路径里取一次**，结果缓存进 `RuntimeInfo` |

铁律（沿用项目「render 不读盘」）：页面**渲染函数只读结构体**；DuckDB 版本在 `open_version_page(...)` 里取，失败记 `unknown` 并把错误文案放进 hover 提示，不在 render 里重试。

### 6.5 构建时间 / Rust / Cargo 版本

`build.rs` 里：

- 构建时间：`SystemTime::now()` → 本地格式化（`chrono` 仅在工作台 crate 可用；build.rs 不引依赖，用 `time` 手工拼接 UTC+offset 或直接写 Unix 秒，由运行期 `chrono` 格式化 —— **推荐后者**：注入秒数，`version.rs` 复用 `commit_time_text()` 的格式化逻辑）。
- `rustc`：`Command::new(std::env::var("RUSTC")?)` 不行 —— build script 里可用 `RUSTC` 变量但那是路径；稳定做法是 `Command::new("rustc").arg("--version")`（项目已用 `Command` 跑 git，同法）。取 `--version` 首行即可（`rustc 1.98.0 (797e8a9bc 2026-08-05)`），页面只显示版本号段。
- 失败一律 `unknown`，不阻断（沿用 `version-badge.md` §3 的原则）。

## §7 交互规格

### 7.1 搜索

- 单行输入（`InputState`），placeholder「搜索 crate / 用途…」，即时过滤（内存过滤，1218 条也不卡）。
- 匹配：显示名 / crate 名 / 用途 / 许可（大小写不敏感）。
- `Ctrl+F` 聚焦搜索框；`Esc` 先清空搜索、再 `Esc` 才关窗口（两级，避免误关）。

### 7.2 分组筛选

`[全部] [SQL 与分析] [数据引擎/驱动] [UI 框架] [数据生成] [基础设施]` 一组 `TabBar::segmented`（或 `Button` 组按现有 `seg` 样式）。与搜索是「与」关系。

### 7.3 排序

默认按组内顺序（设计序 = 重要性序，不按字母）。点「名称 / 版本」列头可切换升降序；排序只在当前视图内生效，不持久化（低频页面，避免记偏好）。

### 7.4 复制（三种格式）

| 按钮 | 内容 | 用途 |
| --- | --- | --- |
| 复制摘要 | 现有 `clipboard_text()` 扩为 13 行纯文本（± 引擎版本两行） | 贴进 issue / 验证记录（`version-badge.md` §8 那句格式） |
| 复制全部 | Markdown 表格（§5.2 的 16 行原样）+ 构建段 | 贴进文档 / PR 描述 |
| （行级）复制单行 | `name version (license)` 一行 | 只提某个库时用 |

页头只放两个主按钮（摘要 / 全部），行级复制在右键菜单（`PopupMenu`），不占按钮位。

### 7.5 其它

- 点击「来源 = vendor+patch」的 Tag → 弹出说明（`Alert` 或 `Dialog`）：「与 crates.io 发布包不同，含项目私有 patch，明细见 `docs/architecture/ui/gpui-kit-0.10.0-upgrade-report.md`」。
- 全量视图行数多 → `DataTable` 虚拟滚动；核心视图 16 行用同一 `DataTable`（`stripe(true)`）以保持视觉一致。
- 关闭：页头「关闭」或 `Esc`。

## §8 视觉与组件（只用 gpui-kit 官方控件）

| 用途 | 组件 | 备注 |
| --- | --- | --- |
| 依赖表 | `table::DataTable`（`DataTable::new(&state)`，`stripe(true)`、`bordered(false)`） | 需要 `TableDelegate`；16 行 + 1218 行两种数据源共用一个 delegate 实现 |
| 搜索 | `input::Input` + `InputState` | 与设置页搜索框同款 |
| 分组筛选 | `tab::TabBar::segmented` 或既有 `seg` 按钮组 | 设置页已用 `TabBar::segmented` |
| 状态徽标 | `tag::Tag`（`outline()`） | `DIRTY` → `Tag::danger()`、`CLEAN` → `Tag::success()`、`UNKNOWN` → `Tag::secondary()` |
| vendor 标记 | `Tag::secondary().outline()` | 文案「vendor+patch」 |
| 脏树警示 | `alert::Alert::warning` | 沿用现有 |
| 分组分隔 | `separator::Separator::horizontal_dashed().label(...)` | 沿用现有 |
| 按钮 | `button::Button`（`ButtonVariant::Primary` 复制、`Ghost` 关闭） | 沿用现有 |

尺寸：窗口 980×620（设置窗口 980×560 同宽，高度多 60 给依赖表）。行高 32，表头 34。颜色全走 `cx.theme().colors`（`background` / `border` / `muted_foreground` / `primary`），零裸 hex。版本号用等宽（`theme` 的 mono 字体族或 `font_family("Cascadia Mono")`，与现有原型一致）。

## §9 i18n

新增 key（`crates/shared/locales/{zh,en}.json`）：

`版本与依赖` / `应用身份` / `构建环境` / `运行引擎` / `核心依赖` / `全部依赖` / `搜索 crate 或用途` / `复制摘要` / `复制全部` / `来源` / `许可` / `用途` / `引擎 / 上游` / `vendor+patch` / `构建时间` / `Rust 版本` / `Cargo 版本` / `共 {n} 项` —— 现有 `关于 RdataStation` / `复制版本信息` / `关闭` 继续用。

## §10 空态与异常

| 情况 | 表现 |
| --- | --- |
| 非 git 检出（发布包） | 身份区 3 行 `unknown`，状态徽标 `UNKNOWN`，页头提示条「无 git 信息：该构建无法反查提交」 |
| 工作树脏 | `DIRTY` + `Alert::warning`（现有文案保留） |
| 某依赖锁定版本缺失 | 该行版本显示 `unknown`，不隐藏行（行在、版本缺，比整行消失更可信） |
| DuckDB 建连接失败 | 引擎列 `unknown`，hover 显示错误首行；不影响其它行 |
| 全量清单为空（无 `Cargo.lock`） | 「全部」页显示一句占位「无 Cargo.lock：全量清单不可用」 |

## §11 验收与测试

单测（在 `version.rs` / 新 `deps.rs`，纯函数）：

1. `CORE_DEPS` 非空且每项 `version != ""`；点名 6 个库的 `krate` 必须在表内（防删行）。
2. 白名单 `deps-manifest.toml` 的 `krate` 全部能在 `Cargo.lock` 找到版本（缺则测试失败，暴露漂移）。
3. 搜索过滤：`"sql"` 命中 sqlglot / lax-sql / SQLite；`"faker"` 命中 `fake` 行（显示名匹配）。
4. 复制三种格式各含关键字段（提交 / 引擎版本 / Markdown 表头）。
5. `rows()` 扩为 13 项后顺序稳定。

窗口测试（工作台 `--lib`）：

6. 打开页面 → 表内 16 行渲染；点「全部」→ 行数 1218（或 `allow` 抽样断言）。
7. 搜索输入 `duckdb` → 只剩 1 行。
8. 点「复制摘要」→ 剪贴板文本含 `commit` 与 `SQLite 3.51.3`。

契约：`workbench/tests/ui_contract.rs` 把新页面文件加入颜色扫描（零裸 hex）。

## §12 分期

| 期 | 内容 | 落点 |
| --- | --- | --- |
| P1 | 独立窗口 + 左栏 13 项 + 右栏核心 16 行（静态数据，先不搜索） | `components/version_page.rs`（新）、`version.rs`、`build.rs` |
| P2 | 搜索 / 分组筛选 / 三种复制 / 「全部」视图 | 同上 + `deps.rs`（delegate） |
| P3 | 许可清单全量（`cargo-about` 风格）、页面内链到升级报告、Windows/OS 运行时信息 | 视需要 |

P1 落地时同步改：`version-badge.md` §5（弹窗 → 页面）、`version.rs` 的 `rows()==9` 测试、菜单「帮助」项、状态栏 chip 的调用点。

## §13 与既有文档的关系

- `version-badge.md`：§3 注入表要加 3 行（`RDS_BUILD_TIME` / `RDS_BUILD_RUSTC` / `RDS_BUILD_CARGO`）与「生成源码」一节；§5 改指向本页；§8 推荐格式补引擎版本。
- `deps-manifest.toml` 是新文件，需在 `crates/workbench/README.md` 或本文件 §6.3 记一笔（本文件即权威）。
- 原型：`version-info-page-prototype.html`（本文配套，视觉稿非权威）。
