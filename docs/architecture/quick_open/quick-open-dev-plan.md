# Quick Open · 开发方案（Phase 0–2）

> 状态：**Phase 1 第一刀已落地（2026-09-19）**；第二刀（草稿箱文件源 / 命令注册）待开工
> 关联：`quick-open-prototype-design.md`（原型设计 = 权威规格）、`quick-open-prototype.html`（交互稿）、`../layout/layout-design.md` §2.1/§3.3（入口承诺）
> 技术栈：gpui-kit 0.6.1；组件只从组件库取（禁止手搓）；取色零裸 hex；结构尺寸只引用 `crates/workbench_shell/src/ui.rs`
> 说明：本文件记录**做什么、做到哪**；「长什么样」看原型设计，「为什么这样设计」待 `quick-open-architecture.md`

## 0. 进度记录（最近在前）

### 2026-09-19 — Phase 1 第一刀：引擎侧 FTS 接线（写侧 + 清洗 + 迁移）

| # | 任务 | 落点 | 状态 |
| --- | --- | --- | --- |
| P1.1 | FTS 写侧接线：新增 `rebuild_fts_schema(schema)`（按 schema 先删后插），由 `rebuild_schema_index` **同批**调用；FTS 失败只告警（不拖垮导航赖以分页 / 计数的 `metadata_index`） | `crates/engine/src/persistence/metadata_cache.rs` | ✅ |
| P1.1a | 修复历史缺陷①：原 `sync_fts_index` 尾部引用了**不存在的表**（`FROM views` / 规范模型里视图是 `tables.table_type='VIEW'`）→ 整条同步永远跑不通（也是它长期零调用的**真因**）；新实现按规范表取数，删除旧函数 | 同上 | ✅ |
| P1.2 | 修复历史缺陷②：表原是 **contentless**（`content=''`）——实测 MATCH 能命中，但 `SELECT` 回来的 `search_type` / `object_name` **全为 NULL**（`Invalid column type Null at index: 0`），snippet 也无从生成 → 整条读路径其实不可用；迁移 011 改为**存内容 + trigram** | `crates/engine/migrations/connection_metadata/011_fts_content_and_trigram.sql` | ✅ |
| P1.3 | 查询词清洗：`fts_match_query`（拆词 → 逐词加引号 → 末词前缀）；`"` `*` `(` `NEAR` `-` 全部按字面处理（实测不报错、不改变语义） | 同上 | ✅ |
| P1.4 | 单测 3 项：写侧（分域 + 幂等 + 删 schema 不留孤儿）、读侧（对象身份 + `<mark>` snippet + 中文 ≥3 字命中 / 2 字无命中 + 操作符输入不报错）、查询词拆解 | 同上 | ✅ 全绿 |
| P1.5 | database 侧全文通道：`SearchKind{Name, FullText}`（与消费方并列的档位）+ `NavCache::search_fts` 包装（失败告警留痕）+ `SearchHit.snippet`；FTS 命中映射（**空串→None**、catalog 缺位） | `crates/database/src/{nav_jobs,cache}.rs`、`engine::persistence` 重导 `FtsSearchResult` | ✅ |
| P1.6 | 浮层 `#` 档 UI：内容档**整档切两行高**（`List` 要求同行同高）、snippet 按 `<mark>` 切段上色（`markup_segments`）、「为什么命中」标签（名称 / 内容）、门槛提示按档自适应（名称 2 字 / 全文 3 字） | `quick_open/{model,delegate}.rs`、`workbench_shell/src/ui.rs`、`tests/ui_contract.rs` | ✅ |
| P1.7 | 单测：数据库侧 FTS 映射 1 项；workbench 侧内容档行（不被标题过滤误杀 + 标签）/ `<mark>` 切段 / 档位感知门槛 | — | ✅ 全绿 |
| 验证 | `cargo test -p rds-engine --lib -j 2` → **440 项全绿**；`-p rds-database --lib` → **42 项**；`-p rds-workbench --lib` → **106 项**；`--test ui_contract` → 7 项 | — | ✅ |

**实测结论（已写进迁移与原型设计 §6.3）**：trigram 下中文**≥3 字**才命中（2 字不足一个 trigram）→ `#` 档门槛按 **3 字**；名称档继续走 `metadata_index`（LIKE 中缀，不受 3 字限制）；索引体积约为文本 3 倍量级。

**下一步（Phase 1 第二刀）**：草稿箱文件源（需扁平清单通道）、命令注册（跟 crate 登记）、截断提示与「还有 N 条」、`#` 档的最近使用/空态建议；另见原型设计 §18.2 的规模优化（前缀/中缀两段式、并发打开缓存上限、结果短时缓存）。

### 2026-09-18 — Phase 0 第三刀：浮层抽成独立视图实体 + 窗口测试

| # | 任务 | 落点 | 状态 |
| --- | --- | --- | --- |
| P0.13a | 浮层本体抽成独立视图实体 `QuickOpenPalette`（输入 / 结果 / 键盘 / 防抖 / 回填全在它身上），宿主侧只留：懒创建、挂 overlay、注入动作端口、置开关 | `quick_open/palette.rs`（新）、`view.rs`（删 ~460 行，只留装配） | ✅ |
| P0.13b | 动作端口 `QuickOpenHost`（只一个方法：执行一条结果）——`WorkbenchView` 用 `WorkbenchQuickOpenHost` 实现；浮层因此不依赖工作台视图 | `palette.rs`、`view.rs` | ✅ |
| P0.13c | 「执行后关面板」收归浮层（与 Esc / 点遮罩同一组收尾），不再依赖宿主实现——否则哑宿主下行为不完整 | `palette.rs::confirm` | ✅ |
| P0.10/P0.12 | **窗口测试 4 项**（简化宿主 + 哑端口 + 真按键）：打开即聚焦（按键进输入框）/ ↑↓ 漫游 + ↵ 经端口执行并关面板 / Esc 关闭 / 单字符门槛（1 字符不发搜索） | `quick_open/tests.rs`（新） | ✅ 全绿 |
| P0.14 | 契约登记：`quick_open/palette.rs` 纳入尺寸 + 颜色两份扫描清单 | `crates/workbench/tests/ui_contract.rs` | ✅ |
| 验证 | `cargo check -p rds-workbench` / `-p rds-app` 零告警；`cargo test -p rds-workbench --lib` → **86 项全绿**（含 4 项新窗口测试）；`--test ui_contract` → 7 项全绿 | — | ✅ |

### 2026-09-18 — Phase 0 第二刀：元数据名称档接线（Quick Open 的核心）

| # | 任务 | 落点 | 状态 |
| --- | --- | --- | --- |
| P0.11b | 防抖 150ms（`QUICK_OPEN_SEARCH_DEBOUNCE_MS`；句柄替换即取消）+ 「只在词变时发」+ 按 `SearchResult.query` **丢弃过期批次** | `view.rs::schedule_quick_open_search` / `pump_quick_open` | ✅ |
| P0.11b-2 | 宿主结果泵：60ms（开着）/ 400ms（关着）空转，视图销毁即退出；回填**不碰 UI**，只置脏标记，重建成行放 render（泵线程拿不到 `Window`） | `view.rs::ensure_quick_open_pump` | ✅ |
| P0.11c | 元数据行：`SearchHit → MetaObject`（表 / 视图 / 列 / 模式，列带父表；例程暂不接）+ 标题 `schema.name` / `表.列` + 次级信息「连接名 · 驱动」；组头带「搜索中…」 | `quick_open/model.rs::{meta_object, build_groups}`、`delegate.rs` | ✅ |
| P0.11d | 命中动作：`Action::ShowProperties(Box<PropertyRequest>)` → `Shared::show_properties`（与导航搜索结果同一去向，映射口径一致） | `quick_open/model.rs`、`view.rs::execute_quick_open_row` | ✅ |
| 单测 | +2 项：元数据组排首位且携带属性请求（列键含父表、例程被过滤）、「搜索中」只在词长达门槛时显示 | `quick_open/model.rs` | ✅ |
| 验证 | `cargo check -p rds-workbench -j 2` 零告警；`cargo test -p rds-workbench --lib -j 2` → **82 项全绿**；`--test ui_contract` → **7 项全绿** | — | ✅ |

**未做的部分（留待下一刀）**：面包屑单独一行（`List` 要求同行同高，单行次级信息暂以「连接名 · 驱动」代替）；「为什么命中」标签（属 `#` 全文档档）；元数据行的类型图标（先用文本标签）。

### 2026-09-18 — Phase 0 第二刀前置：搜索通道按消费方分流

| # | 任务 | 落点 | 状态 |
| --- | --- | --- | --- |
| P0.11a | `nav_jobs` 增消费方维度：`SearchConsumer{Navigator, QuickOpen}`；`SearchResult` / `Job::SearchIndex` 带消费方；`pending_search` 与 `search_results` 改**按消费方分槽**（`[_; COUNT]`），`has_pending_search(c)` / `enqueue_search(c, ..)` / `drain_search_results(c)` 全部带消费方 | `crates/database/src/nav_jobs.rs` | ✅ |
| P0.11a-2 | 导航侧调用点同步（1 处入队 / 1 处 drain / 3 处 pending 判定） | `crates/database/src/nav_view.rs` | ✅ |
| P0.11a-3 | 单测 2 项：分区取走互不可见、槽位下标互异且在界内 | `nav_jobs.rs` 内 `tests` | ✅ 全绿 |
| 验证 | `cargo check -p rds-database -j 2` 零告警；`cargo test -p rds-database --lib -j 2` → **40 项全绿** | — | ✅ |

### 2026-09-18 — Phase 0 第一刀：模块骨架 + `List` 化 + 键盘通道

| # | 任务 | 落点 | 状态 |
| --- | --- | --- | --- |
| P0.1 | 模块骨架：`quick_open/`（`model` 纯逻辑 / `delegate` 组件委托 / 宿主装配在 `view.rs`） | `crates/workbench/src/quick_open/{mod,model,delegate}.rs`、`lib.rs` | ✅ |
| P0.2 | 结果区换成 `List` + `ListDelegate`：分组头（section header）、空态（`render_empty`）、hover / 选中 / 滚动全归组件 | `quick_open/delegate.rs`、`view.rs::render_quick_open` | ✅ |
| P0.3 | 键盘通道：↑↓ 漫游（跨组连续）、`↵` 打开、`Shift+↵` 保留面板、`Esc` 关闭 | `view.rs`（`MoveUp` / `MoveDown` / `Escape` 由单行 `Input` 冒泡上来接住；`Enter` 走 `InputEvent::PressEnter{secondary,shift}`） | ✅ |
| P0.4 | 打开即聚焦 + 打开清空输入 / 重置选中（标题栏入口与 `Ctrl+P` 同一入口 `toggle_quick_open`） | `view.rs` | ✅ |
| P0.5 | 命中高亮（`search.match.background`）+ 类型短标签 + 结果计数 | `quick_open/delegate.rs` | ✅ |
| P0.6 | 单字符门槛：本地源照常、元数据提示「再输入 1 个字符」（闸门本身随元数据源生效） | `quick_open/model.rs::Query::async_ready` + `view.rs` 提示行 | ✅（提示层） |
| P0.7 | 命令表从 `view.rs` 硬编码搬进模型（`command_rows()`，带快捷键） | `quick_open/model.rs` | ✅ |
| P0.8 | 纯逻辑单测 6 项（前缀解析 / 词长门槛 / 命中区间 / 评分分档 / 分组过滤 / 命令键唯一） | `quick_open/model.rs` 内 `tests` | ✅ 全绿 |
| P0.9 | 契约登记：新常量进尺寸契约、`quick_open/delegate.rs` 纳入尺寸 + 颜色两份扫描清单 | `crates/workbench/tests/ui_contract.rs`、`workbench_shell/src/ui.rs` | ✅（待跑测试确认） |
| P0.10 | 窗口测试（打开即聚焦 / ↑↓ 改选中 / ↵ 副作用 / Esc 关闭） | `crates/workbench/tests/` | ✅ 已落地（4 项，见 §0；随浮层抽实体后落在 `quick_open/tests.rs`） |
| P0.11 | 元数据名称档（跨连接索引，异步 + 防抖 + 过期丢弃） | `database::nav_jobs` + 宿主泵 | ⬜ 第二刀 |
| P0.12 | 后台搜索**双消费方分流**（导航与 Quick Open 不再互抢结果） | `crates/database/src/nav_jobs.rs`（`consumer` 维度） | ⬜ 第二刀前置 |

**本刀的行为变化（要记住）**

- 结果区由手搓 `div` 行换成 `List` 组件：鼠标点击 = 确认（Ctrl+点击 = 保留面板），hover / 选中 / 滚动由组件负责；
- 分析库表行**暂时下线**（原来只有展示、没有动作）：先保证「每一行都有真实动作」，随第二刀的元数据源与属性面板动作一起回归；
- 选中权威从「下标」改为**业务键**（`conn:{ix}:{name}` / `cmd:{label}`），异步回填不会顶掉用户当前位置；
- `Esc` 会先给输入框一次机会（`clean_on_escape` 默认关闭，单行输入无事可清 → 冒泡到浮层关闭）。

## 1. 现状盘点（开发前）

| 维度 | 开发前 | 现在 |
| --- | --- | --- |
| 结果区 | 手搓 `div` + `on_mouse_down`，无 hover / 选中态 | `List` + `ListDelegate` |
| 键盘 | 无（只能鼠标点） | ↑↓ / ↵（三态）/ Esc |
| 焦点 | 打开后不聚焦，要先点输入框 | 打开即聚焦 |
| 数据源 | 连接 / 命令 / 分析库表（表无动作） | 连接 / 命令（表下线待接线） |
| 命中 | `contains` 过滤、无高亮 | 分档评分（相等 > 前缀 > 词首 > 子串）+ 高亮 |
| 命令表 | `view.rs` 硬编码 | `quick_open/model.rs` |

## 2. 本轮决策（可改）

| # | 决策 | 理由 |
| --- | --- | --- |
| D1 | `LIKE` 转义**不在本模块重复实现** | `engine::persistence::MetadataCacheOps::like_escape` 已有实现 + 单测（`search_index_matches_infix_and_escapes_wildcards`），重复一份必然漂移 |
| D2 | FTS5 查询词清洗**归 `engine` 的 `search_fts`** | 现状是 `format!("{}*", query)` 裸拼，用户输入 `"` `*` `NEAR` 会破坏 `MATCH` 语法；修在它自己的 crate，第二刀随 FTS 接线一起做 |
| D3 | 键盘**不新增全局绑定** | 单行 `Input` 对 `MoveUp` / `MoveDown` / `Escape` 都没有处理器（多行模式才注册），Action 会沿焦点链冒泡到浮层根；`Enter` 直接给 `InputEvent::PressEnter{secondary, shift}`，三态白拿 |
| D4 | 选中用**业务键**，组件索引只是渲染锚点 | 异步回填 / 结果重算不抢用户位置；镜像期用 `syncing_from_host` 守卫，避免「更新正在被更新的实体」 |
| D5 | 双消费方分流放**第二刀前置** | `nav_jobs` 是单队列 + 单结果槽，导航与 Quick Open 同时搜会互抢；不先分流，元数据接进来就是 bug |

## 3. Phase 0 剩余（第二刀）任务表

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P0.11a | `nav_jobs` 增消费方维度（`consumer` 或独立结果槽），导航侧同步改 | `crates/database/src/nav_jobs.rs`、`nav_view.rs` | ✅ 已落地（见 §0） |
| P0.11b | 宿主泵：防抖 150ms（`QUICK_OPEN_SEARCH_DEBOUNCE_MS`）、只在词变时发、按 `SearchResult.query` 丢弃过期批次 | `view.rs` / `quick_open/` | ✅ 已落地（见 §0） |
| P0.11c | 元数据行（表 / 视图 / 列 / schema）+ 面包屑归属 + 「为什么命中」标签 | `quick_open/model.rs`、`delegate.rs` | ✅ 行与归属已落地；面包屑单独一行与「为什么命中」标签待做（见 §0） |
| P0.11d | 命中动作接属性面板（复用 `nav_search_hit_property` 的映射口径） | `view.rs` | ✅ 已落地（映射口径对齐，见 §0） |
| P0.12 | 窗口测试补 P0.10 | `crates/workbench/tests/quick_open_window.rs` | ✅ 已落地（4 项，见 §0） |

## 4. Phase 1 / Phase 2 概要

- **Phase 1**：FTS 接线（`sync_fts_index` 跟随 schema 级内省）+ `#` 全文档模式 + snippet 行 + 截断提示「还有 N 条」+ 草稿箱文件源 + 命令注册（跨 crate 登记）。
- **Phase 2**：最近使用 / 空输入态、`@` 当前连接限定、无结果转移入口、焦点恢复与移交。

## 5. 测试场景

| # | 场景 | 状态 |
| --- | --- | --- |
| T1 | `Ctrl+P` 打开 → 输入字符进入输入框（打开即聚焦） | ⬜ 窗口测试 |
| T2 | ↑↓ 改变选中，↵ 执行 → 断言副作用 | ⬜ 窗口测试 |
| T3 | Esc 关闭；点击遮罩关闭 | ⬜ 窗口测试 |
| T4 | `>` 只出命令；删掉前缀回默认 | ✅ 单测（`build_groups`）+ ⬜ 窗口 |
| T5 | 元数据异步：旧 `query` 批次被丢弃 | ✅ 实现（`pump_quick_open` 按 `query` 比较）+ ⬜ 窗口测试 |
| T6 | 无内省缓存的连接不建缓存文件 | ✅ 沿用导航的门（`cache_file_exists`）+ ⬜ 窗口测试 |
| T7 | 无匹配文案随模式（`无匹配命令` / 无匹配全文 …） | ✅ 实现（delegate `render_empty`） |
| T8 | 单字符门槛：1 字符不发元数据搜索、给提示 | ✅ 单测（`async_ready`）+ 提示层 |
| T9 | 命令键唯一且稳定（选中跟随依赖它） | ✅ 单测 |

## 6. 风险

| # | 风险 | 处置 |
| --- | --- | --- |
| R1 | `gpui-kit` 的 `List` 要求**同行同高**（只量一个样本行） | 本轮所有行统一 24px（两行式 snippet 行与 `#` 模式一起设计，届时整模式统一切高） |
| R2 | 输入框与列表**焦点之争** | 焦点恒在输入框；列表不抢焦点（`selectable(true)` 但从不由宿主 focus） |
| R3 | `nav_jobs` 单槽互抢 | 第二刀前置 P0.11a |
| R4 | 大库 `LIKE` 全表扫（缓存库无索引） | 第二刀记录耗时；前缀/中缀两段式或 trigram 见原型设计 §18.2 |
| R5 | 断电 / shim 损坏等环境问题挡住验证 | 见 §7 绕行命令 |

## 7. 验证命令

```bash
# 常规
cargo check -p rds-workbench -j 2
cargo test  -p rds-workbench --lib quick_open -j 2
cargo test  -p rds-workbench --test ui_contract -j 2

# 环境异常：rustup shim（~/.cargo/bin/*.exe）损坏时报 “Permission denied”，
# 绕过 shim 直接用工具链二进制（路径按本机实际）
RUSTC=<toolchain>/bin/rustc.exe RUSTDOC=<toolchain>/bin/rustdoc.exe <toolchain>/bin/cargo.exe \
  test -p rds-workbench --lib quick_open -j 2
```

> 注意：`crates/mock` 若有**在改未完成的编辑**，整仓（含 workbench）都编不过——先让 mock 回绿再跑上面命令。

## 8. 实现位置映射表（设计决策 → 代码）

| 设计决策 | 落点 |
| --- | --- |
| 模式前缀解析（`>` / `#`）与最小词长门槛 | `crates/workbench/src/quick_open/model.rs::parse` / `Query::async_ready` |
| 命令表（唯一权威） | `crates/workbench/src/quick_open/model.rs::command_rows` |
| 匹配评分与命中区间 | `crates/workbench/src/quick_open/model.rs::{match_span, rank, filter_ranked}` |
| 结果行 / 分组头 / 空态渲染 | `crates/workbench/src/quick_open/delegate.rs` |
| 浮层本体（输入 / 结果 / 键盘通道 / 防抖 / 回填 / 关闭） | `crates/workbench/src/quick_open/palette.rs` |
| 动作端口与宿主装配 | `palette.rs::QuickOpenHost` + `view.rs::WorkbenchQuickOpenHost` / `render_quick_open` / `execute_quick_open_action` |
| 选中锚点（业务键）与镜像守卫 | `delegate.rs`（`selected_key` / `begin_host_sync`）+ `view.rs::mirror_quick_open_selection` |
| 打开 / 关闭 / 聚焦 / 输入订阅 / 键盘 | `crates/workbench/src/view.rs`（`toggle_quick_open` / `ensure_quick_open` / `refresh_quick_open` / `move_quick_open_selection` / `quick_open_confirm` / `render_quick_open`） |
| 动作执行（含关面板语义） | `crates/workbench/src/view.rs::execute_quick_open_row` |
| 尺寸常量 | `crates/workbench_shell/src/ui.rs`（`QUICK_OPEN_*`） |
| 契约登记 | `crates/workbench/tests/ui_contract.rs`（尺寸数值 + 两份扫描清单） |
| 元数据名称档（第二刀） | `crates/database/src/nav_jobs.rs` + `crates/database/src/cache.rs::search_index` |
| 元数据全文档（Phase 1） | `crates/engine/src/persistence/metadata_cache.rs::{sync_fts_index, search_fts}` |
