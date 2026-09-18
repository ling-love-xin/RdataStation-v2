# Quick Open · 开发方案（Phase 0–2）

> 状态：**Phase 0 第一刀已落地（2026-09-18）**；第二刀（元数据名称档）待开工
> 关联：`quick-open-prototype-design.md`（原型设计 = 权威规格）、`quick-open-prototype.html`（交互稿）、`../layout/layout-design.md` §2.1/§3.3（入口承诺）
> 技术栈：gpui-kit 0.6.1；组件只从组件库取（禁止手搓）；取色零裸 hex；结构尺寸只引用 `crates/workbench_shell/src/ui.rs`
> 说明：本文件记录**做什么、做到哪**；「长什么样」看原型设计，「为什么这样设计」待 `quick-open-architecture.md`

## 0. 进度记录（最近在前）

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
| P0.10 | 窗口测试（打开即聚焦 / ↑↓ 改选中 / ↵ 副作用 / Esc 关闭） | `crates/workbench/tests/` | ⬜ 待补 |
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
| P0.11b | 宿主泵：防抖 150ms（`QUICK_OPEN_SEARCH_DEBOUNCE_MS`）、只在词变时发、按 `SearchResult.query` 丢弃过期批次 | `view.rs` / `quick_open/` | 连打 10 个字符只发 1–2 次后台搜索 |
| P0.11c | 元数据行（表 / 视图 / 列 / schema）+ 面包屑归属 + 「为什么命中」标签 | `quick_open/model.rs`、`delegate.rs` | 搜 `ord` 能命中并打开属性面板 |
| P0.11d | 命中动作接属性面板（复用 `nav_search_hit_property` 的映射口径） | `view.rs` | ↵ 打开属性面板并定位对象 |
| P0.12 | 窗口测试补 P0.10 | `crates/workbench/tests/quick_open_window.rs` | 4 个场景全绿 |

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
| T5 | 元数据异步：旧 `query` 批次被丢弃 | ⬜ 第二刀 |
| T6 | 无内省缓存的连接不建缓存文件 | ⬜ 第二刀（沿用导航的门） |
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
| 选中锚点（业务键）与镜像守卫 | `delegate.rs`（`selected_key` / `begin_host_sync`）+ `view.rs::mirror_quick_open_selection` |
| 打开 / 关闭 / 聚焦 / 输入订阅 / 键盘 | `crates/workbench/src/view.rs`（`toggle_quick_open` / `ensure_quick_open` / `refresh_quick_open` / `move_quick_open_selection` / `quick_open_confirm` / `render_quick_open`） |
| 动作执行（含关面板语义） | `crates/workbench/src/view.rs::execute_quick_open_row` |
| 尺寸常量 | `crates/workbench_shell/src/ui.rs`（`QUICK_OPEN_*`） |
| 契约登记 | `crates/workbench/tests/ui_contract.rs`（尺寸数值 + 两份扫描清单） |
| 元数据名称档（第二刀） | `crates/database/src/nav_jobs.rs` + `crates/database/src/cache.rs::search_index` |
| 元数据全文档（Phase 1） | `crates/engine/src/persistence/metadata_cache.rs::{sync_fts_index, search_fts}` |
