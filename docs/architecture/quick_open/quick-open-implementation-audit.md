# Quick Open 现状审计（代码事实 · 差异账 · 缺陷清单）

> 调查方式：只读代码 + 只读文档 + git 历史，**未修改任何源码**。
> 基线：`main` @ `6868db72`（工作树另有 28 处未提交改动，均不在 `quick_open` 源码面，
> `workbench_shell/src/ui.rs` 的未提交 diff 只动了属性面板常量，不影响本文引用的 Quick Open 常量）。
> 撰写日：2026-11-09。

## 0. 为什么要这份文档

已有的三份文档主体形成于 2026-09-19（`quick-open-prototype-design.md` / `quick-open-dev-plan.md` /
`quick-open-showcase.md`），此后代码又走了四步，都没回写：

| 日期 | 提交 | 改了什么 |
| --- | --- | --- |
| 2026-10-25 | `124fe08c` | 新增 Goto 档（`Ctrl+Shift+O`，仅元数据、1000 上限字典序、`↵` 回树定位、`⌥↵` 属性面板） |
| 2026-10-26 | `ff70a82a` | 导航搜索引入 `NameMatchMode`（前缀 / 包含二选一，**取消**「前缀优先、中缀兜底」两段式） |
| 2026-10-29 | `17bd766d` | `quick_open` 纯逻辑下沉 `rds-editor`（workbench 只剩浮层 / 委托 / 宿主装配） |
| 2026-11-08 | `ced5071e` | `editor/quick_open/model.rs` 拆成目录（`types` / `scoring` / `sources` / `groups`） |

所以文档里的路径、匹配口径、上限承诺**都已经不是今天的代码**。本文只做两件事：
把今天的事实写清楚（带 locator），把文档与代码的差、以及读代码时能直接证实的缺陷列成可整理的清单。

---

## 1. 身份与范围

| 项 | 值 |
| --- | --- |
| 对象 | Quick Open（统一检索 / 命令面板），入口 = 标题栏搜索条 / `Ctrl+P` / `Ctrl+Shift+O` |
| 仓库 | `D:\RdataStation\RDS\RdataStation-v2` |
| revision | `main` @ `6868db72`（工作树 28 处未提交，不含 quick_open 源码） |
| 已读 | `crates/{editor,workbench}/src/quick_open/**`、`crates/workbench/src/view.rs` 相关段、`crates/workbench/src/panels/shared.rs`、`crates/database/src/nav_jobs/*`、`crates/store/src/persistence/metadata_cache.rs` 搜索段、`crates/workbench_shell/src/ui.rs`、`crates/app/src/main.rs` 键位段、`crates/workbench/tests/ui_contract.rs`、`docs/architecture/quick_open/*.md`、`.agents/skills/{rds-design,rds-ui-spec,rds-layout,rds-architecture,gpui-kit-dev}/SKILL.md`、`docs/architecture/ui/{ui-design-spec.md,vscode-ui-design-system.md}` 相关段、`docs/architecture/layout/layout-design.md`、`vendor/gpui-component/src/{command,kbd}` 公共 API |
| 未读 | `quick-open-dev-plan.md` 全文（只读了结论段）、`quick-open-showcase*`（宣传件）、浮层的窗口级测试断言细节（只取函数名） |

---

## 2. 一页结论

1. **形态**：`Shared::quick_open` 是唯一权威开关（`Cell<bool>`），浮层是独立视图实体 `QuickOpenPalette`，
   宿主只挂 overlay + 注入动作端口；纯逻辑（解析 / 评分 / 分组 / 上限 / 命令目录）已下沉 `rds-editor`。✅确认
2. **四路数据源**：元数据（后台跨连接索引搜索）、草稿箱文件（后台扁平清单，每次打开取一次）、
   连接（同步读 `Shared::connections`）、命令（`rds-editor` 静态目录 13 条）。✅确认
3. **只有名称档与内容档两种后台查询**，都走 `nav_jobs::enqueue_search` 的 `SearchConsumer::QuickOpen` 槽位；
   两千行以内的结果形态差异全部由 `build_groups` 在本地决定。✅确认
4. **默认档的名称搜索实际是「仅前缀」**（`NameMatchMode::StartsWith` 硬编码），但同文件的注释、
   规格文档 §6.1 / §6.3 仍写「中缀 LIKE / 两段式」。⚠️这是本文最重要的一条差异，也是「搜不到」类反馈的头号来源。
5. **换档不重搜**：`sent_query` 只以「词」为键，`#` / 无前缀之间切换时若词不变（例如整串粘贴 `#渠道`、
   或删掉 `#` 前缀），后台**不会**重新发一次 FTS / 名称查询，旧批次原样换一种行形态上屏。⚠️可由源码直接证实。
6. **搜索范围不可见**：`SearchResult::searched` 被导航面板与元数据搜索页消费，Quick Open **丢弃**；
   后端 `SEARCH_MAX_HITS = 300` 的截断也不回传，Goto 档的 1000 行预算因此永远触发不了「还有 N 条」。⚠️确认
7. **工程面是干净的**：无窗口纯逻辑可单测（12 例）、窗口级测试 12 例、`ui_contract` 已登记尺寸与取色、
   遮罩与列表栈帧的两个既有坑都有注释与回归。✅确认
8. **债务集中在文案与文档**：入口/组头/占位三处口径互不一致，两处源码注释描述的行为已经不存在，
   两个尺寸常量没有任何生产者引用。⚠️确认

---

## 3. 分层与实现地图

```
app/main.rs                键位：ctrl-p / ctrl-shift-o / alt-enter（workbench context）
  └─ workbench/view.rs     开关（Shared::quick_open）+ 挂 overlay + 动作宿主（唯一副作用实现点）
       └─ quick_open/palette.rs    浮层实体：输入 / 列表 / 键盘 / 防抖 / 回填泵
            ├─ quick_open/delegate.rs   List 委托：行快照 → 组件行（选中/hover/空态归组件）
            └─ editor/quick_open/       纯逻辑（无 GPUI 依赖）
                 ├─ model/types.rs      模式前缀解析 / 行与动作 / 组 / 源槽位
                 ├─ model/scoring.rs    命中档位、命中区间、过滤排序
                 ├─ model/sources.rs    索引命中 / 文件 / 连接 → 行
                 ├─ model/groups.rs     分组装配 + 渲染上限 + Goto 排序
                 └─ commands.rs         命令目录（13 条，唯一权威）
```

| 层 | 落点 | 证据 |
| --- | --- | --- |
| 键位 | `crates/app/src/main.rs:247`（`ctrl-p`）、`:249`（`ctrl-shift-o`）、`:253`（`alt-enter` → `QuickOpenLocate`） | 已读 |
| 入口控件 | `crates/workbench/src/view.rs:1889-1927`：`role(Button)` / `w_80()` / `px_2()` / 文案「搜索连接、命令…」/ 右侧 `Ctrl+P` / `.occlude()`（挡标题栏 Drag 命中区） | 已读 |
| 开关 | `view.rs:2356-2365`：`open = !Shared::quick_open.get()`；打开边沿才把 `mode` 落进 `quick_open_mode_pending` | 已读 |
| 共享开关 | `crates/workbench/src/panels/shared.rs:274-275`（字段）、`:418`（初值 `false`） | 已读 |
| 挂载 | `view.rs:2382-2394`：未开返回 `None`；开则懒建浮层并投 `on_opened` | 已读 |
| 宿主端口 | `view.rs:2982-2996`（`WorkbenchQuickOpenHost`，弱引用升级失败 = 静默丢弃） | 已读 |
| 浮层状态 | `palette.rs:47-85`：输入 / 列表 / 选中业务键 / 元数据缓存 / 文件快照 / `sent_query` / `searching` / `search_seq` / 防抖与泵 | 已读 |
| 执行 | `view.rs:2429-2541`：`Action` 全枚举逐支落地，末尾按 `keep_open` 决定关不关 | 已读 |
| 纯逻辑导出 | `crates/workbench/src/quick_open/mod.rs:17-19` 重导 `editor::quick_open::model`；`crates/editor/src/lib.rs:123-125` 公开 `pub mod quick_open` | 已读 |
| 后台搜索 | `crates/database/src/nav_jobs/worker.rs:1550-1581`；消费方分槽 `SearchConsumer::QuickOpen`；`has_pending_search` 在 `:1532-1534`（Quick Open 未使用） | 已读 |
| 索引查询 | `crates/store/src/persistence/metadata_cache.rs:3857-3863`（`search_index`，按 `mode` 分支）；`:2815-2824`（`search_fts`，已接 `fts_match_query` 清洗） | 已读 |
| 常量 | `crates/workbench_shell/src/ui.rs:80-110` | 已读 |
| 契约 | `crates/workbench/tests/ui_contract.rs:252-260`（`delegate.rs` / `palette.rs` 纳入扫描）、`:75-82`（常量值断言） | 已读 |

---

## 4. 主链：一次「打开 → 输入 → 回填 → 确认」

### 4.1 打开

1. 键位/点击 → `toggle_quick_open(Mode::Default | Goto, cx)`：翻转 `Shared::quick_open`，
   置 `quick_open_open_pending = true` 并记下档位（`view.rs:2356-2365`）。
2. 下一帧 `render` → `render_quick_open`（`:2382`）→ 懒建浮层 → `palette.on_opened(mode)`
   （`palette.rs:119-130`）：置 `pending_open` / `focus_pending`、锁 `mode_override`、清选中，
   文件清单**只**在无在途请求时清 `files_requested_for`（避免一次打开发两份清单）。
3. 浮层自己的首帧 `render` → `ensure_entities` → `consume_pending_open`（`:261-284`）：
   清空输入、清 `meta`、**清 `sent_query`**、`searching = false` → `refresh` → 聚焦输入框。
   「打开」是权威刷新边沿：空词也要出默认分组（否则看起来像搜不到）。

**旁路**：已打开时再按另一入口（`Ctrl+Shift+O` 或点标题栏）**不会换档，只会关**（`:2357` 的取反）。

### 4.2 输入

- 每次 `InputEvent::Change` → `refresh`（`:289-359`）：按 `mode_override` 解析词（Goto 档只取词、前缀不切档）
  → 收 `connections` → `build_groups` → 推给 `List` 委托 → `mirror_selection`（`:631-644`，镜像期禁止回写）
  → `schedule_search` → `schedule_file_list`。
- 选中以**业务键**为准（`selected_key`），刷新时键还在就保留，否则落第一行（`:325-340`）——
  异步回填不抢用户位置。
- 占位文案随档切换（`:317-320`）：Goto = 「跳转到对象（表 · 视图 · 例程 · 序列 · 触发器）」，
  其余 = 「搜索对象、命令…（> 命令 · # 全文）」。
- 底部提示行固定文案 `↑↓ 选择 · ↵ 打开 · Shift+↵ 保留面板 · Esc 关闭`（`:751`），
  右侧是「再输入 N 个字符 …」/「元数据搜索中…」/「N 条」（`:668-675`）。

### 4.3 后台搜索（元数据）

| 步骤 | 行为 | 证据 |
| --- | --- | --- |
| 门槛 | 命令档永不发；名称档 ≥ 2 字，内容档 ≥ 3 字（trigram） | `model/types.rs:34-36, 56-61` |
| 去重 | **只比词**：`sent_query == needle` 直接 return | `palette.rs:411-413` |
| 组请求 | 档位 → `SearchKind::{Name, FullText}`；其余参数固定为 `case_sensitive=false` / `regex=false` / `NameMatchMode::StartsWith` / `include_comments=false` / `include_definitions=false` | `palette.rs:431-435, 449-463` |
| 防抖 | 150ms，句柄替换即取消 | `palette.rs:444-447`、`ui.rs:110` |
| 序号 | 每次发送 `search_seq += 1`，回填时序号或词不匹配即丢 | `palette.rs:441-442, 514-516` |
| 回填 | 60ms 泵（关面板降频 400ms）；收下 → `searching=false`、`meta = hits`、置 `rows_dirty` | `palette.rs:481, 499-545` |
| 后端 | 跨连接分块并行；每连接 50、全局 300 截断；每连接命中数受目标顺序影响 | `hits.rs:10-13`、`worker.rs:1150-1195` |

### 4.4 确认（三条互不相同的路径）

| 输入 | 路径 | 动作 | 证据 |
| --- | --- | --- | --- |
| `↵` | 输入框事件 `PressEnter{shift}` → `confirm` 回读列表取动作 | `keep_open = shift`；`Ctrl+↵` 三态**未做** | `palette.rs:237-241, 149-154` |
| 鼠标点击 | 委托在列表更新栈帧内解析好动作 → `confirm_with` | 避免重入回读 panic | `delegate.rs:445-463`、`palette.rs:160-168` |
| `⌥↵` | `QuickOpenLocate` → `confirm_locate` | 默认档 = 回树定位（仅元数据行）；Goto 档 = 属性面板；不可定位行 = 什么都不做 | `palette.rs:195-224` |

动作 → 副作用全表在 `view.rs:2438-2536`；执行完 `keep_open=false` 时宿主与浮层各关一次
（`:2537-2539` 与 `palette.rs:180-182`），重复置同一布尔，无害但冗余。

### 4.5 关闭 / 取消

`Esc`（`palette.rs:763`）、点遮罩（`:716-719`，遮罩与面板是**兄弟**，面板 `.occlude()` 挡掉后方命中区）、
执行完自动关、`Shared::quick_open` 被别处置 false（设置页齿轮 `view.rs:2136/2221`）。
浮层在自己 `render` 里先查开关，关掉后返回空元素（`:648-650`），所以「关」不依赖宿主重绘。

---

## 5. 数据源矩阵

| 组 | 档位 | 数据 | 匹配 | 动作 | 上限 | 证据 |
| --- | --- | --- | --- | --- | --- | --- |
| 元数据（表 · 视图 · 列） | 默认 | 后台索引命中 | 后端**前缀** + 前端再按子串过滤排序 | `ShowProperties`（属性面板）；`⌥↵` 定位 | 单组 8 / 总 50 | `groups.rs:57-61, 149-164` |
| 元数据全文（注释 / 定义） | `#` | 后台 FTS | 后端 `MATCH` + snippet；本地**不再按标题过滤** | 同上 | 单组 8 / 总 50 | `groups.rs:22-35, 221-241` |
| 跳转到对象 | `Ctrl+Shift+O` | 后台索引命中（**排除列**） | 本地 `rank_row` 过滤 + 限定名字典序 | 主动作 `RevealMetadata`；`⌥↵` 属性 | 1000（实际 ≤300） | `groups.rs:36-49, 171-203` |
| 文件（草稿箱） | 默认 | 后台扁平清单，每次打开一次 | 本地，按**相对路径**匹配、按文件名高亮 | `OpenDocument` | 清单由 `FLAT_FILE_LIMIT` 截断并给组头说明 | `palette.rs:391-404, 374-385`、`groups.rs:130-146` |
| 连接 | 默认 | 同步 `Shared::connections` | 本地 | `SelectConnection(ix)` → 选中 + 清导航缓存（**不自动连接**） | — | `groups.rs:114-131`、`view.rs:2526-2530` |
| 命令 | 默认 / `>` | `rds-editor` 静态目录 13 条 | 本地，`title` 与 `match_text`（展示名 + 英文别名）各算一次取优 | 8 类动作 | — | `commands.rs:35-148`、`scoring.rs:26-37` |

---

## 6. 与规格文档的差异账

| # | 文档怎么说 | 代码是什么 | 状态 |
| --- | --- | --- | --- |
| A1 | §6.1「匹配 = 两段式，前缀段 + 中缀 LIKE 回落；前缀命中排在所有中缀之前」（`prototype-design.md:119`），§6.3 定案「名称档继续走中缀」（`:151`） | Quick Open 固定 `NameMatchMode::StartsWith`（`palette.rs:458`），引擎侧 `StartsWith` **只走前缀段**且注释明写「不再做前缀优先、中缀兜底」（`metadata_cache.rs:3848-3851`）。同文件注释还写「名称档（中缀 LIKE）」（`palette.rs:430`） | ⚠️ 文档与代码双向不一致 |
| A2 | §16「不建 crate，落 `crates/workbench/src/quick_open/` 模块」 | 纯逻辑已下沉 `rds-editor`（`editor/src/lib.rs:123-125`），workbench 只留 delegate / palette / 宿主动作。依赖方向新增 `editor → scratchpad`（为一层 `FlatFile → FileObject` 映射，见 `editor/Cargo.toml` 注释） | ✅ 已变更，文档未回写 |
| A3 | §19 映射表：`view.rs → quick_open_results`、`nav_jobs.rs`、`cache.rs`、`metadata_cache.rs`（`prototype-design.md:442-451`） | 今天是 `quick_open/{delegate,palette}.rs`、`nav_jobs/{worker,hits}.rs`、`store/.../metadata_cache.rs` | ✅ 路径全部漂移 |
| A4 | §5 前缀 chip 右侧显示「`#` 元数据全文」 | 有 chip（`palette.rs:676-700`），但 Goto 档用 `⌖ 跳转`，且 chip 只在有档位时出现（默认档无） | ✅ 已做，细节不同于文档 |
| A5 | §4 提示行要给「`›` `>` 命令 · `#` 元数据全文」的发现性提示 | 提示行是 `↑↓ 选择 · ↵ 打开 · Shift+↵ 保留面板 · Esc 关闭`；前缀提示被塞进**占位文案**（输入后即不可见） | ⚠️ 发现性弱于设计 |
| A6 | §18.3「`↵` / `Ctrl+↵` / `Shift+↵` 三态」 | 只有两态（`↵` / `Shift+↵`），`Ctrl+↵` 待编辑器后台打开；`delegate.rs:458-459` 注释亦承认 | ✅ 已知未做 |
| A7 | §10「范围常显：已搜 N 个连接 …」 | `SearchResult::searched` 在 `palette.rs:499-545` **从未被读**；导航面板（`nav_view/actions.rs:687`）与元数据搜索页（`panels/metadata_search.rs:157`）都读了 | ⚠️ 设计有、实现缺 |
| A8 | 「被截的条数不静默丢」（`groups.rs:101-103` 自述原则） | 渲染层截断记账（`hidden`），后端 300 截断**无回传**，Goto 档 1000 预算永不触发 footer | ⚠️ 半截 |
| A9 | §6.2「全文档 = 注释 / 类型 / 定义」 | 组头写「元数据全文（注释 / 定义）」（`groups.rs:29`），实际语料不含定义（视图 / 例程定义未入 FTS，§6.2 自述），且 Quick Open 传 `include_definitions=false` | ⚠️ 组头过度承诺 |
| A10 | §6.2「FTS 读侧已清洗」 vs `model/mod.rs:25-26`「现状是裸拼 `format!("{}*")`」 | 实际已清洗：`search_fts` 调 `fts_match_query`（`metadata_cache.rs:2822`），函数在 `:4488`，带单测（`:5619`） | ⚠️ 源码注释过期 |
| A11 | §15 测试点 9「单组 ≤ 8、总 ≤ 50，截断提示出现」 | 默认档 4 组 × 8 = 32 ≤ 50，所以 `QUICK_OPEN_MAX_ROWS`（总预算）在默认档**永不生效**；只有 Goto 单组能触到总预算 | ✅ 事实、口径需明确 |
| A12 | 入口占位「搜索对象、文件、命令…」 | 入口实际「搜索连接、命令…」（`view.rs:1907`）；面板实际「搜索对象、命令…（> 命令 · # 全文）」 | ⚠️ 三处三样 |
| A13 | `rds-layout` skill 写「面板 = Dialog + Input + 分组列表（`文件 / 表`、`命令` 两组混排）」 | 不是 `dialog::Dialog`，是自绘 overlay（遮罩 + 面板兄弟 + `.occlude()`）；分组是**四组**（元数据 / 文件 / 连接 / 命令）**且多出 Goto 整档** | ⚠️ skill 也过期 |
| A14 | `rds-ui-spec` 记入口「20rem×1.625rem」（26px 高），`vscode-ui-design-system.md:225` 亦按 26px 点评 | 代码 `QUICK_OPEN_ENTRY_HEIGHT = 1.75`（28px），视图与契约都按 28px | ⚠️ 26 / 28 两个口径 |

---

## 7. 缺陷清单（只记录，不改）

分级：**P1 会静默给错结果** / **P2 语义或能力缺口** / **P3 文案与工程债**。

### D1 · P1 · 同一个词换档不重搜，旧批次换皮上屏 ✅确认

- 触发：默认档已对 `渠道` 发过名称搜索 → 光标移到行首敲 `#`（或整串粘贴 `#渠道`）。词仍是 `渠道`。
- 机制：`sent_query` 只存词（`palette.rs:73, 411`），档位不在键里；`refresh` 每次 Change 都调
  `schedule_search`（`:355`），于是 **FTS 从未发出**；`meta` 还是上一批名称命中。
- 后果：`#` 档的组头会显示「元数据全文（注释 / 定义）」，行里的 `why` 却由「标题是否包含词」现算
  （`groups.rs:224-234`）→ 这些行几乎全被标成「名称」，且**不会有任何「只在注释里命中」的行**。
  用户看到的是一份语法合法、语义错误的答案。
- 反向同样成立：在 `#` 档删掉 `#`，FTS 结果会以普通元数据行形态留下（snippet / `why` 被丢掉）。
- 最小修法：把去重键从 `String` 升级为 `(mode, needle)`（或直接让 `sent_query` 存 `Query`），
  顺手把「目标连接集合指纹」一并纳入（见 D2 的第二段）。

### D2 · P2 · 名称档「仅前缀」是硬编码，且 Quick Open 没有匹配模式开关 ✅确认

- 现状：`NameMatchMode::StartsWith` 写死在 `palette.rs:458`；后端只有前缀段（`metadata_cache.rs:3847-3851`）。
- 反差：**同一套通道**的导航面板有「前缀 / 包含」开关（`ff70a82a` 引入），Quick Open 没有；
  本地源（连接 / 文件 / 命令）却是子串匹配（`scoring.rs:63-81` 的 `Substring` 档），于是
  同一次输入里「文件能中缀中、元数据不能」。
- 用户可见后果：敲 `order` 搜不到 `sales_order_2024`（表名中缀），只能靠 `#` 档或导航面板绕。
- 修法二选一：(a) 认下「前缀档」是产品口径 → 改文档 + 改 `palette.rs:430` 注释 + 在空态/组头说明；
  (b) 给浮层加显式档位（如 `?` 前缀或设置项）走 `Contains`，并在 `sent_query` 键里带上它（与 D1 同批）。
- 附带：`palette.rs:439-440` 声称「防抖窗口内开关 / 目标重排后重发同词」，实际被 `:411` 直接拦掉
  ——除非关掉再打开（`:267` 才清 `sent_query`）。这条注释描述的路径在生产里到不了。

### D3 · P2 · 后端 300 截断对用户不可见；Goto 的 1000 行预算不可达 ✅确认

- `SEARCH_MAX_HITS = 300`（`hits.rs:13`）在合并后统一 `truncate`（`worker.rs:1195`）；
  `SearchResult` 不回传「被截了多少」。
- 渲染层只对自己截的部分记账（`groups.rs:104-124`）；Goto 档预算 `1000`（`ui.rs:104`）
  永远大于后端能给的量，所以「还有 N 条」在 Goto 档**只能**出现在 >1000 条的合成测试里
  （`goto_caps_at_one_thousand`）。真实库里 300 之后的对象是静默消失的。
- 修法：`SearchResult` 增 `truncated: bool`/`dropped: usize`，或让浮层用 `searched` 与批次长度判；
  Goto 预算降到 ≤300（或直接取消这个预算，改为「后端上限即产品上限」并在 UI 说明）。

### D4 · P2 · 搜索范围与「为什么搜不到」不可见 ✅确认

- `searched` 被丢弃（见 A7）。三种完全不同的原因在 UI 上长得一样：
  ① 连接从没内省过（无缓存，`cache_file_exists` 门直接跳过）；
  ② 当前项目没有缓存；
  ③ 被 300 截断。
- 修法：提示行/组头常显「已搜 N 个连接」，无命中时加一句「未内省的连接不参与搜索」。
  这是规格 §10 原本就写好的形态，属于「设计了没落地」，不是新需求。

### D5 · P3 · `searching` 会卡在「元数据搜索中…」✅确认

- 触发：敲 `ab`（发批次 1）→ 删成 `a`（`async_ready=false`，`schedule_search` 提前 return，**不清标志**）
  → 批次 1 回填时词已变，被 `:514` 丢弃（也不清标志）→ 再敲回 `ab`（`sent_query` 相同，又被 `:411` 拦）
  → 提示行永久停在「元数据搜索中…」。
- 修法：`schedule_search` 的两个提前 return 分支按 `async_ready` 复位；或让提示行只在有在途 seq 时显示。

### D6 · P3 · 三处组头 / 提示与内容不符 ✅确认

| 处 | 文案 | 实际 |
| --- | --- | --- |
| 默认档元数据组 | 「元数据（表 · 视图 · 列）」（`groups.rs:58`） | `meta_object` 只排除 Catalog，**模式 / 例程 / 序列 / 触发器**都会进这一组（`sources.rs:54-98`） |
| `#` 档 | 「元数据全文（注释 / 定义）」（`groups.rs:29`） | 语料不含定义（见 A9） |
| Goto 档 | 标题不含「模式」（`groups.rs:43`） | `goto_rows` 只排除列，模式行会出现（`groups.rs:171-191`、`goto_kind_order` 还给模式排了序） |

### D7 · P3 · 入口文案没跟上能力 ✅确认

入口「搜索连接、命令…」（`view.rs:1907`）只承诺两路，实际四路；面板占位是正确的第四种说法。
进 Quick Open 之前，用户不知道「元数据 / 草稿箱文件」也在里面。

### D8 · P3 · 入口宽度 / 内距两个常量没有生产者，内距还偏离 4px 网格 ✅确认

| 常量 | 声明值 | 引用情况 | 入口实际 |
| --- | --- | --- | --- |
| `QUICK_OPEN_ENTRY_WIDTH` | 20.0rem（320px） | 只有 `ui_contract.rs:75` 断言 320px，**源码零引用** | `.w_80()`（= 20rem，`view.rs:1896`）——数值巧合相等 |
| `QUICK_OPEN_ENTRY_HEIGHT` | 1.75rem（28px） | `view.rs:1897` `rems(...)` + 契约 28px | 接线正常 |
| `QUICK_OPEN_ENTRY_PADDING_X` | 0.625rem（10px） | **零引用，契约也没登记** | `.px_2()`（8px） |

三点后果：① 改宽度常量入口不会跟、`ui_contract` 仍绿；② 10px 不在 4px 网格上（`rds-design` 明令禁止非 4 倍数间距），而契约根本没管这个常量；③ 入口高度 1.75（28px）与 `rds-ui-spec` 记的 1.625（26px）差 2px，
`vscode-ui-design-system.md:225` 还专门批过 26px「偏窄偏矮」——**三个数各说一套**。

先例：`docs/architecture/theme/theme-tokens-complete.md:308` 记过同款问题（`RESULT_PANE_HEIGHT` 分叉、一份是死值，收敛为一份），照同一流程处理即可。

### D9 · P3 · 源码注释描述的行为已经不存在 ✅确认

| 位置 | 注释说 | 实际 |
| --- | --- | --- |
| `palette.rs:439-440` | 防抖窗口内开关 / 目标重排会重发同词，序号是权威 | `:411` 拦死，重开面板才清（见 D2 附带） |
| `editor/quick_open/model/mod.rs:25-26` | FTS 现状是 `format!("{}*", q)` 裸拼 | 已 `fts_match_query` 清洗（见 A10） |
| `palette.rs:430` | 名称档「中缀 LIKE」 | 前缀档（见 D2） |

### D10 · P2（待实测）· `Ctrl+Shift+O` 两处上下文各绑一个动作 ❓推断

`main.rs:249` 在 `workbench` 上下文绑 `ToggleQuickGoto`；`main.rs:380` 在 `analytics-resource`
上下文绑 `CheckoutSelected`。若资产库面板有焦点且其上下文位于 `workbench` 之内，
gpui 从焦点元素向上派发时会先命中面板的绑定 → 在资产库面板里按 `Ctrl+Shift+O` 是「取回（检出）」
而不是「跳转到对象」。**未实测**，见 §11 验证动作 2。

### D11 · P3 · 已开面板再按另一入口 = 关闭 ✅确认

`toggle_quick_open` 只做取反（`view.rs:2356-2365`）：开着默认档时按 `Ctrl+Shift+O` 只会关掉，
不会切到 Goto 档。VSCode 同键位是「换档 / 关闭」，这里与肌肉记忆不一致。

### D12 · P3 · 空输入即全量列表 ✅确认

空词在 `filter_ranked` 里全通过（`scoring.rs:84-86` 的空词返回 `(0,0)`），所以打开面板立刻列出
全部草稿 + 全部连接 + 13 条命令，再按「标题长度 / 字典序」排（`:11-17`）。
规格 §18.3 的「最近 + 常用」仍是建议项，当前实现就是「全量」。

### D13 · P2 · 命令目录覆盖 13 条，键位面上的动作没全进目录 ✅确认

目录：新建查询 / 新建笔记 / 新建文件 / 打开草稿箱 / 打开数据库导航 / 打开资产库 / 打开插件 /
打开洞察 / 打开 Mock / 打开历史 / 打开设置 / 完全隐藏侧边栏 / 恢复侧边栏（`commands.rs:35-129`）。
而 `main.rs` 里另有：`Ctrl+S` 保存、`Ctrl+Shift+S` 另存为、`Ctrl+O` 打开文件、`Ctrl+W` 关闭文档、
`Ctrl+Shift+F` 格式化、`Ctrl+Shift+P` 切换项目、`Ctrl+Shift+W` 关闭项目、`Ctrl+Shift+L` 主题、
`Ctrl+Shift+B` / `F8` 书签、`Ctrl+/` 注释、`Ctrl+Enter` 执行…
→ 用户从「命令面板」找不到「关闭文档 / 另存为 / 切换项目 / 切主题」，只能靠记键位。
（其中一部分依赖编辑器或窗口句柄，`commands.rs:3-14` 对此有明确的取舍说明；
但取舍目前只写在这条注释里，产品口径上没有「命令面板不覆盖编辑器动作」的公开约定。）

### D14 · P2（按项目 UI 规范）· 空态与快捷键提示是手搓的 ✅确认

`gpui-kit-dev` 的选型表写着：空态（含「空搜索结果」）→ `empty::Empty`；可点元素要键盘可达；快捷键用 `kbd::Kbd`。

- 现状 ①：`render_empty`（`delegate.rs:396-421`）是 `v_flex().items_center().gap_1().py_6()` 手搓两行，
  正是 skill 点名的那类「手搓居中图标块 + 标题 + 说明」。
- 现状 ②：入口右侧 `Ctrl+P` 是写死字符串（`view.rs:1914`），提示行 `↑↓ 选择 · ↵ 打开 …` 也是（`palette.rs:751`）；
  vendor 有 `Kbd::binding_for_action_in` / `Kbd::format`（`vendor/gpui-component/src/kbd.rs:60,81,107`），
  可渲染**真实生效的绑定**。命令目录里的 `Ctrl+N` / `Ctrl+,` 同样手写（`commands.rs`）。
- 后果：键位一改，三处提示不会跟；`"ctrl-, "` 这类笔误也没有任何机制能暴露（见 §11 验证 1）。
- 修法：空态换 `Empty`（保留现有四档文案与转移建议，`Empty` 自带居中 + 虚线框 + 图标）；
  入口 / 提示行 / 命令目录的快捷键改为 `Kbd` 取真实绑定。

### D15 · P2（取舍点，非纯缺陷）· `command::Command` 组件没有用 ❓待评估

`gpui-kit-dev` 选型表把「命令面板（搜索框 + 过滤命令列表 + 快捷键提示）」明确指向组件，禁止自实现搜索框 + 过滤列表。
vendor 0.8.1 的实际入口是 `command::{Command, CommandState, CommandItem, CommandGroup}`
（`vendor/gpui-component/src/command/mod.rs:11-13`），能力：分组、`on_query`、`filterable(bool)`、`searchable(bool)`、
`empty`、`header`/`footer`、`max_h`、每项 `action`/`keywords`/`child`（自定义行内容）、Action 快捷键自动渲染。

与当前实现的差（代码有、组件未见明确入口）：① 每行「标题 + snippet + `why` 标签」的三段高亮；
② `⌥↵` 第二条动作通道；③ 以业务键而非下标作选中锚；④ `build_groups` 的本地分组 + 上限 + `hidden` 记账。

结论：**先做能力对照 spike 再决定**（读 vendor 源码 + 最小 demo），三选一：
迁移到 `Command` / 只借它的 QueryState 与键盘漫游 / 登记为与 `tree` 同类的例外并写进 UI 规范。
不做决定就动手改，等于在两种架构之间反复摇摆。

---

## 8. 责任闭合卡（唯一需要的跨组件效果主张）

主张：「Quick Open 的元数据结果只来自**已内省过的连接**，不会顺手建缓存，且被截断的部分不告知。」

| 格 | 内容 | 证据 |
| --- | --- | --- |
| 对象 / 范围 | Quick Open 元数据组（默认档 / `#` / Goto 共用一条通道） | `palette.rs:449-463` |
| 触发者 | `refresh` → `schedule_search`（150ms 防抖，≥2 / ≥3 字） | `palette.rs:289-359, 407-467` |
| 当前装配 / 开关 | 消费方槽位 `SearchConsumer::QuickOpen`；`StartsWith` 硬编码；case / regex / comments / definitions 全 false | `palette.rs:449-460` |
| 实际执行者 | `nav_jobs` worker：跨连接分块并行、per-conn 信号量、合并后全局 300 截断 | `worker.rs:1150-1195` |
| 成功副作用与观察点 | `push_search_result` → 浮层 60ms 泵 `drain_search_results` → `meta` → `build_groups` | `palette.rs:481, 499-545` |
| 失败是否返回且被检查 | 「跳过多少连接」以 `searched` 返回，**浮层不读**（D4）；空结果 → 空态「无匹配结果」 | `hits.rs:152-156`、`palette.rs:499-545` |
| 重试 / 重放来源 | 仅「词变了」重发；同词换档、目标集合变化都不重发（D1/D2） | `palette.rs:411-413, 267` |
| 不能覆盖的对象 | 未内省连接、无缓存项目、被 300 截断的尾部连接、「注释语料缺失」的判断 | 同上 + `groups.rs:221-241` |
| status | **conditional**（「不建缓存」这一半成立；「被截断不告知」这一半是缺陷，不是特性） |

「不建缓存」这一半的直接证据：`hits.rs:152-156` 注明无落盘缓存的连接直接跳过、搜索不建文件。

---

## 9. 测试与契约覆盖

### 已有（均为「走生产入口」，无桩替身式白盒）

- 纯逻辑 12 例（`crates/editor/src/quick_open/model/tests.rs`）：`parse_reads_mode_from_first_char_only`、
  `async_ready_threshold_depends_on_mode`、`match_span_*`、`rank_prefers_exact_then_prefix_...`、
  `build_groups_filters_and_drops_empty_groups`、`file_rows_match_on_name_and_path_but_highlight_the_name`、
  `meta_rows_come_first_and_carry_properties_request`、`searching_note_shows_only_when_query_is_ready`、
  `fulltext_rows_keep_content_hits_and_tag_the_reason`、`markup_segments_splits_highlight_marks`、
  `groups_are_capped_and_report_what_was_hidden`、`command_keywords_match_...`、`command_ids_and_row_keys_...`、
  `goto_mode_lists_only_metadata_and_drops_columns`、`goto_sort_ignorecase_...`、`goto_caps_at_one_thousand...`
- 窗口级 12 例（`crates/workbench/src/quick_open/tests.rs`）：打字到达输入框、↑↓ + `↵` 执行、
  点击行执行、点遮罩不执行、`⌥↵` 派发 `RevealMetadata`、不可定位行 `⌥↵` 无操作、`Esc` 关闭、
  元数据搜索只在 ≥2 字启动、Goto 档锁元数据且无连接行、Goto `↵` 回树定位、Goto `⌥↵` 属性、
  打开时加载草稿清单。
- 契约：`ui_contract.rs` 已把 `delegate.rs` / `palette.rs` 纳入「无裸尺寸 / 无裸颜色」两份扫描。

### 空白（没有对应用例，也正是 §7 缺陷没被发现的原因）

1. 同词换档（D1）——最能说明问题的一条。
2. 过期批次的丢弃路径（`search_seq` / 词不匹配的 `continue`）。
3. `searching` 标志复位（D5）。
4. 后端 300 截断后的可见性（D3）。
5. 项目切换 / 文件清单在途（`files_pending`、`files_requested_for`）。
6. 空输入全量列表（D12）与「最近使用」缺失。
7. 设置页与浮层的互斥（`view.rs:2136/2221`）。
8. 键位上下文冲突（D10）。

---

## 10. 整理建议（建议分批，每批一刀一 commit）

| 批 | 内容 | 风险 | 涉及 |
| --- | --- | --- | --- |
| **① 文档回写（零代码）** | 把 §6/§A 的差异逐条写回 `prototype-design.md`（匹配口径、路径映射、Goto 档、上限口径），`dev-plan.md` 补「P2-c 后分层」 | 无 | docs |
| **② 注释债清零** | D9 三处；顺手把 `palette.rs:430` 的「中缀」改对（无论采用哪种口径） | 无 | 3 处注释 |
| **③ 搜索键升级** | D1：`sent_query: Option<String>` → `Option<SearchKey{mode, needle, targets_fingerprint}>`；补 2 条单测 | 低（浮层内部） | `palette.rs` + `tests.rs` |
| **④ 口径定案** | D2：选「前缀档」或加 `Contains` 档位；同步 A1 / A5 / A11 的文案与文档 | 中（产品决策） | `palette.rs` / docs |
| **⑤ 可见性补齐** | D3 + D4：`SearchResult` 增截断与跳过信息；提示行常显「已搜 N 个连接」；空态给转移入口 | 中（跨 crate 契约） | `nav_jobs` / `palette` / `delegate` |
| **⑥ 文案与常量收口** | D6 / D7 / D8：组头口径统一；入口文案与面板一致；入口宽度改回 `QUICK_OPEN_ENTRY_WIDTH`（或删常量） | 低 | `view.rs` / `groups.rs` / `ui.rs` |
| **⑦ 命令面收口** | D13：决定「命令面板是否覆盖编辑器动作」，并把口径写进文档；要覆盖就补目录项 + 宿主执行支 | 中 | `commands.rs` / `view.rs` |
| **⑧ 键位与开关** | D10 先实测；D11 把 toggle 改成「同档关 / 异档换档」 | 低–中 | `main.rs` / `view.rs` |
| **⑨ 组件选型对齐** | D14：空态换 `empty::Empty`、快捷键改 `Kbd` 取真实绑定（含命令目录三处手写键位）；D15 先出 spike 结论再动 | 低（D14）/ 中（D15 若迁移） | `delegate.rs` / `view.rs` / `commands.rs` |

> 顺序建议：①②⑥ 一批做完（纯文案 + 常量，零行为变化，可一次 commit 一个主题）；
> ③（D1）单独一批，它是唯一会影响用户可见结果的改动，值得单独可回滚；
> ⑤（D3/D4）涉及 `nav_jobs` 契约，单独分支做；④⑨ 属于**产品/规范决策**，先出结论再排期。

不建议现在动的：`Mode` / `Row` / `Group` 的结构（`types.rs`）——它同时被 Goto 档、内容档、
命令目录、12 例单测消费，改动收益低于成本。

---

## 11. 未闭环的三件事与最小验证动作

| # | 未知 | 为什么重要 | 最小验证动作 |
| --- | --- | --- | --- |
| 1 | `main.rs:254` 的 `KeyBinding::new("ctrl-, ", OpenSettings, ...)` 结尾多一个空格，是否仍能绑上 `Ctrl+,`？（Kerystroke 解析是否 trim 未实测，本机没有 gpui 源码副本） | 命令目录对外宣传 `Ctrl+,`（`commands.rs:110`），若绑定失效，命令面板给的快捷键提示是假的 | 启动应用按 `Ctrl+,`：能开设置 = 空格被容忍（但字符串仍是笔误）；不能开 = P2 缺陷 |
| 2 | D10 的键位冲突是否真的发生 | 决定 `Ctrl+Shift+O` 在资产库面板里是否「名不副实」 | 打开资产库面板并聚焦列表，按一次 `Ctrl+Shift+O`，看是「跳转浮层」还是「取回（检出）」 |
| 3 | D1 是否在真机上按预期复现 | 它是本文唯一 P1，但只由源码推得，未跑运行时 | 开面板 → 敲 `ord` → `Home` 后敲 `#` → 观察组头是否立刻变「元数据全文」、行是否出现 snippet 与 `why=内容` |

> 基线测试（2026-11-09 本机跑过）：`cargo test -p rds-editor --lib quick_open` → **16 passed / 0 failed**；
> `cargo test -p rds-workbench --lib quick_open` → **12 passed / 0 failed**（编译期另有 2 条
> 属性面板未使用变量的警告，与本模块无关）。但「测试通过」**不能**当作上面 D1–D13 不成立的证据：
> 现有用例没有覆盖 §9 列出的 8 个空白点，尤其是同词换档。

---

## 12. 批次 ① 已落地（2026-11-09，工作树未提交）

按 §10 的 ①②⑥ 合并执行，6 个源文件，除空态外均为零行为变化：

| # | 改动 | 涉及 |
| --- | --- | --- |
| 1 | **删除** `QUICK_OPEN_ENTRY_PADDING_X`（零引用、契约未登记、10px 不在 4px 网格） | `workbench_shell/src/ui.rs` |
| 2 | 入口宽度改回常量：`.w_80()` → `.w(rems(ui::QUICK_OPEN_ENTRY_WIDTH))` | `view.rs:1896` |
| 3 | D9 三处注释债清零：`palette.rs:430` 名称档改「只查前缀段」（并指明引擎按 `mode` 单段查询）、`palette.rs:439-440` 改为「**同词不重发**，指向 D1」、`model/mod.rs:25-26` 改为「已走 `fts_match_query`」 | `palette.rs` / `model/mod.rs` |
| 4 | D6 三处组头对齐实际数据范围：`#` 档去「定义」；Goto 组头补「模式」；默认档元数据组补 例程 / 序列 / 触发器 / 模式（同步 3 处断言） | `groups.rs` + `model/tests.rs` |
| 5 | D14 空态换 `empty::Empty`（`EmptyMediaVariant::Icon` + `IconName::Search`；四档文案与转移建议保留） | `delegate.rs` |

验证（本机实跑）：

- `cargo test -p rds-editor --lib quick_open` → **16 passed / 0 failed**
- `cargo test -p rds-workbench --lib quick_open` → **12 passed / 0 failed**
- `cargo test -p rds-workbench --test ui_contract` → **9 passed / 0 failed**
- `cargo check -p rds-workbench --lib` 干净（仅 2 条 `panels/property_panel` 未使用变量警告，与本模块无关）

> 踩坑记录：中途 lib-test 报 `PROPERTY_GENERAL_LABEL_WIDTH` 找不到（`panels/property_panel/sections.rs:190`）。
> 根因是**两个 cargo 进程并发**抢同一个 build 目录锁，`rds-workbench-shell` 的旧 `rmeta` 被复用；
> `cargo clean -p rds-workbench-shell` 后消失。与 Quick Open 无关，但验证前应先确认没有在跑的 cargo。

仍未回写：本批改的是代码里的文案，`quick-open-prototype-design.md` §6/§19 与 `rds-layout` skill 的 Quick Open 节
仍是旧口径（A13 / A14）。那部分属于 §10 的 ①（纯文档），建议与这批一起提交或单独一刀。
