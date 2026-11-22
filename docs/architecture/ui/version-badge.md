# 版本标识与「关于」弹窗

> 目标：真机验证前先回答一个问题——**我现在跑的 exe，是哪一次提交的产物？**
> 只要这个问题答不上来，任何"验证通过/未通过"都不可信。

## §1 动机

本项目长期存在三条并行改动线，`cargo build` 的产物又被反复复用。典型事故不是代码错，而是**验证错对象**：
拿三天前的 exe 去验今天改的行为，或者验的是脏工作树而结论被记到了某个干净提交名下。

因此需要一个"念得出来、对得上号、能贴给别人"的标识，以及一个把它摊开看的入口。

## §2 标识格式

```
0.2.11+932554dc-dirty
│       │        └─ 工作树有未提交改动（含未跟踪文件）
│       └─ git 短哈希（8 位，rev-parse --short）
└─ workspace 版本号（Cargo.toml [workspace.package].version）
```

规则：

| 情况 | 形态 |
| --- | --- |
| 干净工作树 | `0.2.11+932554dc` |
| 有未提交改动（含未跟踪） | `0.2.11+932554dc-dirty` |
| 无 git 检出（发布包 / 拷出去的目录） | `0.2.11` |

`-dirty` 是**警示**，不是修饰：脏树的二进制里混着未提交的代码，它不对应任何一次提交，结论必须标明。

## §3 注入字段

构建期由 `crates/workbench/build.rs` 注入（`cargo:rustc-env`），运行期只读：

| 常量 | 来源 | 缺失值 |
| --- | --- | --- |
| `RDS_BUILD_COMMIT` | `git rev-parse --short HEAD` | `unknown` |
| `RDS_BUILD_BRANCH` | `git rev-parse --abbrev-ref HEAD` | `unknown` |
| `RDS_BUILD_DIRTY` | `git status --porcelain` 是否非空 → `1` / `0` | `unknown` |
| `RDS_BUILD_COMMIT_TIME` | `git log -1 --format=%ct` | `unknown` |
| `RDS_BUILD_PROFILE` | cargo 的 `PROFILE` | `unknown` |
| `RDS_BUILD_TARGET` | cargo 的 `TARGET` | `unknown` |
| `RDS_BUILD_LAX_SQL` | `Cargo.lock` 中 `lax-sql` 的 `version` | `unknown` |
| `RDS_BUILD_GPUI_KIT` | `Cargo.lock` 中 `gpui-kit` 的 `version` | `unknown` |

任何一条拿不到都只写 `unknown`，**不阻断构建**——版本标识是辅助设施，不能成为编译的前置条件。

注入点放在 `workbench` 而不是 `app`：UI 与弹窗在 `rds-workbench` 里，`env!` 常量必须在**编译该 crate 时**定型，
放在 `app` 的 build script 里注入，workbench 读不到。

## §4 运行期读取（`crates/workbench/src/version.rs`）

- `version::info() -> BuildInfo`：一次构建的全部字段。`dirty` 用 `Option<bool>`（`None` = 无法判定，与"干净"严格区分）。
- `BuildInfo::tag_base()`：`0.2.11+1fab7d89`，**不含**工作树后缀。弹窗顶部用它 + `Tag` 徽标表达脏净，避免同一事实说两遍。
- `BuildInfo::tag()`：§2 的单行标识（`tag_base()` + `-dirty`）。状态栏那枚标识用它。
- `BuildInfo::tree_text()`：给弹窗看的"干净 / 有未提交改动 / 无法判定"。
- `BuildInfo::commit_time_text()`：`chrono` 按本地时区格式化 `YYYY-MM-DD HH:MM:SS +08:00`。
- `BuildInfo::rows()`：弹窗逐行明细（9 行）。
- `BuildInfo::clipboard_text()`：粘进 issue / 聊天 / 验证记录的多行文本。

纯读取，无缓存、无状态；`grep` 不到任何运行期写盘。

## §5 弹窗

入口：**状态栏右端常驻标识**（`view::actions::render_status_bar` 的 `.right(...)` 末尾）。
点击打开 `components::version_dialog::open_version_dialog`。

- 为什么不在标题栏：标题栏已有主题 / 语言 / 快速布局三枚图标加三个窗口按钮，再塞一个 `Info` 会挤；
  版本核对是"低频、但验证时第一眼要看到"，压在状态栏最右端既常驻又不抢位置。
- 状态栏那枚 chip 沿用 `view::status::sb_toggle` 的既有样式（`px_1` + `rounded_sm` + 显式前景色 +
  hover 高亮 + 3.5 号图标），只因开对话框需要 `window` 而在 `actions.rs` 内联构造，未改动 `sb_toggle` 的签名。

结构（复用 `components/dialog_helpers` 的标准模板）：

- 标题：`关于 RdataStation`；弹窗宽度显式 `Dialog::w(px(560.))`
  （`Dialog` 默认 448，只在内容容器上设上限不改弹窗宽度）
- 顶部一行：`tag_base()` 大字 + 状态徽标 `Tag`（`DIRTY` / `CLEAN` / `UNKNOWN`，全部 `outline()`）
- 脏树时挂 `Alert::warning`（标题「工作树有未提交改动」，正文说明该构建不对应任何提交）
- 明细分两组，均为 `DescriptionList::new().label_width(px(120.)).columns(1)`（**horizontal 默认布局**）：
  - 身份 5 项：版本标识 / 版本号 / 提交（commit · branch）/ 工作树 / 提交时间
  - `Separator::horizontal_dashed().label("构建与依赖")`
  - 构建 4 项：构建档 / 目标平台 / SQL 排版引擎（lax-sql x.y.z）/ UI 组件库（gpui-kit x.y.z）
- 底部说明：核对只看「提交」「工作树」两行
- 脚注按钮：右为**主操作**「复制版本信息」（`Button` + `ButtonVariant::Primary` + `Copy` 图标，
  写 `ClipboardItem`），左为「关闭」

> 版式坑（改版时踩过）：`DescriptionList::vertical()` 会把每项拆成"带底色的 label 行 + 值行"两行，
> 9 项就是 18 行，必然撑破对话框高度触发内部滚动。**明细一律用默认 horizontal**。

### 5.1 落到项目（只写不展示）

打开项目时（**只读打开除外**），这个标识还会写进项目本体：

- 位置：`{项目}/.RSmeta/config/settings.json` → `extensions.rds.build_tag`
- 键名：`crates/project/src/store.rs::BUILD_TAG_KEY`
- 时机：`project::ui::apply_opened`（打开完成的唯一收口），用打开时已加载好的 `ProjectStore`
- 写盘策略：**值未变则跳过**（避免每次打开都改 settings.json 的 mtime）；写失败只 `warn`，不阻断打开
- 为什么只写不展示：项目里已有一套「资产版本链」（`Version` / `version_count` / `list_versions` /
  `create_version`，记的是**内容快照**），卡片上再出现一个"版本"会被当成它
- 依赖方向：`project` crate 不依赖 `workbench`，标识由宿主注入
  （`ProjectUiHost::with_build_tag`，装配在 `crates/workbench/src/components/project_host.rs`）

用途：排障时回答"这个项目最近是被哪个构建打开的"；想直接看就打开那个 `settings.json`，
目前没有界面入口（刻意不做）。

## §6 刷新时机（容易踩）

`build.rs` 只声明了四个 `rerun-if-changed`：

```
crates/workbench/build.rs
Cargo.lock
.git/HEAD
.git/index
```

含义与后果：

- **改源码 → 本 crate 本来就要重编 → build script 重跑 → 版本戳更新。**这是常态路径，够用。
- `git commit` / `git add`（只动 HEAD 或 index）也会触发重跑。
- **但 `git status` 结果变化本身不会触发任何东西**：改了文件却没重新编译时，弹窗里仍显示**上一次编译时**的工作树状态。
  这是这个机制的固有局限——它报的是"编译时刻"的真相，不是"现在"的真相。
  要让 `-dirty` 准确，唯一办法是**改完文件后重新编译**。

## §7 局限（如实记录）

1. `git status --porcelain` **包含未跟踪文件**（`?? …`），所以新建一个文件就足以让 `-dirty` 亮起。这是有意为之：未跟踪文件同样会进二进制。
2. 无 `.git` 的目录（`cargo package` 产物、发给别人的压缩包、CI 的干净检出副本）→ 提交与工作树字段退化为 `unknown`，标识只剩版本号。此时**无法**从二进制反查提交，只能靠发布流程记录。
3. 子模块 / 嵌套仓库状态不计入（`git status` 只在仓库根跑一次）。
4. 提交时间是 **HEAD 的提交时间**，不是构建时间；构建时间不在弹窗里，避免与"提交时间"混淆。

## §8 验证口径（怎么核对）

三个来源应完全一致：

1. 应用内弹窗的 `tag()`；
2. 仓库根 `git rev-parse --short HEAD`；
3. 仓库根 `git status --porcelain` 是否为空。

写验证记录时的推荐格式：

```
对象: RdataStation 0.2.11+932554dc-dirty
条件: debug / x86_64-pc-windows-msvc / lax-sql 0.3.0 / gpui-kit 0.9.0
说明: dirty 含 3 条 sql rustfmt + 本功能 6 个文件
```

## §9 实现位置

| 文件 | 职责 |
| --- | --- |
| `crates/workbench/build.rs` | 采集 git / 环境 / 锁定版本，注入 8 个 `RDS_BUILD_*` |
| `crates/workbench/src/version.rs` | 常量解析、`tag_base()` / `tag()` / `rows()` / `clipboard_text()`，4 个单测 |
| `crates/workbench/src/components/version_dialog.rs` | 弹窗装配（标准模板 + `Alert` / `Tag` / `Separator` / `DescriptionList` / 主位复制按钮） |
| `crates/workbench/src/components/mod.rs` | 模块登记 |
| `crates/workbench/src/lib.rs` | `pub mod version` |
| `crates/workbench/src/view/actions.rs` | 状态栏右端版本 chip（点击开弹窗） |
