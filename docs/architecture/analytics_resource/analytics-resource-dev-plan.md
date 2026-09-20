# 资产库 / 分析存档模块（M6）· 开发方案（Phase 0–5）

> 状态：**设计定稿（2026-09-15）；Phase 0–3 主体 + Phase 2 前八刀 + UI 收尾与第十一 / 十二刀（2026-09-20）已落地**——归档/取回/再归档闭环（**含标签 / 分组 / 别名真的落上**）+ 变更事件 + 索引修复 + 版本历史 / 索引修复 / 回收站 / 标签 / 分组五个对话框与组织入口、五个排序键、三个设置项、**搜索匹配面（显示名 / 别名 / 标签 / 来源表 / 尾部）**、**拖拽行到分组头**均可用，**140 单测 + 30 窗口测试全绿**（详见 §0 进度记录；行态口径对齐见第九刀的 V 表） · 关联文件：`analytics-resource-architecture.md`（语义裁决与数据流）、`analytics-resource-prototype-design.md`（原型与交互规格）、`analytics-resource-prototype.html`（交互稿）、`README.md`（模块入口）
> 前置：v1 行为蓝本 `v1/backend/src/core/persistence/analytics_resource_store/`（9 文件 2237 行）+ `v1/docs/backend/ANALYTICS_RESOURCE_MANAGER_DESIGN.md`；v1 前端 `v1/frontend/extensions/builtin/analytics-resource/`（**仅占位卡片列表**，见 `analytics-resource-prototype-design.md` §10）
> 上游：`../scratchpad/scratchpad-dev-plan.md` Phase D（归档/取回 D1–D6，本方案是其落点的另一半）
> 复用 `connection-dev-plan.md` / `scratchpad-dev-plan.md` 的推进方式：Phase 划分 → 文件落点 → 验收 → 测试场景 → 风险
> **范围**：分析存档的归档/取回/登记/版本/组织/检索/回收站/索引修复。**不含**连接与内省（M3/M4）、工作区文件读写（M5）、DuckDB 计算（M2）、Mock 生成（M7）、洞察计算（M8）、项目级→系统级提升（M1）。

## 0. 进度记录（最近在前）

### 2026-09-20 — Phase 2 第十二刀（P2.2 收口）：拖拽行到分组头

| V | 做什么 | 落点 | 为什么 / 影响面 |
| --- | --- | --- | --- |
| V18 | **行可拖**：`ArchiveDragPayload { ids, kind, label }`（多选时带**整选集**）+ 幽灵 `ArchiveDragGhost`（形状给种类、颜色中性、22px 与 M4 幽灵同高）；只读项目不给拖 | 新增 `src/dnd.rs`、`src/resource_view.rs::render_row` | 拖拽是「移动到分组」的**第二条入口，不是第二套实现**：载荷不带动作（落点决定语义），出口仍是宿主 `request_move_to_group` |
| V19 | **落点只有分组头**：`group_drop_target(key)` → `NotATarget`（「全部分组」聚合头）/ `Ungroup`（未分组头）/ `Into(id)`；悬停高亮 `list_active`（与选中底同 token）；`rows_to_move` 只发**真的要改归属**的行（拖到原分组 = 无操作） | `src/dnd.rs`、`src/resource_view.rs::{render_group_header, drop_rows_onto_group}` | 与 M4 的两处有意差别：不做「拖到某一行之前」（资产库没有手工排序，做了就是承诺一个做不到的语义）；「全部分组」不接（落上去「移出分组」与「什么都不做」都说得通） |
| 测试 | +3 单测（幽灵文案 / 落点映射含聚合头 / 只发要改的行与顺序）、+2 面板窗口测试（拖到分组只发变更行、聚合头与只读项目不发） | `src/dnd.rs`、`tests/panel_window.rs` | 「鼠标扫过一行分组头不该产生一次写库」是这条路径最容易做错的地方 |
| 验证 | `cargo test -j 2 -p rds-analytics-resource` → **140 单测 + 21 面板窗口 + 9 对话框窗口全绿** | — | 基线 137 / 19 / 9 |

### 2026-09-20 — Phase 2 第十一刀（P2.3 余项）：搜索匹配面（别名 / 标签 / 来源表）

| V | 做什么 | 落点 | 为什么 / 影响面 |
| --- | --- | --- | --- |
| V16 | **匹配面加宽**：`ArchiveRow` 的 `tag_ids` 换成 `tags: Vec<ArchiveTagChip>`（同一份数据两处用：**筛标签维比 id，搜索比名字**），并加 `alias` / `source_table`；新增纯函数 `filter::search_haystack`——把「显示名 · 别名 · 来源表 · 标签名 · 尾部字段」拼成一个**已小写化**的串，`matches` 只剩一次 `contains` | `src/resource_view.rs`、`src/filter.rs`、`src/present.rs::to_row` | 原型 §2.2 要的“显示名 / 别名 / 标签 / 来源表”到齐；**匹配面比展示面宽**（别名 / 标签 / 来源表行上不显示，面板只有 240px），与 quick_open 的 `keywords` 同口径：**可搜、不高亮**。字段间留空格 → 堵住跨字段偶然连缀的假命中；装配一次而不是逐字段 `format!` → 搜索是逐行调用的 |
| V17 | **“能搜什么”写在搜不到的那一刻**：`filter::SEARCH_FIELDS`（人读清单 / 单一来源）+ `no_match_hint(query)`；无匹配空态的副文案改由它给（有搜索词 → 列出可搜字段；纯筛选无匹配 → 指清筛选） | `src/filter.rs`、`src/resource_view.rs::render_no_match` | 匹配面加宽后能力仍然**不可见**（三个新字段都不在行上）：搜不到的那一刻正是用户怀疑“存档是不是没了”的时刻。同时消掉渲染层自己编文案的旧例 |
| 顺带 | 详情面板的标签 chips 改用行上那一份（`to_detail(resource, status, count, &row.tags)`），不再各装配一遍 | `src/present.rs::build_snapshot` | 同一装配来源，不会出现“行里有、详情里没有”；少一次逐行克隆 |
| 测试 | +4 filter 单测（别名 / 标签名 / 来源表命中与大小写、haystack 保名与尾、`SEARCH_FIELDS` 与实现一一对应、空态文案分流）、+1 present 单测（装配带上别名 / 来源表）、+1 面板窗口测试（面板里搜别名 / 标签名 / 来源表真能录到行） | `src/filter.rs`、`src/present.rs`、`tests/panel_window.rs` | 清单与实现同步靠 `search_fields_list_covers_what_the_haystack_really_matches` 铉住——“说明了却搜不到”比不说更糟 |
| 验证 | `cargo test -j 2 -p rds-analytics-resource` → **137 单测 + 19 面板窗口 + 9 对话框窗口全绿**；`cargo check -j 2 --workspace --all-targets` 无错 | — | 基线：132 / 18 / 9 |

> 与第四刀的连接：那一刀把排序做到“比原始值而不比尾巴文案”，本刀把搜索做到“比匹配面而不比展示面”——同一个判据的两面（**用户看到的**与**程序比较的**可以不是一回事，但都要单一来源）。
>
> 搜索**不高亮**是当前口径：匹配面里的别名 / 标签 / 来源表不在行上，高亮无处落；将来若做高亮，也只高亮行上有的字段（名称 / 尾巴）。

### 2026-09-20 — Phase 2 第十刀：批量打标签（多选解锁的第二个动作）

| V | 做什么 | 落点 | 为什么 / 影响面 |
| --- | --- | --- | --- |
| V13 | **行右键加「编辑标签…」**（多选时「编辑标签（N 项）…」，作用于整选集）；`ResourcesHost::request_edit_tags` 改收 `&[TagTarget]`（与删除 / 移动同一口径），详情面板的「＋ 标签」走一元切片 | `src/resource_view.rs`、`src/detail_view.rs`、`src/model.rs::TagTarget` | 之前打标签只在详情面板（且只收单 id），列表里多选后无批量入口；文案抽成纯函数 `tag_entry_label`，选集→目标抽成 `tag_targets`（都有单测：菜单内部读不到，可断言的那半要拎出来） |
| V14 | **对话框支持一批目标**：`TagDialogSeed` 加 `target_count` / `partial`；状态加三态（全有 / 部分 / 无）+ `promote_partial` / `set_checked_three_way` / `reset_batch`，`diff()` 的 remove 侧并入开窗时的「部分」集 | `src/dialogs/tag.rs` | 「部分」是批量特有的态：`Checkbox` 没有半选，用一枚小字说清楚；**点一下 = 让所有目标都有**（布尔翻转会把“本来部分有”的推成“无” = 没要求的批量摘除） |
| V15 | **取数与执行按批**：`TagListJob.targets` → 逐目标取已挂标签算**交集（全有）/ 并集 − 交集（部分）**；`TagActionJob.targets`，`Apply` 跑目标 × 标签两层循环（逐条改、不做预回滚，报“改了 N 处”）；`CreateAndTag` 给全选集打上；会话按 `targets` 认自己（换批重开） | `crates/workbench/src/services/resource_jobs.rs`、`components/resource_host.rs`、`panels/{mod,shared,resources}.rs` | 与批量删除 / 批量移动同一形态（作业收切片、回执带数量）；宿主侧取数是唯一要改的 I/O 点（面板仍然不碰库） |
| 测试 | +3 对话框单测（三态提升 / 取消、三态勾选框、`reset_batch` 基准）、+2 面板单测（`tag_targets` 单选与多选、菜单文案）；既有标签测试全过 | `src/dialogs/tag.rs`、`src/resource_view.rs` | 重点铉住“**送了几项**”（漏送 = 只改了用户点中的那一条）与“部分 → 取消要真摘掉” |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **132 单测 + 18 面板窗口 + 9 对话框窗口全绿**；`cargo check -j 2 --workspace --all-targets` 无错 | — | — |

> 批量移动不必再做：它随第三刀的「移动到分组 ›」已经对整选集生效（`selection_ids`），本刀只把注释里“多选只解锁删除”那句旧话改掉了。

### 2026-09-20 — Phase 2 第九刀（UI 收尾）：行态口径对齐 + 共用原语接入 + 组件换装

> 本批起给改动**编 V 号**（V1–V12，与 `database-nav-dev-plan.md` §0 的 V 表同一体例），便于与导航侧（V11 行态统一 / V14 共用原语）逐条对照追溯。本批**只动视图层**，与业务 / 数据层无关。
>
> **行态口径的权威表**：`docs/architecture/theme/ui-constraints.md` §8.3（2026-09-20 起收成单一来源：展开指示 / 悬停与选中优先级 / 选中底 token / 激活条 / 缩进 / 行高预算 / 截断 / 命中区 / 类别与状态 / 动效）。本批与导航、草稿箱、Mock 对齐的就是那张表；以后再遇同类问题**改那一处**，不要在面板文档里各拍一次。

| V | 做什么 | 落点 | 为什么 / 影响面 |
| --- | --- | --- | --- |
| V1 | 分组头展开指示接共用原语：字符 `▸/▾` → `tree::disclosure_slot()` + `tree::disclosure_icon()` | `resource_view.rs::render_group_header` | `workbench_shell::tree` 头注点名的「第三处」就是这里（M4 导航 / M5 草稿箱 2026-09-20 已换，这不再是产品决策，是对齐已定口径）；10px 槽宽是缩进算式的一部分，换载体不会让标题左边缘漂 |
| V2 | 分组头色条改 `primary` + 圆头 `0.875rem`（与 M4 分组头同形同角色） | 同上 | 两个 token 在 `rds-theme.json` 里明暗同值（`#C25B46` / `#E8846F`）→ 零视觉变化；几何对齐原型 §2.4「沿用 M4 分组头做法」 |
| V3 | 列表行距收成 `ui::ROW_HEIGHT`（24px）：两处 `ListItem` 统一走新增的 `list_row()`（压掉组件默认的 `py_1`） | `resource_view.rs::{list_row,render_group_header,render_item}` | **修一处尺寸偏差**：`List` 只量一个样本行定全局行距，而 `ListItem` 自带的 `py_1`(4px) 让每项实际 32px（规格 24px，导航侧行高是精确值）；同时消掉多选行自绘 `list_active` 底与悬停 `list_hover` 上下露出的光晕 |
| V4 | 分组头键盘可达：停在头上按 Enter 折叠 / 展开（纯函数 `header_fold_key` + 委托记 `focused_header`） | `resource_view.rs::{header_fold_key,set_selected_index,confirm}` | 头是自绘可点行（鼠标能折、键盘不能）；走 `List` 自己的 `confirm` 通道（Enter 由组件路由），不新增自绘焦点 / 不破坏虚拟化 |
| V5 | 撤销栏「撤销」自绘 `div` → `Button::ghost().xsmall()`（栏高仍 24px：`py_1` → `py_0p5`） | `resource_view.rs::render_undo_bar` | 撤销是**唯一**入口（`Ctrl+Z` 未绑），自绘 div 没有 hover / 焦点 / 键盘；`Button` 自带 `track_focus` + `tab_stop`，Enter/Space 可激活 |
| V6 | 加载骨架手搓灰条 → 组件 `Skeleton` | `resource_view.rs::render_loading` | 「过程进行中」给现成组件（自带 2s 呼吸、只改透明度、`reduce_motion` 下停在全亮）；只挂**暂态**（首帧取数期间），列表已有行时不摆它 |
| V7 | 详情面板「复制」（指纹）与 chip 上的 `×` 自绘 div → `Button::ghost().xsmall()` | `detail_view.rs::{render_detail,render_tag_section}` | 可点元素键盘可达 + 命中区 20px 下限；`archive-tag-remove-{id}` / `archive-detail-copy-hash` 仍可用调试选择器 / id 定位；chip 高 20→24px |
| V8 | 版本历史行 → `list::ListItem`（hover / 选中由组件承担） | `dialogs/version.rs::version_line` | 原先手搓的 `when(selected, bg) + hover(bg)` 那对**悬停会把选中底盖掉**（违反导航侧 V11 的行态口径，草稿箱侧同批修过）；换组件后由构造保证，列内距 `px_2` 对齐表头 |
| V9 | 标签行自绘 `✓` → 真 `Checkbox`；勾选底 `accent.opacity(0.3)` → `list_active`；悬停不覆盖勾选 | `dialogs/tag.rs`（含新增 `TagDialogState::set_checked`） | 对齐 `dialogs/pick.rs` 既有写法；`✓` 字符既无键盘焦点也没把「勾上 / 未勾」交给 a11y（只剩一个文本节点）；勾选底统一到与面板 / 其它对话框同一个「选中底」角色 |
| V10 | 回收站行去掉 hover | `dialogs/trash.rs::trash_line` | 这一行本来就不是可点行（动作全在行内两个按钮上），悬停变色是「这里能点」的承诺；与草稿箱「引用行 / 回收站行不做 hover」同口径。原先那层用的是 `accent.opacity(0.3)`（品牌淡底），与行态 token 不是一回事 |
| V11 | 固定描边统一 `ui::HAIRLINE`（10 处 `px(1.0)`） | crate 内 4 个视图文件 + `ui.rs` 把 `HAIRLINE` 加进重导出 | rds-ui-spec 硬约束（固定描边走 `ui::HAIRLINE`，与 editor / settings 同写法）；零视觉差、无新常量 |
| V12 | 顺手清掉分组头那段「同一句写两遍」的注释残余 | `resource_view.rs::render_group_header` | 上一批合并留下的残迹（一份还在描述已改名的 `GROUP_BAR_WIDTH`） |

**验证**：`cargo test -p rds-analytics-resource -j 2` → **127 单测**（+2：`header_fold_key` 的「头上 Enter = 折叠 / 行上 = 打开 / 越界不 panic」与 `TagDialogState::set_checked` 的按值写 + 幂等）+ **18 面板窗口 + 9 对话框窗口**全绿（面板窗口新增断言：两个分组头之间**每项正好 24px**，钉住 V3 的行距；对话框窗口新增断言：标签行真的摆了 `Checkbox`）；`cargo test -p rds-workbench --test ui_contract -j 2` 7 项、`cargo test -p rds-database --lib -j 2` 70 项、`cargo check --workspace --all-targets -j 2` 均通过。

**本批「不改」（有意，别为了「统一」再动它们）**：

| 候选项 | 为什么不改 |
| --- | --- |
| `tree::active_bar`（选中行左侧 2px 条） | 分组头那根是**类别条**（原型 §2.4），不是选中条；而行的选中已由 `List` / `ListItem` 承担（`list_active` 底 + 1px 框）——接上 `active_bar` 会同时叠出两套选中语义 |
| `tree::indent_rem` / `indent_spacer` | 分组头是**固定两级**（`depth ∈ {0,1}`，见 `filter::build_visible_items`）的局部内距，不是 `depth × 步长` 的树缩进；套公式（0.5 + 0.875×depth）会把 depth=1 的头一次右移 14px（视觉回归） |
| 行尾「相对时间 → 悬浮显示绝对时间」（原型 §2.3；`present.rs` 两处注释都留了「悬浮由渲染层补」） | 0.6.1 的 `.tooltip()` **只挂在组件上**（`Button` / `Switch` / `Checkbox` / `Radio` …）；div 级 tooltip 要用 gpui-base 的 `TooltipOverlay` 亲手接 overlay + 触发（全仓零使用）——属「工具提示基建」一批，不在本轮 |
| `List` 的 `px_3` 行左内距（导航自绘行是 `px_1` = 4px） | 组件自身的行内距约定；改了要连分组头色条位置一起动，收益只是「与导航像素级一致」 |
| 行内 hover 动作（原型 §2.3 的 hover 版） | 原型把它归给详情面板批，仍是未落地功能；另：鼠标在行内时指针已在行内，不挂 hover 也够用 |

**遗留（本批新增）**：行尾绝对时间 tooltip（等工具提示基建）；`version` / `tag` / `trash` 三个对话框的行仍未虚拟化（`MAX_*_ROWS` 上限 + 明说不静默截断，行数真成问题时再上 `List`）；V4 的 Enter 路径覆盖到「纯函数 + 既有折叠用例」，**键盘真机验收待过**（与导航侧 §2.6 的真机清单同性质：仓库已记载 `simulate_click` 全套跑不可靠，故不写点击 / 按键模拟断言）。

### 2026-09-19 — 文档：一页看懂按**编辑器宣传页体例**重写（两版孪生）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 体例对齐 ✅ | 对齐 `editor-showcase.*` 的 16 节体例：一句话 + 「它替你做到的事」/ v1 对照（六条「看着有、实际是假」的底座）/ 三种类型与复现强度 / 三个高光 / **四个剧本**（每步都能照着做）/ 归档全流程（ASCII + 落点表）/ 版本语义 / 面板与详情骨架（ASCII）/ 组织与检索 / 八个对话框 / 回收站与索引修复 / 只读三重守卫 / 架构分层 + 22 个端口 + 两条纪律 / 质量与证据（套件表）/ 边界（不做与还没做）/ 文档地图 | `analytics-resource-showcase.{md,html}` |
| 视觉版 ✅ | 吸顶导航（16 锚点）+ Hero（首屏是**面板与详情线框**）+ 剧本卡（编号步骤）+ ASCII 流程图与骨架 + 守卫表 / 缺陷卡；单文件、明暗两套、零外链、带打印样式 | `analytics-resource-showcase.html` |
| 可贴版 ✅ | 纯文本 + ASCII 骨架（不再依赖 mermaid 渲染），适合贴进 PR / wiki / 聊天；两版章节一一对应 | `analytics-resource-showcase.md` |
| 新增两处真实证据 ✅ | ① 归档对话框的标签 / 分组曾全链路没人消费（“白填”，已修）；② 同批重名标签撞唯一索引（写测试时踩到，已改本地缓存）——两条都写进「开发期当场抓出来」栏 | 同上 |
| 文档地图 ✅ | 三处同步：模块索引（可贴版 / 视觉版分行）· 模块入口 §6 与顶部一行 · 使用手册顶部「第一次接触先看一页看懂」 | `docs/architecture/README.md`、`README.md`、`analytics-resource-user-guide.md` |

### 2026-09-18 — Phase 2 第八刀：归档的「标签 / 分组 / 别名」真的落地（修一处静默丢弃）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| **修静默丢弃** ✅ | 发现：`ArchiveRequest.{tags, group_id}` **全链路没人消费** —— 归档对话框里填的标签填了等于白填（本体与登记行落好了，附属项没有）。现在归档（首次 / 再归档 / 幂等分支）都会把这两项落到位：标签**按名找、找不到就建**（同名复用，本批内也缓存），分组走移动语义 | `src/service.rs`（`apply_labels` / `link_tags`） |
| 失败语义 ✅ | 本体 + 登记行是**主操作**（不因附属项失败回滚——那会把刚归档好的文件再搬回去）；标签 / 分组是**附属项**（都能在面板上补做），失败原因进 `ArchiveOutcome.notes`，由宿主写进同一句回执——**不静默**（如悬空分组 id → “已归档「x」v1 → …（分组未归入：…）”） | `src/model.rs`、`crates/workbench/src/{services/resource_jobs.rs,panels/resources.rs}` |
| 对话框 ✅ | 新增**别名**输入与**分组**下拉（未分组 / 各分组，当前项打勾）；分组选择住在 `Rc<RefCell<_>>`（下拉不是输入框，而 builder 是 `Fn`），选完 `window.refresh()` 让按钮文案跟上；**不做「新建分组…」子项**（那要“先建组再归档”的两步提交，而面板已有建组入口） | `src/dialogs/archive.rs` |
| 名单来源 ✅ | 宿主的 `known_groups` 从**面板快照的分组字典**取（同一次取数产物，不另查库；快照未到时只给「未分组」） | `crates/workbench/src/components/resource_host.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 1` → **125 单测 + 18 面板窗口 + 9 对话框窗口全绿**（+1 服务：标签与分组真的落上 / 同名标签复用 / 悬空分组只给说明不回滚 / 别名入库；对话框旧用例增断言：别名去空白、分组落成 id）；`cargo test -p rds-workbench -j 1 --lib --test ui_contract` 102 + 7 全绿 | — |

**两处刻意的取舍**：

1. **不改“附属项失败即回滚归档”**：文件已经进了 `resources/`、登记行也写好了，为“标签没打上”搬回去等于把主操作的成果赔进去；而这两项在面板上都能补做；
2. **标签仍是文本输入（逗号分隔）+ 不存在即新建**：归档对话框已经有五个字段，再塞一个可滚动勾选列表就把主流程变成第二个标签管理界面；“打错一个字就多一个标签”的代价由标签对话框的行内删除承担。

**未落地**：默认分组（P2.4 最后一项，语义待拍板：设置页预设 vs 记住上次）、拖拽到分组头、批量标签 / 移动（P2.5）、`F2` 重命名。

### 2026-09-18 — Phase 2 第七刀（P2.4 收口）：分组折叠态持久化

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 设置项 ✅ | `resources.collapsed_groups`（复合值：**项目根路径 → 折叠的分组 key 列表**，`entry = Module`、不上页）。按项目分桶的理由：分组 id 是每项目自己生成的，平铺一份列表会在“打开另一个项目”时被“丢掉不认识的 id”那一步（清已删分组的标记）抹掉别的项目的记录。空列表 = 删掉该项目的记录，不留空壳 | `crates/settings/src/{model,registry,lib}.rs` |
| 面板 ✅ | 新增 `ResourcesPanel::{set_collapsed, collapsed_keys}` + 私有 `refresh_view_items`（折叠 / 注入 / 快照推送三处不再各抄一遍 `build_visible_items`）；`toggle_group_collapse` 把**当前全集**（排序后）交回宿主——增量写入在切项目 / 分组被删时会与旧值叠出幽灵记录 | `src/resource_view.rs` |
| 宿主接线 ✅ | 新端口 `ResourcesHost::remember_collapsed`（写：按项目分桶，没项目时不写）；构造期读设置注入（与默认排序同一位置） | `crates/workbench/src/{components/resource_host.rs,panels/resources.rs}` |
| 验证 | `cargo test -p rds-analytics-resource -j 1` → **124 单测 + 18 面板窗口 + 9 对话框窗口全绿**（+1 面板窗口：注入折叠态当场生效且不回写；折叠旧用例增断言：两次切换交出 `[af_1]` → `[]`）；`cargo test -p rds-settings -j 1` **21 项全绿**（登记表契约测试自动覆盖：复合值不上页 / 无标量读写路径） | — |

**两处刻意的取舍**：

1. **折叠态落 `settings.json` 而不是项目库**：按 `settings-architecture.md` §2.2 的判据它是“项目级结构化状态”，严格说该进 `project.db`；但它只服务一屏的展开形状，而进项目库要新迁移 + 后台往返（加载经过作业线程、写回再走一遍），代价与收益不匹配。位置与 `navigator.filters` 同类（那个也有“当前视图状态”的味道），已在设置架构 §14 登记为待确认项（Q7），将来若随 Q1 一起迁往项目库，M6 跟随；
2. **存的是 key 全集而不是增量**：增量在“切项目 / 分组被删 / 面板重建”这些路径上要靠宿主自己合并旧值，合并逻辑一旦漏一处就是幽灵记录（折叠一个不存在的分组）；全集的代价只是每次多写几十字节。

**未落地**：拖拽到分组头、默认分组（P2.4 剩下的那项，需先想清“默认分组的语义：新建分组还是归档落点”）、批量打标签 / 批量移动（P2.5）、`F2` 重命名。

### 2026-09-18 — Phase 2 第六刀（P2.4 中段）：默认排序接设置项（“记住上次”）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 落盘 key ✅ | `SortField::{key, from_key, default_order}`：key（`name` / `archived_at` / `updated_at` / `size` / `version`）与菜单文案**分家**（文案会改，key 不会）；未知 key 返回 `None`（不猜，调用方回退默认）；每列的默认方向（名称升序，时间 / 大小 / 版本**降序**——点这几列的人想看的是“最新 / 最大”） | `src/filter.rs` |
| 面板 ✅ | 新增 `ResourcesPanel::set_sort(field, order)`（**注入用，不回调宿主**——否则注入的默认值会被当成用户动作原样写回）；`choose_sort` 在算完新排序后调一次新端口 `ResourcesHost::remember_sort`（用户动作才写） | `src/resource_view.rs` |
| 设置项 ✅ | `resources.default_sort`（Enum 五列，默认 `name`，入口 = 设置页 + 模块内，生效 = 下次操作）：设置页「资产库」节第二行 | `crates/settings/src/{model,registry,lib}.rs` |
| 宿主接线 ✅ | 构造期读设置 → `SortField::from_key` → 注入面板；面板点排序 → `remember_sort` → 写回设置（**“默认排序” = 上次用的那个**，不另给一份设置） | `crates/workbench/src/{components/resource_host.rs,panels/resources.rs}` |
| 验证 | `cargo test -p rds-analytics-resource -j 1` → **124 单测 + 17 面板窗口 + 9 对话框窗口全绿**（+1 面板窗口：注入默认排序只重排不回写；两个排序旧用例改断言写入序列）；`cargo test -p rds-settings -j 1` **21 项全绿**；`cargo test -p rds-workbench -j 1 --lib --test ui_contract` 100 + 7 全绿 | — |

**两处刻意的取舍**：

1. **只存字段不存方向**：把（字段 + 方向）存成两个 key 会让设置页多出一堆只能二选一的行；方向交给字段惯例（名称升序、时间 / 大小 / 版本降序），用户在面板里当次的翻转不被记住；
2. **没有单独的“默认排序”菜单**：面板里点排序就是改默认——两处各存一份（“默认”与实际）必然会不一致。

**未落地**：分组折叠状态持久化（P2.4 余项；它是**项目级结构化 UI 状态**，按 `settings-architecture.md` §2.2 的判据不进 `settings.json`，需单开一刀）、拖拽到分组头、批量打标签（P2.5）、`F2` 重命名。

### 2026-09-18 — Phase 2 第五刀（P2.4 前半）：历史内容保留接设置项

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 领域类型 ✅ | `KeepVersions::{All, MetadataOnly, Keep(n)}` + `from_setting` / `to_setting` / `limit` / `label`：把设置项里那个有符号数（`-1` 哨兵）收成一个类型——`All` 在裁剪那一步是 `None`（**不动作**），不是“保留 0 份”；`ArchiveRequest.keep_versions` 随之改收 `Option<KeepVersions>` | `src/model.rs` |
| 服务层 ✅ | `ArchiveService.keep_versions: KeepVersions` + `with_keep_versions(KeepVersions)`；两处裁剪合并为 `prune_copies(resource_id, 本次覆盖)`：全留直接不动作，失败只记日志 | `src/service.rs` |
| 对话框 ✅ | 「保留历史内容」接受 `-1`（仍拒其他负数：可能是手滑打错，不当全留静默吃掉）；填了 `-1` 时行下给一句“全部保留：不裁剪任何历史内容副本”，填了数字给“本次归档：保留最近 n 份”（哨兵值不说明就等于让用户猜） | `src/dialogs/archive.rs` |
| 设置项 ✅ | 新节「资产库」+ `resources.keep_versions`（`i64`，默认 5；预设档：只留元数据 0 / 5 / 10 / 20 / **全部保留 -1**；生效方式 = 下次操作；消费方能点到符号） | `crates/settings/src/{model,registry,lib}.rs` |
| 宿主接线 ✅ | 归档与版本还原两条会写副本的路径：主线程读设置 → `KeepVersions::from_setting`（设置层不依赖 M6，转换在宿主侧）→ 作业带值 → `open_service` 装配；其余作业不传（用默认 5 份） | `crates/workbench/src/{components/resource_host.rs,panels/resources.rs,services/resource_jobs.rs}` |
| 验证 | `cargo test -p rds-analytics-resource -j 1` → **124 单测 + 16 面板窗口 + 9 对话框窗口全绿**（+3：对话框 `-1` 与设置值往返、服务两端策略：`All` 一份不裁 / `MetadataOnly` 副本全清，且两端都不动版本行）；`cargo test -p rds-settings -j 1` **21 项全绿**（登记表契约测试自动覆盖新项）；`cargo test -p rds-workbench -j 1 --lib --test ui_contract` 100 + 7 全绿 | — |

**三处刻意的取舍**：

1. **`-1` 而不是多一个 `keepAll` 开关**：两个会互相矛盾的键比一个稍宽的语义更难用，且 `-1` 是 v1 与架构 §5.2 已有的口径；
2. **`All` 与 `MetadataOnly` 的区分放在 `limit()` 这一步**：`None` = 不裁剪、`Some(0)` = 裁到只剩元数据——两者都是“不保留内容”，但一个是“不动作”一个是“清干净”，合并成一个 0 会在出“全留却把副本删了”这种错时没人看得出来；
3. **设置读取在主线程、随作业带入**：工作线程上拿不到 GPUI 的 global（与 `refresh` 里带 `read_only` 同理），也不在服务层反向读设置——`analytics_resource` 不依赖 settings crate（依赖方向是 `workbench → settings` 与 `workbench → analytics_resource`，两条都不反向）。

**未落地**：默认排序与分组折叠态持久化（P2.4 后半，需先过设置层的准入五条）、拖拽到分组头、批量打标签（P2.5）、`F2` 重命名。

### 2026-09-18 — Phase 2 第四刀（P2.3 余项）：五个排序键（+ 归档登记体积）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 行带原始值 ✅ | `ArchiveRow` 加 `updated_epoch` / `archived_epoch` / `size_bytes`（时间为 Unix 秒、体积为字节）——排序比原始值，**不解析格式化过的尾巴**（`1.2 KB` 与 `900 B` 比会静默排错）；`present::to_row` 从行模型填 | `src/resource_view.rs`、`src/present.rs` |
| 五个排序键 ✅ | `SortField` 扩为 名称 / 归档时间 / 更新时间 / 大小 / 版本（原型 §2.2 口径）+ `SortField::ALL`（菜单顺序与文案的单一来源）；`apply_view` 逐键按原始值比较，同键名称兜底且**不随方向翻转** | `src/filter.rs` |
| 缺值排最后 ✅ | `opt_key(Option<i64>, order)`：没有体积 / 没记归档时间的行在**两个方向上都排最后**——降序把“未知”顶到最前，等于让它冒充“最大” | 同上 |
| 排序菜单 ✅ | 菜单遍历 `SortField::ALL`（原来硬编码两项），当前项带方向箭头 | `src/resource_view.rs` |
| **体积真的被登记** ✅ | 「大小」排序要有真数据：`PayloadStore::file_size`（可传任意路径；读不到给 `None`，不假装 0）。归档（首次 / 再归档）、还原到历史版本、补登为存档、接受当前内容**五条路径都指纹与体积同批写**——只换指纹不换体积会让「大小」永远停在旧值上（错得比没数据还难发现）；写入前夹紧到 `i32` 上限（列是 64 位、模型是 `i32`，不夹会在读回时变出负数） | `src/payload.rs`、`src/model.rs`、`src/resource.rs`、`src/service.rs`、`src/indexer.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 1` → **121 单测 + 16 面板窗口 + 9 对话框窗口全绿**（+3 排序单测：三个键的原始值口径 / 缺值两方向都排最后 / 同键兜底不翻转；+1 面板窗口：名称与时间·体积反着排，证明没落到名称兜底上；+3 存储断言：归档登记体积、再归档换体积、还原回历史体积）；`cargo check -p rds-workbench --all-targets -j 1` 通过（3 条告警在编辑器补全的 WIP 里，与本批无关） | — |

**两处刻意的取舍**：

1. **体积写在归档时而不是渲染时**：面板取数在后台线程上，但为排序逐行 stat 等于每次刷新都把本体全读一遍元数据；归档一次记下来，行上就是纯数据（索引可重建：重建索引的 `adopt_file` 同样登记）；
2. **缺值排最后而不是给 0 / 空字符串**：分析表与引用型本来就没有字节数，拿 0 参与排序会把它们混进“最小”一类——那是在编数据。

**未落地**：`keepVersions` 接设置项（P2.4）、分组折叠状态持久化（P2.4）、拖拽到分组头、批量打标签（P2.5）、`F2` 重命名。（搜索匹配别名 / 标签 / 来源表已由**第十一刀**补齐。）

### 2026-09-18 — Phase 2 第三刀：组织方式的管理入口（标签改名 / 删除 + 分组建 / 改 / 删 / 移动）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 标签管理 ✅ | 标签对话框每行加 **⋯ 菜单**（重命名… / 删除）；重命名开一个单输入小对话框（预填当前名，空名拒绝，幂等与同名拒绝在存储层）；删除走 `AlertDialog` 确认 + **说清“从 N 条存档上摘掉”**；两个动作都走已有的 `Job::TagAction`（`RenameTag` / `DeleteTag` 新增两个分支） | `src/dialogs/tag.rs`、`crates/workbench/src/services/resource_jobs.rs` |
| 分组管理 ✅ | 新对话框 `dialogs/group.rs`（**一个文件两个形态**：新建 / 重命名，共用一个输入 + 校验）；**行右键菜单「移动到分组 ›」子菜单**（未分组 + 各分组 + 新建分组…，当前所在项置灰）——多选也用这一条（批量移动是原型里多选解锁的动作）；**分组头右键**：重命名… / 删除分组 / 新建分组…（虚拟分组「全部分组 / 未分组」不给菜单） | `src/dialogs/group.rs`、`src/resource_view.rs`、`src/ui.rs`（+1 常量） |
| 宿主接线 ✅ | `ResourcesHost` +4 个端口（`request_move_to_group` / `request_create_group` / `request_rename_group` / `request_delete_group`）；`Job::GroupAction`（`Create` / `Rename` / `Delete` / `Move`，改完重取主列表——分区与归属都在快照里）；分组显示名从**面板快照的分组字典**取（事件路径不开库） | `crates/workbench/src/{services/resource_jobs.rs,components/resource_host.rs,panels/resources.rs}` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **118 单测 + 15 面板窗口 + 9 对话框窗口全绿**（+1 分组对话框；+1 对话框窗口：新建 / 重命名两种形态都开得出、没提交就不发事件）；`cargo check -p rds-workbench --lib -j 2` 零告警 | — |

**两处刻意的取舍**：

1. **「移动到分组」不进对话框**：行菜单里直接列分组，一次点击完成（多一级对话框反而多两步）；
2. **删除分组不删存档**：确认框里明说“组里的存档会回到未分组”（分组是组织方式）。

**未落地**：拖拽行到分组头（沿用 M5 行拖拽，单独一刀）、折叠状态持久化（P2.4）、批量打标签（P2.5）、`F2` 重命名。

### 2026-09-18 — Phase 2 第二刀：分组折叠区（存储层补齐 + 分区渲染 + 折叠）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 存储层补齐 ✅ | 建 / **改名** / **删除**（同一事务：软删分组行 + 清成员关联，**成员回到未分组**——分组是组织方式，删分组不该删存档）；`add_resource_to_folder` 改成**移动语义**（先清后插：单层分组下一个资源只在一个分组，否则面板分区会把同一行画两遍）；新增 `folders_by_resource`（一次查完）与 `clear_resource_folder`（移回未分组）；`create_folder` 拒空名 / 同名 / 父分组（单层在类型上就不存在子分组）；行映射收敛为 `map_folder_row` | `src/folder.rs`、`src/tests.rs`（t018） |
| 行与快照 ✅ | `ArchiveRow.folder_id: Option<String>`（单层 → `Option` 而不是集合，类型上挡掉“同时属于两个分组”）、`ResourcesSnapshot.groups: Vec<GroupOption>`；`build_snapshot` 改成**输入结构体** `SnapshotInputs`（字段已经七八个，位置参数到那一步就没人看得懂了） | `src/resource_view.rs`、`src/present.rs` |
| 分区渲染 ✅ | `filter::build_visible_items`（纯函数）：**全部分组 → 未分组 → 各分组**三层；分组头与行一样是列表的一项（虚拟化 / 漫游 / 滚动只维护一份）；**计数从当前可见行现算**（筛选后头里的数就是眼前的行数）；认不出的分组归属算未分组（行不丢）；一个分组都没有时不出头（空库不多两行噪声） | `src/filter.rs` |
| 分组头与折叠 ✅ | 头 = 2px 色条（`list_active_border`——原型 §6 明说不用 `sidebar_accent`）+ 折叠三角 + 名称 + 计数；点整行折叠 / 展开；折叠**只影响行出场**（`view_rows` 不变，否则选中 / 多选会被误清）；分组被删后自动清掉它的折叠标记 | `src/resource_view.rs`、`src/ui.rs`（+1 常量） |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **117 单测 + 15 面板窗口 + 8 对话框窗口全绿**（+1 存储：建/改名/删/移动语义/批量映射；+5 分区：无分组不出头 / 三层结构与计数 / 折叠 / 计数随可见行 / 脏分组归属；+1 面板窗口：三个头渲染 + 折叠后行消失而 `view_rows` 不变）；`cargo test -p rds-workbench -j 1 --lib --test ui_contract` 95 + 7 全绿 | — |

**三处刻意的取舍**：

1. **折叠状态是会话级**：原型要求“持久化”，而设置项（`settings.json`）属 P2.4——在那之前不假装持久化；
2. **计数从可见行现算**，而不是让宿主给“每分组总数”：头与列表永远一致，且少一条查询；
3. **删分组不删存档**：成员回「未分组」（原型 §2.4 的三层结构里本来就有一层未分组）。

**未落地**：分组管理 UI（新建 / 改名 / 删除入口）、「移动到分组」菜单与**拖拽到分组头**（沿用 M5 行拖拽，单独一刀）、折叠状态持久化（P2.4）。

### 2026-09-18 — Phase 2 第一刀：标签（存储层补齐 + 打标 / 去标 + 筛选维）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 存储层补齐 ✅ | **补 v1 缺失的两项**：`rename_tag`（同名未删拒绝、改成自己幂等、`scope` 不可编辑）、`delete_tag`（**同一事务里软删标签行 + 清全部关联**，返回解除数）；`create_tag` 补空名与同名拒绝（库里有部分唯一索引兑底，但那条约束报的是英文 SQLite 原话）；新增 `tags_by_resource`（一次查完，避免 N+1）与 `tag_usage_counts`（筛选菜单的用量）；三处重复的行映射收敛为 `map_tag_row` | `src/tag.rs`、`src/tests.rs`（t017） |
| 数据进快照 ✅ | `ArchiveRow` 加 `tag_ids`（筛选用 id：名字会改）、`ResourcesSnapshot` 加 `tags: Vec<TagOption>`（字典 + 用量）、`ArchiveDetail.tags` 从 `Vec<String>` 改为 `Vec<ArchiveTagChip>`（带 id）；`present::{tag_chips, tag_options, to_row, to_detail, build_snapshot}` 均接入 | `src/{resource_view,detail_view,present}.rs` |
| 筛选维 ✅ | `ResourcesFilter.tags`（id 多选，**并集**：选中两个标签 = 这两个标签的存档都看）；`menu_dims` 计入标签数；快照推送时清悬空标签条件（标签被删后不能留下“什么都没匹配”而勾还在） | `src/filter.rs`、`src/resource_view.rs`（筛选菜单加标签组） |
| 打标 / 去标 ✅ | 详情面板新增**标签分区**：chips（每枚带 ×，点了就去掉；不进确认框——重新打上只需两步）+ 「＋ 标签」开对话框；只读项目下两者都收起来 | `src/detail_view.rs` |
| 标签对话框 ✅ | `dialogs/tag.rs`：全部标签勾选（带用量）+ 新建输入（「新建并打上」；**回车 = 应用**，与主按钮同路）+ 底栏差集提示（“本次改动：加 N · 去 M”）+ 取消 / 应用（**应用后关窗**；新建后不关窗，等宿主把新词典推回来）；状态可被宿主换行与对齐比较基准 | 同上、`src/ui.rs`（+2 常量） |
| 宿主接线 ✅ | `Job::TagList` / `Job::TagAction`（`TagJobAction::{Apply, CreateAndTag, RemoveOne}`）；动作后**重取标签行 + 主列表**（行的 `tag_ids` 与详情 chips 都在快照里）；`Shared::tag_dialog`（pending → 侧栏 render 开窗 → session → 关窗清掉）；刷新时标签两件事都一次查完 | `crates/workbench/src/{services/resource_jobs.rs,panels/{shared,resources,mod}.rs,components/resource_host.rs}` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **111 单测 + 14 面板窗口 + 8 对话框窗口全绿**（+1 存储：改名/删除/批量/用量；+2 筛选；+1 呈现；+3 标签对话框；+1 面板窗口：标签筛选与悬空条件；+1 面板窗口：详情 chips 的 × 与只读态；+1 对话框窗口：词典换行不关窗）；`cargo test -p rds-workbench -j 1 --lib --test ui_contract` 95 + 7 全绿（`Shared` 白名单 +`tag_dialog`） | — |

**三处刻意的取舍**：

1. **标签颜色不上色**：`analytics_tags.color` 是用户填的 hex，而颜色一律走主题 token（原型 §6 零裸色）——先用“淡边 + 文字”的 chip，颜色留给后续“按颜色分组”一类需求再谈；
2. **多选是并集**：“同时打两个标签”是更细的诉求，靠筛选器表达会多一个没人看得懂的语义；
3. **× 不要确认**：去标签是可两步恢复的动作，弹确认框比误点代价还大。

**未落地**：分组的建/改/删/移与面板的分组折叠区（P2.2）、`F2` 重命名（等重命名入口）、批量打标签（多选态，P2.5）、更多排序键（需 `ArchiveRow` 带原始值）、`keepVersions` 接设置项（P2.4）。

### 2026-09-18 — P0.8 + Phase 3 第三刀：项目级回收站（上提中性化 → 移入 / 还原 / 永久删除 / 清空 + 对话框）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| **回收站上提 + 中性化** ✅ | `ProjectTrash` 从 `scratchpad` 搬到 `engine::persistence::trash`：来源是字符串 `origin`，类型是 `TrashKind{File,Folder}`，还原返回 `TrashRestoreOutcome`（**不再返回 `ScratchpadEntry`**）；`unique_path` 内联（并报告是否改名）；**不做 origin 校验**（那是调用方纪律）。`scratchpad` 侧改重导出，旧路径仍可用 | `crates/engine/src/persistence/{mod,trash}.rs`、`crates/scratchpad/src/{lib,store}.rs`、`crates/scratchpad/Cargo.toml` |
| **按来源清空** ✅ | `ProjectTrash::empty_origin(origin)`：整仓 `empty()` 会把别的模块的条目一起删掉——共用一处仓库不等于可以替对方清空；M5 的列表与「清空」也改成按来源过滤（`list_trash` / `empty_trash`），跨模块条目不再出现在草稿箱列表里 | 同上 |
| 索引侧软删 ✅ | 新增 `soft_delete_archive` / `undelete_archive`（可同步改本体路径）/ `find_deleted_archive_by_rel_path` / `purge_deleted_row` / `purge_all_deleted`；`hard_delete_row` 与两个 purge 都**连标签与分组关联一起删**（关联表是 `resource_id` 上的外键且无 `ON DELETE CASCADE`，而项目库开了 `foreign_keys=ON`——不清关联会直接删不掉） | `src/resource.rs` |
| 四个服务动作 ✅ | `move_to_trash`（批量；先移本体、再软删行、失败回滚本体；**不做预回滚**，中途失败时错误里带“本批已移入 N 项”）、`restore_archive_from_trash`（三条守卫：origin / 登记行还在 / 同名不覆盖并同步登记路径）、`restore_archive_by_rel_path`（索引修复那条；找不到就指向“只能删记录”）、`purge_archive` + `empty_trash`（都只动本模块的条目）；事件 `Trashed` / `Untrashed` | `src/service.rs`、`src/model.rs`（`ORIGIN_RESOURCES` / `TrashArchiveEntry`） |
| 对话框 ✅ | `dialogs/trash.rs`：条目表格（名称+类型 / 原位置 / 删除时间 / 大小）+ 行内动作（还原、永久删除走 `AlertDialog`）+ 页脚「清空回收站」（只清本模块的，确认文案里说清）+ **别人的条目只给一句说明不列行**（列出来等于摆一个必然被拒的还原）；行数据可被宿主换行 | 同上、`src/ui.rs`（+6 常量）、`src/present.rs`（`build_trash_snapshot`） |
| 宿主接线 ✅ | `Job::Trash`（批量移入）/ `Job::TrashList` + `Job::TrashAction`（取数 / 还原 / 永久删除 / 清空；动作后重取回收站列表与主列表）；`Shared::trash_dialog`（pending → 侧栏 render 开窗 → session → 关窗清掉）；详情面板加**危险区**（移入回收站；引用型与本体缺失时置灰并给出口）；索引修复的「从回收站还原」**开闸**（原来摆着置灰） | `crates/workbench/src/{services/resource_jobs.rs,panels/{shared,resources,mod}.rs,components/resource_host.rs}`、`src/detail_view.rs`、`src/dialogs/index_repair.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **104 单测 + 12 面板窗口 + 7 对话框窗口全绿**（+6 服务：软删与本体入站 / 还原与跨模块拒绝 / 同名避让与本体缺失拒移 / 部分成功 / 只动自己的永久删除与清空 / 按路径还原；+2 呈现；+3 回收站对话框；+1 对话框窗口测试）；`cargo test -p rds-engine --lib -j 2 trash` 4 项（含 `empty_origin` 只删自己名下的）；`cargo test -p rds-scratchpad --lib -j 2` 36 项全绿；`cargo test -p rds-workbench --lib -j 2` 95 项 + `--test ui_contract` 7 项全绿（`Shared` 白名单 +`trash_dialog`） | — |

**三处刻意的取舍**：

1. **软删登记行而不是硬删**：标签与分组是资源 id 上的关联，硬删会留下孤儿归属（v1“还原丢归属”就是这么来的）——行留在表里、靠 `deleted_at IS NULL` 过滤，还原才能把别名 / 指纹 / 标签一起带回；
2. **不做预回滚**：批量移入中途失败时已进回收站的**不回退**（部分成功就部分成功），但错误里要交代“本批已移入 N 项”——回退会让状态更难读；
3. **别人的条目只给说明**：项目级回收站里看得见别人的东西，但还原与删除都只做自己名下的（跨模块还原 = 把对方的数据搬进 `resources/`，那是越权）。

**未落地**：回收站的自动清理策略（按时间 / 容量）；回收站条目的“在原位置打开”；`F2` 重命名与批量标签 / 分组（Phase 2）。

### 2026-09-17 — Phase 1 第十四刀：批量多选与行点击归位（原型 §2.3 / §3.2 / §9）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| **单击不再打开** ✅ | 组件（`list::List`）把**单击**接到 `confirm`，而面板的 confirm 是「打开（只读）」——点一行就开文件，与原型“单击=选中、双击=打开”相反。行自己接管点击（`on_click` + `stop_propagation`），外层的确认通道只留给 `Enter` | `src/resource_view.rs`（`classify_row_click`） |
| 点击语义 ✅ | `RowClick` 四态：单击=单选、`Ctrl`=切换入选择集、`Shift`=从锚点到该行的区间、双击=打开；判定用纯函数 `classify_click(click_count, modifiers)`（双击优先、`Shift` 优先于 `Ctrl`），变换用 `apply_row_click`（选择集按可见行顺序维护）——两者都有单测 | 同上 |
| 多选状态 ✅ | 面板 `multi`（选择集，有序）+ `anchor`（区间锚点）与 `selected`（焦点行，详情面板读它）并存；空集时清锚点、只剩一条时焦点回归它；筛选/排序同样清悬空项 | 同上 |
| `Ctrl+A` ✅ | 全选当前可见行（原型 §2.3 说“当前分组”，分组未落 → 即全部可见行）；新 Action `SelectAllRows`，app 层绑 `ctrl-a` | `src/commands.rs`、`crates/app/src/main.rs` |
| 多选视觉与菜单 ✅ | 多选行自己画背景（组件的选中样式只认它的单选索引）；多选时单行动作（打开 / 查看统计 / 取回 / 版本历史 / 复制路径 / 在系统中显示）一律置灰，删除项改文案为“移入回收站（N 项）”并对整个选择集生效（原型 §3.2“单/多选 → 单选可用”） | `src/resource_view.rs` |
| 批量删除链路 ✅ | `ResourcesHost::request_delete` 改收 `&[String]`（单条与批量同一条路）；`DeleteSelected` 改成对整个选择集生效；回执带数量（真删除仍等 P0.8） | 同上、`crates/workbench/src/components/resource_host.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **95 单测 + 6 对话框窗口测试 + 12 面板窗口测试全绿**（+2 单测：点击判定与选择集变换；+1 窗口测试：手势 → `Ctrl+A` → `Delete` 整批到宿主，且选中不触发打开）；`cargo test -p rds-workbench --test ui_contract -j 2` 7 项全绿；`cargo check -p rds-workbench --all-targets` 与 `cargo check -p rds-app` 零告警 | — |

**两处刻意的克制**：

1. **不做批量取回**：原型 §3.2 把取回定为“单选”（多选在原型里只解锁批量删除与后续的批量标签/移动），不自己加一套；
2. **`Esc` 不清多选**：原型里 `Esc` 只负责清搜索 / 关菜单，不抢它的语义（清多选等真实需要时再说）。

**未落地**：`F2` 重命名（等重命名入口，Phase 2）；批量打标签 / 批量移动（等标签与分组批）；批量删除的真执行（等 P0.8）。

### 2026-09-17 — Phase 3 第二刀：索引修复对话框（原型 §4.5）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 对话框 ✅ | `dialogs/index_repair.rs`：三分组（有文件无记录 / 有记录无本体 / 指纹不匹配）+ 行内动作列（与版本历史不同：这里一行最多两个动作、行内容短，装得下）；每组标计数与一句说明；干净时给一句话而不是空表格 | 同上、`src/ui.rs`（+2 常量） |
| 行数据合成 ✅ | `present::build_repair_rows`：按分组顺序 + 组内路径排序；**指纹不匹配行把两个指纹都摆出来**（“登记 xxx · 实际 yyy”，各缩到 12 位）；未登记行的标题取文件名（完整路径在副文案里） | `src/present.rs` |
| 三个真动作 ✅ | 补登为存档（`adopt_file`，**固定文件型**——`resources/` 里只可能是文件，所以不摆一个只有一项的 kind 选择器；来源无从得知留空）；删除记录（`remove_orphan_record`，走 `AlertDialog` 二次确认）；接受当前内容（`accept_current_content`，回执说明“旧内容不可得，那一版只留元数据”） | `crates/workbench/src/services/resource_jobs.rs`（`Job::IndexRepairAction`） |
| 跳转而非重复 ✅ | 「从历史还原」在本对话框里就是「打开版本历史…」：挑哪一版是版本历史的活，不在这里重复一遍挑选 UI（跳转类动作由宿主在事件路径拦截，不入修复作业） | `crates/analytics_resource/src/dialogs/index_repair.rs`、`crates/workbench/src/panels/resources.rs` |
| 两个入口真接 ✅ | 状态行「修复…」与面板头「⋯ → 重建索引…」→ `Job::IndexScan`（后台线程，逐个本体算 sha256）→ 报告 → 侧栏 render 开窗；修完一项**自动重扫**并把新行推回已开的窗（与版本历史同形） | `crates/workbench/src/{components/resource_host.rs,panels/{shared,resources,mod}.rs}` |
| 端口简化 ✅ | `ResourcesHost::request_version_history` 改收 `resource_id: &str`（原来收整条 `ArchiveDetail` 但只用 id）：调用方不再为一个 id 克隆整条详情，索引用修复对话框也能直接调它 | `src/resource_view.rs`、`src/detail_view.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **92 单测 + 6 对话框窗口测试 + 11 面板窗口测试全绿**（+3 对话框、+2 呈现单测；+1 对话框窗口测试：三分组各行的动作就位 / 干净时不给动作 / 修完换空行后对话框还在）；`cargo test -p rds-workbench --test ui_contract -j 2` 7 项全绿（`Shared` 白名单 +`repair_dialog`）；`cargo check -p rds-workbench --all-targets -j 2` 零告警 | — |

**两处刻意的取舍**：

1. **不分资源**：整个项目只有一份索引，所以修复会话不带 id（版本历史那种“同一个存档才复用”的判断在这里不存在）；
2. **「从回收站还原」摆着但置灰**并给 tooltip：“等项目级回收站上提（P0.8）后才可用”——藏起来用户会以为没有这个能力，摆出来能说清为什么现在没有。

**未落地**：“有文件、无记录”的按目录批量补登（现为逐行）；详情面板版本区的“最近 3 条”明细；回收站对话框（等 P0.8）。

### 2026-09-17 — Phase 3 第一刀：版本历史对话框（原型 §4.3）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 本体层版本副本 ✅ | `version_copies`（有哪些版本的副本，降序）/ `version_copy_file`（某版本的副本文件：按目录里的实际文件名取，改名后也能找到自己的历史）/ `delete_version_copy`（只删副本、不删版本行）/ `copy_version_out`（取回该版本为草稿）/ `restore_version_copy`（写回本体位置，含只读属性恢复） | `src/payload.rs` |
| 还原语义 ✅ | `ArchiveService::restore_version`：**用旧内容生成新版本**（与再归档同序：留副本 → 写前快照 → 覆盖本体 → 索引 +1），两条守卫不静默降级——版本无副本则拒（并说清“可能已被保留策略裁剪”），副本内容与当前指纹相同则幂等返回（不为“还原到自己”造无用版本） | `src/service.rs`、`ChangeReason::Restored`（`src/model.rs`） |
| 历史版本取回 ✅ | `ArchiveService::checkout_version`：源换成历史副本，其余与 `checkout` 同一语义与守卫（不得落在 `resources/` 内） | 同上 |
| 行数据合成 ✅ | `present::build_version_rows`：**当前版本也占一行**（写前快照语义下版本表里没有它），相邻版本算“较 vN 大小±X · 指纹是否变”；历史行的大小 / 指纹从快照 JSON 取，**解析失败就留空**（不拿当前值顶替） | `src/present.rs` |
| 对话框 ✅ | `dialogs/version.rs`：版本 / 时间 / 大小 / 指纹 / 变化 / 副本六列的表格（最多 50 行 + 明说“还有 N 个更早的未列出”）；选中行后动作栏才出场（还原为当前版本 / 取回该版本为草稿 / 删除内容副本）；删副本走 `AlertDialog` 二次确认（副本不进回收站）；只读项目下三动作置灰且**说明原因** | 同上、`src/ui.rs`（+6 常数） |
| 行可被换掉 ✅ | `VersionDialogState` 由宿主持有克隆：动作完成后 worker 再取一次版本 → `set_rows` 换行，**不必关窗重开**（选中项在新行里不存在则自动清掉） | 同上 |
| 两个入口 ✅ | 行右键菜单「版本历史…」（在“取回…”之后，不设禁用：本体缺失也能看历史）+ 详情面板「版本」分区的「查看全部…」（该分区改成**总是出现**：只有 v1 的存档也能打开，而“没有历史”本身就是一条信息） | `src/resource_view.rs`、`src/detail_view.rs` |
| 宿主接线 ✅ | `Job::Versions` / `Job::VersionAction`（动作后**顺手再取一次版本 + 主列表**）；`Shared::version_dialog`（`pending` → 侧栏 render 开窗 → `session` → 关窗清掉）；只读与无项目时拒绝并把对话框忙态收掉 | `crates/workbench/src/{services/resource_jobs.rs,components/resource_host.rs,panels/{shared,resources,mod}.rs}` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **87 单测 + 5 对话框窗口测试 + 11 面板窗口测试全绿**（+1 本体、+2 服务、+2 呈现、+3 对话框单测；+1 对话框窗口测试：列表渲染 / 选中后动作栏出现 / 换行后选中被清）；`cargo test -p rds-workbench --test ui_contract -j 2` 7 项全绿（`Shared` 白名单 +`version_dialog`）；`cargo check -p rds-workbench --all-targets -j 2` 零告警 | — |

**三处刻意的取舍**：

1. **不做行级 diff**（原型 §4.3 已定）：只给“大小 ±N · 指纹是否变”两个值，文本 diff 归编辑器；
2. **不做原地回滚**：还原就是新版本（与 git `revert` 同构），历史永远只追加；
3. **列表不虚拟化**：版本行最多列 50 条并**明说**（与草稿选择对话框同一口径）——版本元数据行不随保留策略缩减，真成为问题再上 `List`。

**未落地**：详情面板版本区的“最近 3 条”明细（需要一次批量“每存档最近 N 版”取数）、回收到对话框 / 索引修复对话框（Phase 3 余下两项）。

### 2026-09-17 — Phase 1 第十三刀：面板头收口（`⋯` 四项 + 标题图标，原型 §2.1）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 面板头 `⋯` ✅ | 原型 §2.1 的四项：**重建索引…**（与状态行「修复…」同一条路）/ **打开资源目录**（系统文件管理器开 `{项目}/resources/`）/ **回收站…**（等 P0.8）/ **刷新**。前两项是真入口，后两项沿用已有回执——入口先摆出来，点了说清为什么没动 | `src/resource_view.rs` |
| 菜单动作可测 ✅ | `HeaderMenuAction`（四项 + `label` + `ALL`）与 `dispatch_header_action` 放在渲染闭包之外：菜单顺序与文案被单测钉住，四个去向也能走**生产入口**被窗口测试点到（弹层里的菜单项在窗口测试里点不到） | 同上 |
| 标题图标 ✅ | 前缀 `icons/chart-column.svg`（与活动栏该面板同一形状），一律 `muted`——标题文字才是 `foreground` | 同上 |
| 宿主端口 +3 ✅ | `request_open_payload_dir`（目录不存在时**说清**“归档第一个存档时会创建”，而不是抛一个系统错误）/ `request_open_trash`（回执）/ `request_refresh`（不写状态栏回执：状态行的「加载中…」就是它的回执，再叠一条只是噪声） | `crates/workbench/src/components/resource_host.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **79 单测 + 4 对话框窗口测试 + 11 面板窗口测试全绿**（+1 单测：菜单四项的顺序与文案；+1 窗口测试：`⋯` 在面板头里且四个动作各自到得了宿主）；`cargo test -p rds-workbench --test ui_contract -j 2` 7 项全绿；`cargo check -p rds-workbench --lib -j 2` 零告警 | — |

**一处有意留白**：标题点击折叠/展开（原型 §2.1 第三行）**不做**——M4/M5 面板头由 Dock 的 tab 呈现、不自绘，单独给 M6 会让同一个动作只在一处可用；等“自绘面板头统一”那一批三块一起做。已同步到原型 §2.1 的落地注。

### 2026-09-17 — Phase 1 第十二刀：草稿箱归档入口与「＋ ▾」（原型 §2.1 对齐）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 面板头 `＋ ▾` ✅ | 原型 §2.1 的双入口：**从草稿箱归档…**（草稿多选对话框）/ **从本地文件归档…**（系统文件选择）。菜单存在的理由就是那句"存档的本体必须来自某处"——不存在凭空创建的存档 | `src/resource_view.rs`、`src/ui.rs`（+2 常量） |
| 草稿多选对话框 ✅ | `dialogs/pick.rs`：候选由**宿主**备好（M6 不认识草稿箱的内部结构）；「全选」开关 + 行 checkbox；**单选 → 交回宿主展开完整的归档确认**（可改显示名 / 看到冲突落点），**多选 → 默认名批量入队**；列表最多列 50 条并**明说**"还有 N 个未列出"（不静默截断，也不为它手搓虚拟化） | `src/dialogs/pick.rs` |
| 归档凭证补上出处 ✅ | 草稿来源的两件事：`source_connection_id` 取草稿 `file_meta` 的**首选连接**（显式绑定优先，其次最近执行过的那条）；`promoted_from` 记 `scratchpad/<草稿相对路径>`——原型 §4.1 的"来源连接自动带出"到此落地 | `crates/workbench/src/components/resource_host.rs` |
| 目标位置保目录结构 ✅ | 草稿归档的 `rel_path` 直接用草稿相对路径（`resources/reports/a.sql`），与原型 §4.1 一致；本地文件仍只给文件名（它没有草稿相对路径） | 同上 |
| 端口 ✅ | `ResourcesHost` 拆成 `request_archive_from_drafts` / `request_archive_from_file`（旧的单一 `request_archive` 下线）；app 层的 `RequestArchive` 动作落到"本地文件"那条（动作没有"选菜单"这一步），在代码注释里写明 | `src/resource_view.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **78 单测 + 4 对话框窗口测试 + 10 面板窗口测试全绿**（+1 单测：选中顺序与列表一致；+1 窗口测试：草稿选择对话框开得出、按钮真在、未选不提交、勾两条交回的就是它们） | — |

**两处与原型不同**（均记在此）：

1. **空库的双按钮**：原型 §5 写"从草稿箱归档… / 了解资产库能做什么"，实现给的是**从草稿箱归档… / 从本地文件归档…**——"了解能做什么"需要一个帮助页落点，仓里还没有（产品级决定，不自造）。
2. **草稿箱右键「归档为存档…」仍未接**：那是草稿箱 crate 的菜单项（其视图刚下沉完，属另一处改动）；面板头这条已覆盖同一需求，两句文案与弹窗完全共用。

**未落地**：版本历史 / 回收站 / 索引修复三个对话框（Phase 3；回收站等 P0.8）、分组与标签、批量多选、`F2` / `Ctrl+A` / `Ctrl+Z`、详情面板的版本最近 3 条 / 内容预览 / 危险区。

### 2026-09-17 — Phase 1 第十一刀：原型收口（加载态 / 路径动作 / 指纹复制）

> 触发：对"实现 vs 原型"做了一次逐节核对（清单见本刀末尾），把不依赖其他相位的差异一次补齐。

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 加载态 ✅ | 原型 §5 的"加载中"行：入队即置 `loading`，状态行前缀「加载中… ·」（**不藏计数**）；列表为空时给 **3 行骨架**而不是空态——否则首个快照到达前会先给用户看一眼"还没有任何存档"（那是"还没读到"） | `src/resource_view.rs`、`crates/workbench/src/panels/resources.rs` |
| 右键菜单补两项 ✅ | 原型 §3.2 的「在系统中显示」/「复制路径」：后者拷**本体绝对路径**到剪贴板并给回执；前者用 `opener::reveal`（选中文件而不是只开目录）。两者都要求本体可定位（旧行 / 缺失行置灰） | 同上、`crates/workbench/src/components/resource_host.rs` |
| `opener` 开 `reveal` feature ✅ | 工作区的 `opener` 原本只有 `open`（打开文件本身）；`reveal` 是官方特性门后的 API，一处开启全仓可用（**不动 project 里那份自写的 `explorer` 调用**，它可后续收敛到同一个 API） | `Cargo.toml`（workspace） |
| 详情面板补两块 ✅ | 头部补 **kind 图标**（与列表行同一套 `kind_icon`，一律 `muted`）；「内容指纹」那行补**复制**口（拷**完整指纹**——拿去比对时截断版没用；展示仍是前 12 位），标签文案用 `HASH_LABEL` 常量两处共用 | `src/detail_view.rs`、`src/resource_view.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **77 单测 + 3 对话框窗口测试 + 10 面板窗口测试全绿**（+1 窗口测试：取数中给骨架且不摆空态；+1 单测：`status_line` 加载中只加前缀） | — |

**逐节核对结果**（实现 vs 原型）：

- **有意差异**：详情面板落右 Dock（17.5rem）而非编辑区右侧 20rem 属性面板；归档入口先接"本地文件"（草稿箱右键与「＋▾」下拉未接）；归档对话框缺 分组 / 别名（等 Phase 2）；回执双轨（状态栏 + 5 秒撤销栏）；缺失行用**灰显**而非删除线（行内只留一个颜色信号）；排序键 / 搜索范围 / 筛选维度按行上有的原始值给（Phase 2 扩）。
- **未落地**：面板头 `＋▾` / `⋯`（四项）、标题图标与点击折叠、分组折叠区与拖动入分组、标签 chips、F2 / Ctrl+A / 多选、hover 行内动作、详情头部可点改显示名 / 来源连接跳 M4 / 版本「最近 3 条 + 查看全部…」/ 内容预览 / 危险区、右键菜单的版本历史与重命名·标签·分组项、空库双按钮与草稿箱建议行、索引异常首次进入的顶部提示条、`TableRef` 的"立即校验"、三个 Phase 3 对话框、取回侧记 `derived_from_resource_id`（M5 侧）、编辑器顶部只读条（编辑器侧）。

### 2026-09-17 — Phase 1 第十刀：只读三重守卫（P1.6，编辑器侧）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 编辑器只读打开 ✅ | `editor::persist::open_file_read_only`（与 `open_file` 同一个内部实现，只差一个只读维度）：以**编辑器只读**建文档——状态栏「只读」+ 编辑内核 readonly + 保存动作拒绝，全走既有机制（`ReadOnly { editor, connection }` 的两个维度**不合并**） | `crates/editor/src/persist.rs` |
| 端口带只读维度 ✅ | 「在编辑器中打开」请求由**路径**扩成 `OpenInEditorRequest { path, read_only }`：草稿箱的草稿可写、M6 的存档本体只能看——**只读由发起方判定**（编辑器不认识 `resources/` 的归属） | `crates/workbench/src/panels/{shared,mod}.rs` |
| 宿主 ✅ | `WorkbenchView::open_in_editor_with(path, read_only, …)`；同路径已打开仍旧**只激活、不重读、也不改只读态**（不把用户手上的文档悄悄锁上） | `crates/workbench/src/view.rs` |
| 面板接线 ✅ | `ResourcesHost::request_open` 改收**整条 `ArchiveDetail`**（本体路径在它身上，与 `request_checkout` 同口径）；本体路径由 `PayloadStore::resolve` 解析（越界 / 点前缀守卫在那一层）；菜单「打开（只读）」、回车/双击与详情面板动作区都走它 | `crates/workbench/src/components/resource_host.rs`、`src/{resource_view,detail_view}.rs` |
| 详情动作区 ✅ | 补上「打开（只读）」（与「取回（检出）…」并列）：打开 = 看本体（缺失的行 / 旧行没登记路径时禁用）；取回 = 拿一份可写工作副本（本体异常或只读项目下禁用）——禁用理由指向出口 | `src/detail_view.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **77 单测 + 3 对话框窗口测试 + 9 面板窗口测试全绿**（+1 窗口测试：派发 `OpenSelected` 后宿主收到的是**选中那条的详情**）；`cargo test -p rds-editor --lib -j 2` → **217 项全绿**（+1：只读打开置编辑器只读且连接维不受影响，可写版对照不戴只读）；`cargo check -p rds-workbench --lib -j 2` 零告警 | — |

**三重守卫的现状**（P1.6 完整形态）：① 应用守卫（写入 `resources/` 直接拒）与 ③ 文件系统只读属性在 Phase 0 已落；本刀补上 ② 编辑器只读。三者仍然**不对等**：只有应用守卫是硬约束，另两层是提示与辅助（网络盘 / 有权限的用户可绕过）。

**未落地**：本体异常的”打开“（缺失 / 内容已变时的修复入口，随索引修复对话框）；`Ctrl+Z`；批量多选；草稿箱发起侧归档入口。

### 2026-09-17 — Phase 1 第九刀：归档撤销栏（可立即反悔）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 撤销语义 ✅ | `ArchiveService::undo_archive`：**本体移回原路径 + 硬删登记行**，三条守卫都不静默降级——版本 > 1（已再归档）拒绝、原位置已被占拒绝（撤销必须**精确**还原）、顺序与归档同构（本体先行、索引后动、**索引失败把本体搬回去**） | `src/service.rs` |
| 撤销凭据 ✅ | `ArchiveUndo { resource_id, name, source_path }`：**只在内存里活 5 秒**。归档本就不往库里记"本体原来在哪"（`promoted_from` 记的是来源草稿的**相对**路径，本地文件归档时为空），给所有存档加一列换一个几秒的窗口不划算；过期就没了，不会在库里留一个"看起来能撤销"的字段 | `src/model.rs` |
| 硬删行 ✅ | `AnalyticsResourceStore::hard_delete_row`（撤销归档与索引修复的"删孤儿记录"共用一条 SQL，`remove_orphan_record` 委托给它） | `src/resource.rs` |
| 撤销栏 ✅ | 面板底部（状态行**上方**，不随列表滚走）：`已归档「X」 撤销`；5 秒后自动消失，且**守卫"仍指向同一次归档"**（这 5 秒里又归档一次，新凭据不该被旧计时器清掉）——与 M5 撤销栏同一时长与形态 | `src/resource_view.rs` |
| 宿主 ✅ | `OpOutcome::{Archived 带 undo, Undone}` + `Job::Undo`（做完照样补一次取数）；**只有首次归档给凭据**（再归档的"撤销"是版本回退，服务层也会挡）；`request_undo_archive` 走同一套端口 | `crates/workbench/src/{services/resource_jobs.rs,components/resource_host.rs,panels/resources.rs}` |
| 事件 ✅ | `ChangeReason::Undone`（发/收两端同批：面板经宿主动作刷新） | `src/model.rs`、`src/service.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **77 单测 + 3 对话框窗口测试 + 8 面板窗口测试全绿**（+2 服务单测：撤销搬回本体并删行且事件有序；原位置被占 / 已有 v2 两种被拒且两边状态不变；+1 窗口测试：撤销栏随凭据出现与退场）；`cargo test -p rds-workbench --test ui_contract -j 2` 7 项全绿；`cargo check -p rds-workbench --lib -j 2` 零告警 | — |

**两处刻意的克制**：

1. **撤销窗口只有 5 秒、且只在内存**：它是"立即反悔"，不是一条待办；过期后要回退请走版本历史（`version.rs` 的还原入口，Phase 3）。
2. **状态栏回执保留**：撤销栏只回答"要不要反悔"，回执回答"它落到哪了"（`→ resources/x.sql`）——两件事，都不省。

**未落地**：`Ctrl+Z` 绑定（撤销栏的按钮是唯一入口）、批量归档的撤销（随批量批）、版本历史对话框（Phase 3）。

### 2026-09-17 — Phase 1 第八刀：归档 / 取回对话框与真执行（P1.4 / P1.5 部分）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 对话框（crate 内）✅ | `dialogs/archive.rs`：来源 / 目标位置 / 来源连接只读摆出 + 显示名 / 标签 / 保留历史内容可填；`dialogs/checkout.rs`：文件名 + 「取回后打开」勾选 + 版本提示。**表单状态、校验与渲染都在 crate**，执行交给宿主注入的 `on_submit`（照 `group_form_dialog` 形状） | `src/dialogs/{mod,archive,checkout}.rs`、`src/ui.rs`（+2 宽度常量） |
| 校验与解析 ✅ | 纯函数 + 单测：`parse_tags`（`,` / `，` / 空白分隔，去空去重保序）、`parse_keep_versions`（空 = 跟随设置，0–100，越界挡在对话框）、`name_hint` / `file_name_hint`（空 / 路径分隔符 / `.` / `..` / Windows 保留设备名——**注定失败的名字不进服务层**）、`suggest_work_copy_name` | 同上 |
| 冲突不静默覆盖 ✅ | 归档前探目标是否被占：`PayloadStore::{rel_path_taken, free_rel_path}`（**文件系统层面**，命名规则只在 crate 里写一次）；命中则对话框把"将归档为 resources/x-2.sql"原样摆出，确认按钮同时改文案。登记表层面的占用仍由服务层在提交时拒绝——两处各管一层 | `src/payload.rs` |
| 真执行 ✅ | `services::resource_jobs`：单线程队列扩成 `Refresh / Archive / Checkout` 三种作业——开库 + 组装 `ArchiveService` + 执行 + **顺手补一次取数**（不让界面停在"说成功了、列表没变"）；取回落点的重名避让（`-2`…`-999`）在动文件**之前**定死 | `crates/workbench/src/services/resource_jobs.rs` |
| 宿主接线 ✅ | `resource_host`：归档 = 系统文件选择 → 对话框 → 入队；取回 = 对话框 → 入队。回执文案由侧栏轮询印组装（它才有项目根与状态栏）：`已归档「X」v2 → resources/x.sql` / `已取回 scratchpad/x（工作副本）.sql（v3 的工作副本）；改完再归档将生成 v4`；勾了"打开"就递 `Shared::request_open_in_editor` | `crates/workbench/src/components/resource_host.rs`、`panels/resources.rs` |
| 刷新端口 ✅ | `ResourcesBridge`（照 `ScratchpadBridge`）：入队与轮询都在侧栏面板手里，而发起方在右栏详情 / 对话框回调——端口只此一份，面板之间不互订 | `panels/{shared,mod}.rs`、`view.rs` |
| 详情动作 ✅ | 详情面板底部动作区「取回（检出）…」：只读存档**唯一的编辑入口**；禁用时给出口（"本体异常，先在状态行『修复…』处理"）。其余动作各自被挡（打开只读 = P1.6 / 回收站 = P0.8 / 标签 = Phase 2 / 版本历史 = Phase 3） | `src/detail_view.rs`、`panels/right.rs` |
| 端口形状 ✅ | `ResourcesHost::request_checkout` 改收**整条 `ArchiveDetail`**（不再是 id）：宿主据此命名工作副本与提示版本，不必回头读面板选中态（那是渲染期正被借用的对象）；列表委托因此多持一份 `details` 映射 | `src/resource_view.rs` |
| 窗口测试 ✅ | +3 项：归档对话框开得出 + 按钮真在 + 预填值直接可提交 + 空名/非法份数挡住；带冲突的种子照样渲染；取回对话框同形状（默认名合法、路径分隔符挡住）。提交路径直接调 `submit_*`——**与按钮回调、Enter 走的是同一个函数** | `tests/dialog_window.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **75 单测 + 3 对话框窗口测试 + 7 面板窗口测试全绿**；`cargo test -p rds-workbench --test ui_contract -j 2` → 7 项全绿；`cargo check -p rds-workbench --lib -j 2` 零告警 | — |

**取舍与边界**（避免"看起来支持"）：

1. **归档入口只接了"本地文件"**：草稿箱右键「归档为存档…」是上游（草稿箱 crate）的入口，那边视图正在下沉重构，本刀不碰；面板头与空态的按钮相应改成中性的「归档…」/「选择文件归档…」。本地文件没有"来源草稿连接"可带出，`source_connection` 留空（**不猜**当前活动连接最多也不是它的来路）。
2. **分组 / 别名不进对话框**：原型 §4.1 的这两格要等 Phase 2 的分组与重命名入口（`folder.rs` 的改名/删除尚待补），现在摆上去也只是个摆设。
3. **回执用状态栏而不是撤销条**：原型 §4.1 的"归档后给撤销条"要复用 M5 的撤销栏（撤回 = 本体移回 + 删登记行），属另一批；本刀先把动作与回执做真。

**未落地**：撤销栏、草稿箱发起侧、版本历史 / 回收站 / 索引修复三个对话框（Phase 3；回收站还被 P0.8 阻塞）、只读三重守卫（编辑器侧 P1.6）、批量多选。

> 注：`Cargo.lock` 未随本刀提交（工作树里含其它模块的在途改动）。

### 2026-09-17 — Phase 1 第七刀：存档详情接入右栏（crate + workbench）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.3（续）✅ | 右侧新增面板档位「存档详情」（`RightPanel::Archive`）：只做转发渲染 + 空态，视图仍在 crate 内（`detail_view::render_detail`）——档位名/图标属外壳数据 | `crates/workbench_shell/src/model.rs`、`crates/workbench/src/panels/right.rs` |
| 数据链 ✅ | 详情与行**同一次取数产出**：`build_snapshot` 增 `history_counts` 入参，为每行落一份 `ArchiveDetail`（`ResourcesSnapshot.details`），面板按选中行 id 取（`selected_detail()`）——详情不在渲染期补取，也不额外开库 | `src/present.rs`、`src/resource_view.rs`、`crates/workbench/src/services/resource_jobs.rs` |
| 版本数一次查完 ✅ | `AnalyticsResourceStore::version_counts()`（`GROUP BY resource_id`）：逐行查会把一次刷新变成 N+1 次查询；不在结果里的行 = 无历史版本（写前快照语义下当前版本不进版本表） | `src/version.rs` |
| 时间口径 ✅ | 详情用**绝对时间**（`format_timestamp`，`%Y-%m-%d %H:%M`），行上仍用相对时间：列表窄要扫得快，详情是看"归档凭证"的地方要精确值；时区与 `connector.rs` 同口径（UTC，本地化是全局议题） | 同上 |
| 空值不空行 ✅ | "版本"分区在无历史版本时**整节不出现**（与其它分区同一口径）：一排"（无）"除了占地方没有信息量 | `src/detail_view.rs` |
| 联动 ✅ | 宿主观察**资产库面板实体**（不是 `SidebarPanel`：子实体的 `notify` 不级联到父面板）→ 右栏正显示存档详情时唤醒它重渲染；句柄存 `WorkbenchView` 私有字段。面板之间不互订，跨 crate 的视图也无从知道对方存在 | `crates/workbench/src/view.rs`、`crates/workbench/src/panels/resources.rs` |
| 契约 ✅ | `Shared` 新增 `resources_panel` 弱句柄进白名单（与 `mock_panel` / `insight_panel` 同例） | `crates/workbench/tests/ui_contract.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **69 单测 + 7 窗口测试全绿**（+2 详情单测）；`cargo test -p rds-workbench --test ui_contract -j 2` → 7 项全绿 | — |

**取舍记录**：详情落在**右 Dock 的档位**里，而不是原型 §3.1 的"编辑区右侧 20rem 属性面板"（`PROPERTY_PANEL_*`）——右 Dock 已有"面板档位"这套现成机制（切换/图标/快捷键/宽度记忆都由外壳管），而属性面板那条路要等 M4 的 `h_resizable` 容器接进编辑区。两者将来若合并，宽度口径按 `RIGHT_DOCK_WIDTH`（17.5rem）与 `DETAIL_PANEL_DEFAULT_WIDTH`（20rem）取一，**不要两份并存**。

**已知缺口**（本轮没做，均记在下）：

1. **关闭项目后左栏仍显示上一个项目的存档**（详情同理）——刷新只挂在"打开/切换项目"上（`project_host::on_opened`），宿主没有 close 回调可挂；要修需先给项目宿主补一个 `on_closed` 或让面板在无项目时清空快照。
2. 详情面板只做**只读信息区**：动作按钮（取回 / 版本历史 / 打标签 / 移入回收站）、内容预览与危险区随对话框批接入（原型 §3.1 的其余分节）。

**未落地**：五个对话框、只读三重守卫（编辑器侧）、批量多选（含 `F2` / `Ctrl+A`）。

> 注：`Cargo.lock` **未随本刀提交**——工作树里它还含其它模块的在途改动，一并提交会混入别人的 WIP。

### 2026-09-16 — Phase 1 第六刀：Action 与快捷键（crate + app 层）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.8 ✅ | 新增 `FocusSearch`（`Ctrl+F` 聚焦工具栏搜索框）与 `ClearSearch`（`Esc` 只清搜索词——种类 / 只看需处理留在菜单里，误清会让人以为筛选坏了）两个 Action + 面板内处理器；app 层把它们与已有的 `DeleteSelected` 绑到 `analytics-resource` context | `src/commands.rs`、`src/resource_view.rs`、`crates/app/src/main.rs`（+ `Cargo.toml` 依赖） |
| 命中路径修复 ✅ | 面板根元素补 `.track_focus(&self.focus_handle)`：**没有它面板不在 dispatch path 上**，app 层绑的键与 `.on_action` 根本落不到（编辑器面板已踩过同一个坑）——窗口测试派发 `ClearSearch` 时暴露并修正 | `src/resource_view.rs` |
| 行漫游与打开 ✅ | `↑↓` 与 `Enter` 不自己声明：列表组件的选中通道与 `confirm` 已接在 `ArchiveListDelegate` 上，两套键语义会打架 | 同上 |
| 窗口测试 ✅ | +1 项：聚焦面板 → 派发 `ClearSearch` → 断言只清搜索词、菜单条件保留（走生产入口：app 层的 `Esc` 就是派发它） | `tests/panel_window.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **67 单测 + 7 窗口测试全绿**；`cargo check -p rds-app -j 2` 零告警 | — |

**未落地**：`F2` 重命名（随重命名入口）、`Ctrl+A` 全选与批量动作（随多选批）、详情面板接入、五个对话框。

> 注：`Cargo.lock` **未随本刀提交**——工作树里它还含其它模块的在途改动（`rds-workbench-shell` 等），一并提交会混入别人的 WIP；下一次 `cargo` 构建会自动补上本刀的依赖边。

### 2026-09-16 — Phase 1 第五刀：列表虚拟化 + 行右键菜单（crate 内）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.1（续）✅ | 行列表从“手搓 `Button` 行 + `overflow_y_scrollbar`”换成 **`list::List`**（虚拟化 + 组件化 hover / 选中 / 键盘漫游）：`ArchiveListDelegate` 持有可见行的副本——`render_item` 在列表渲染期被调用，而那一刻面板正被借用，回头读面板的行集合会直接 panic（与 mock 的生成器搜索委托同例） | `src/resource_view.rs` |
| P1.2（续）✅ | 行的**动作入口改为右键菜单**（原型 §3.2 前三项）：打开（只读）/ 取回（检出）…（本体异常与只读项目禁用）/ ── / 移入回收站；行本身只承载信息——240px 里常驻按钮会把尾部字段挤没 | 同上 |
| P1.2（续）✅ | **kind 图标**：`FileText` / `Table` / `ExternalLink`（取自完整 Lucide 目录，组件子集没有表格形；枚举由 gpui-kit-assets 构建脚本按 svg 文件名生成，**编译通过即资产存在**，不会静默为空），一律 `muted`（颜色信号留给复现强度徽标） | 同上 |
| 选中语义 ✅ | 面板仍是语义权威（`selected` = 行 id）：组件回传的选中经 `set_selected_index` 落回面板；面板的选中在**渲染期镜像**回列表（`sync_list_selection`），镜像期间立标志禁止回写——否则就是“更新正在被更新的实体”（GPUI 直接 panic，实测踩到并已用窗口测试钉住） | 同上 |
| 窗口测试 ✅ | +1 项：宿主来回改选中并逐帧渲染**不重入**；原 3 项条件 / 排序用例照旧绿 | `tests/panel_window.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **67 单测 + 6 窗口测试全绿**（kind 图标映射单测 +1）；`cargo check --workspace --all-targets -j 2` 零告警 | — |

**取舍记录**：行内「打开 / 取回 / 移入回收站」三个按钮**下线**（改写进右键菜单）：原实现只在选中行出现，仍要占一行宽度；它们的正式替代是原型 §2.3 的 hover 版与详情面板的动作按钮，本批先保住“行的信息密度”。

**未落地**：详情面板接入、五个对话框、Action 与按键绑定（`Ctrl+F` / `↑↓` / `Enter` / `F2` / `Delete` / `Ctrl+A` / `Esc`）、行内 hover 动作、批量多选。

### 2026-09-16 — Phase 1 第四刀：workbench 接线（面板挂载 + 快照桥）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.1/P0.1（续）✅ | 左 Dock「资产库」分支由占位换成真面板：**构造期**创建 `ResourcesPanel` 实体 + 注入宿主端口，渲染只转发（`render_resources_placeholder` 下线） | `crates/workbench/src/panels/resources.rs`（新）、`panels/mod.rs` |
| 取数桥 ✅ | `services::resource_jobs`：单工作线程 + tokio（`ProjectDatabaseManager::open` → `list_file_archives` → `IndexRepair::scan` → `present::build_snapshot`）；结果槽只留最新一份；`enqueue_refresh` / `drain_snapshot` / `has_pending` | `crates/workbench/src/services/resource_jobs.rs`（新） |
| 宿主端口 ✅ | `components::resource_host`：`ResourcesHost` 实现——动作类请求给**明确回执**（状态栏提示“尚未接入 + 缺什么”），不接半条链路 | `crates/workbench/src/components/resource_host.rs`（新） |
| 触发点 ✅ | **事件路径**三处：活动栏切到资产库 · Quick Open「打开资产库」· 项目打开/切换（`project_host::on_opened`）——render 不发起任务（与 nav 面板“render 内入队”的既有债刻意区分） | `crates/workbench/src/view.rs`、`components/project_host.rs` |
| 回填 ✅ | `ensure_resources_pump`（照 `ensure_scratchpad_pump`）：60 ms 轮询 `drain_snapshot` → 推给面板实体；失败只给提示并**保留上一份列表** | `panels/resources.rs` |
| 契约 ✅ | 新面板模块登记进 `ui_contract` 的尺寸 / 颜色两份清单（契约 2c 强制：漏登会让两份契约对该文件静默失效） | `crates/workbench/tests/ui_contract.rs` |
| 验证 | 见下文 | — |

**两处刻意的克制**：

1. **不新增 `Shared` 字段**：面板实体句柄与轮询任务都在 `SidebarPanel` 的私有字段里，宿主经 `WorkbenchView::request_resources_refresh` 转一手（`Shared` 字段白名单契约因此无需变动）；
2. **`UntrackedFile` 不进行**：它没有资源行可标（未登记的东西不是存档），留给索引修复对话框呈现；本批只把 `缺失` / `内容已变` 折成行状态。

**已知成本**：每次刷新都会重算全部本体指纹（`IndexRepair::scan`）。它发生在工作线程上，但文件多时会慢；若将来成为瓶颈，先在 `indexer` 加“只查存在性”的快路径，**不要**删掉指纹比对（那会让“内容已变”静默失效）。

**未落地**：五个对话框（归档 / 取回 / 版本 / 回收站 / 索引修复）、编辑器只读打开（P1.6）、Action 按键绑定、虚拟化列表、右键菜单、行图标、`ProjectTrash` 上提（P0.8）。

### 2026-09-16 — Phase 1 第三刀：工具栏（搜索 / 筛选 / 排序）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.1（续）✅ | 工具栏（高 2rem）：搜索框（`Input`，占满剩余宽 + 自带清空钮）· `筛选 ▾`（种类多选 / 只看需处理，按钮上标"菜单条件个数"）· `排序 ▾`（名称 / 版本）；空库时也渲染（版式不随"有没有存档"上下跳） | `src/resource_view.rs`、`src/ui.rs`（+2 常量） |
| P1.2/P2.3（部分）✅ | **两种空态分开**：空库（条件为空 → 引导归档）vs 无匹配（条件非空 → 给"清空筛选"）；可见行 = 筛选 → 排序，**在事件路径算好**（`view_rows`），render 只读 | 同上 |
| 数据层 ✅ | `ArchiveKind::{ALL, label}`（菜单候选顺序与文案的单一来源）；`ResourcesFilter::{toggle_kind, has_kind, menu_dims}`（**全选规范化为不限**：不留"看起来在筛选"的等价条件，否则空库会被显示成"没有匹配"）+ `SortOrder::arrow` | `src/model.rs`、`src/filter.rs` |
| 选中语义 ✅ | 悬空选中改在 `refresh_view_rows` 里清：**筛选与排序也会让行消失**，只盯快照会留下指向"看不见的行"的选中态 | `src/resource_view.rs` |
| 窗口测试 ✅ | +3 项：条件改变可见行且被筛掉的选中被清、排序同键翻转 / 换键保持方向、无匹配态渲染 + 清空筛选（断言**输入框与条件同一次改**） | `tests/panel_window.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **66 单测 + 5 窗口测试全绿**（本刀 +3 单测 +3 窗口）；`cargo check -p rds-analytics-resource --all-targets -j 2` 零告警 | — |

**三条刻意的克制**（避免"看起来支持但算错"）：

1. **状态行仍报库口径计数**（不随筛选变化）：`缺失` / `索引异常` 是"修复…"入口的存在理由，被筛选隐掉就成"看起来没问题的库"；命中数靠"筛选 N"徽标 + 输入框可见。
2. **排序仍只有名称 / 版本**——行上没有 `size_bytes` / `updated_epoch`，拿格式化后的尾巴（`1.2 KB` vs `900 B`）比较会静默排错（见 `filter.rs` 模块头）。
3. **搜索只匹配显示名 + 尾部字段**（**第十一刀已拓宽**至别名 / 标签 / 来源表）：当初原型口径里的别名 / 标签 / 来源表需要 `ArchiveRow` 带上这几个列，先照行上看得见的字段做。

**实现期踩到的一个坑**（已写进代码注释）：`InputState::set_value` **不会**发 `InputEvent::Change`（gpui-component 内部注释明说）——程序性改词（清空筛选 / 宿主预填）必须自己同步条件，否则"输入框里的字"与"实际生效的条件"会静默不一致。故 `set_query` / `clear_filter` 是唯一入口，且有窗口测试锁住。

**未落地**：虚拟化列表（`list::List`）、右键菜单、行图标（`IconName` 子集未核实）、详情面板接入、五个对话框、Action 按键绑定（app 层）、**workbench 桥**。

### 2026-09-16 — 补记：Phase 1 接线前半与 `present.rs` 呈现层（追记两笔已提交但未入档的落点）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P0.1（续）✅ | workbench 依赖 `analytics_resource`（workspace 别名已就位）；活动栏标签与 Quick Open 文案 `资源分析` → **资产库**（提交 `f93d560`） | `crates/workbench/Cargo.toml`、`crates/workbench/src/view.rs` |
| P1.x ✅ | 呈现层：索引行 → `ArchiveRow` / `ArchiveCounts` / `ResourcesSnapshot`——`format_size`（< 1 KB 不给 `0.0 KB`）/ `format_scale`（千分位）/ `format_relative_time`（时钟回拨显"刚刚"而不是负值）/ `tail_for`（按 kind 的字段优先级）/ `build_snapshot`（异常态压过 kind）；宿主桥只剩"取数 → 调它 → 推快照"（提交 `51918d4`） | `src/present.rs` |
| 说明 | 这两笔提交当时未入 §0 与代码地图（同一窗口期内 `README.md` 还把 `present.rs` 漏在代码地图外），本刀一并补齐——文档落后代码的窗口期正是最容易丢线索的时候 | 本文件 + 模块 `README.md` |

### 2026-09-15 — Phase 1 第二刀：详情面板 + 工具栏数据层（crate 内）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.3（部分）✅ | 详情面板只读信息区：`ArchiveDetail` 快照 + `detail_rows`（基本信息含**只读说明** / 来源含**指纹缩略** / 版本 / 组织）+ `alert_line`（只在需处理时出现：缺失 / 内容已变 / **引用型常态就提示**）+ `render_detail`；空值不产生行（指纹除外，无值给破折号）。**动作按钮随对话框批接入**（不提前摆点不动的入口） | `src/detail_view.rs`、`src/ui.rs`（标签列宽） |
| P2.3（数据层）✅ | 工具栏规则独立成模块（面板与后续菜单共用）：`ResourcesFilter`（关键字（**匹配面**：显示名 / 别名 / 标签 / 来源表 / 尾部，大小写不敏感——宽出展示面的部分见**第十一刀**）/ 种类 / 只看异常）+ `SortField`/`SortOrder`（`flipped`、`label`）+ `apply_view`（筛选→排序；同键名称兜底且不随方向翻转）；`is_empty()` 决定面板显示"还没有存档"还是"没有匹配" | `src/filter.rs` |
| 窗口测试 ✅ | `tests/panel_window.rs`：空态 + 只读提示可渲染、**仅渲染不触发宿主动作**（断言）、快照推送驱动行与计数、宿主驱动选中、**行消失后悬空选中被清掉** | `crates/analytics_resource/tests/` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **58 单测 + 2 窗口测试全绿**；`cargo check` 零告警 | — |

**两条刻意的克制**（避免"看起来支持但算错"）：

1. 排序只支持行上**真实存在**的键（名称 / 版本）——按大小或归档时间排序会去比较格式化后的字符串（`1.2 KB` vs `900 B`）而静默排错，需先给 `ArchiveRow` 补 `size_bytes` / `updated_epoch`；
2. 窗口测试的宿主用替身只记录调用，不接服务层。

**未落地**：搜索框与筛选/排序菜单控件（数据层已就位，待 `Input` / `DropdownMenu`）、虚拟化列表（`list::List`）、右键菜单、五个对话框、详情面板动作、快捷键绑定、行图标（`IconName` 子集未核实）。

### 2026-09-15 — Phase 1 第一刀：面板骨架（crate 内）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.1/P1.2（部分）✅ | `ResourcesPanel`（`BasePanel` + `Panel` + `Focusable`）：面板头（标题 + 归档入口）、提示行（只读/通知分色）、行列表（显示名 / 版本徽标（v1 不显）/ **复现强度徽标** / 尾部字段 / 选中态）、状态行（异常时给"修复…"）、空态；宿主动作经 `ResourcesHost` 注入（5 个请求方法），面板**不自己取数** | `src/resource_view.rs`、`src/ui.rs` |
| 纯函数与单测 | `strength_badge` / `badge_tone`（`引用`=warning：复现最弱必须显眼）/ `row_tail`（字段优先级）/ `ArchiveCounts::line`（零桶不显示）+ 4 项单测 | 同上 |
| 接线 | `Cargo.toml` 加 `gpui-kit` 依赖 + dev-dependencies `test-support`（窗口测试待下一批） | `crates/analytics_resource/Cargo.toml` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **49 项全绿**；`cargo check` 零告警 | — |

**实现期踩到的两个坑**（已写进代码注释，供后续视图参考）：

1. **edition 2024 的 RPIT 会捕获入参生命周期**——区域渲染函数写 `-> impl IntoElement` 会借住 `cx`，连续调两个区域函数即"重复可变借用"。返回类型改具体（`Div` / `AnyElement`）后消失；
2. `overflow_y_scrollbar` 返回的不是 `Div`（滚动包装类型），带滚动的区域必须返回 `AnyElement`。

**未落地（下一批）**：搜索框与筛选/排序菜单、虚拟化列表（`list::List`，当前行用 `Button`）、右键菜单、详情面板、五个对话框、Action 与快捷键、行图标（`IconName` 子集未核实，先不引入）、`workbench` 侧接线（依赖 + 渲染入口，属跨 crate）。

### 2026-09-15 — Phase 0 第三批：索引修复（`indexer.rs`）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P0.11 ✅ | `IndexRepair`：`scan`（只读，**不改任何状态**）报告三类差异——有文件无记录 / 有记录无本体 / 指纹不匹配；三类修复动作均需人工确认：`adopt_file`（补登，指纹现算、不搬文件、来源留空）、`accept_current_content`（指纹换实际值 + 版本 +1 + **重新加回只读**）、`remove_orphan_record`（本体都没了，直接删行、不进回收站） | `src/indexer.rs`（新） |
| 零件 | `PayloadStore::list_files`（递归遍历本体目录、跳过隐藏项、`/` 分隔排序）、`AnalyticsResourceStore::{list_file_archives, remove_orphan_record}` | `src/{payload,resource}.rs` |
| 测试 | +8 项（7 索引修复 + 1 本体遍历）：三类差异各一、补登后转干净、重复补登被拒、接受当前内容后版本/指纹/只读三态正确且写前快照保留、本体不存在时拒绕 `accept_current_content` | `src/indexer.rs`、`src/payload.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **45 项全绿**；`cargo check` 零告警 | — |

**两条诚实语义**（写进实现与注释，不做表面修复）：

1. `accept_current_content` 产生的历史版本**只有元数据、没有内容副本**——旧内容在外部被覆盖时已经没了，界面按"副本缺失"呈现，而不是假装能还原；
2. "有记录无本体"的另一个动作**从回收站还原**依赖 `ProjectTrash`（P0.8），本期只提供"删除记录"，还原动作待 P0.8 接入。

**仍余**（2026-09-20 核实，去掉早已完成的旧条目）：`F2` 重命名待接（重命名入口本身在行右键菜单里）；`recycle.rs` 废弃（P0.8，跟 crate）。

### 2026-09-15 — Phase 0 第二批：归档 / 取回闭环

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P0.10（部分）✅ | `ArchiveService`：`archive`（首次归档：指纹 → 本体 move → 写登记，**索引失败回滚本体**）、`archive_into_existing`（再归档：指纹未变即**幂等**返回；变了才"旧内容留副本 → 写前快照 → 覆盖本体 → 版本 +1 → 按 keepVersions 裁剪"）、`checkout`（取回复制；拒绝落在 `resources/` 内的目标）+ `ResourcesChanged { reason, resource_id }` 广播（无订阅者不报错） | `src/service.rs`（新） |
| P0.7（续）✅ | 新列接入：`AnalyticsResource` 增 9 字段 + `RESOURCE_COLUMNS` / `map_resource_row` 同步；新增 `insert_archive`（归档专用写入，不走 v1 通用入口）、`update_archive_content`、`find_archive_by_rel_path`（归档前占用检测）；**行映射从 4 份收敛为 1 份**（`recycle.rs` / `tag.rs` 改调 `map_resource_row`，JOIN 用新助手 `qualified_resource_columns`） | `src/{models,resource,recycle,tag}.rs` |
| 测试 | 测试库改跑齐 007 + 020（此前只跑 007 → 测试库与生产库表结构不一致）；新增 7 项归档服务用例（含**故障注入**：用 SQLite 触发器让写索引必失败，断言本体回滚） | `src/tests.rs`、`src/service.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **37 项全绿**（16 存储 + 4 领域 + 10 本体 + 7 归档服务） | — |

**本轮定下的两条接口约定**（原型与手册已如此描述，此处落到代码）：

1. `save_resource_version` 现**返回快照行 id**，供资源行的 `parent_version_id` 指向本次写前快照；
2. `CheckoutRequest.dest_path` 由调用方给**绝对路径**——M6 不认识上游 `scratchpad/` 的目录结构（依赖方向 `scratchpad → analytics_resource`），只把自己的 `resources/` 管住。

**仍余**：`indexer.rs`（三类孤儿）、废弃 `recycle.rs`（P0.8，跨 crate）、版本保留策略接入设置项、`kind` 过滤/列表展示（Phase 1/2）。

### 2026-09-15 — Phase 0 首切片（仅 crate 内）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P0.1（部分） | 接线三件：crate 入口文档 ✅、workspace 别名 ✅（`analytics_resource = { path = …, package = "rds-analytics-resource" }`，惰性条目）；workbench 依赖 ⬜（属 Phase 1） | `crates/analytics_resource/README.md`、`Cargo.toml` |
| P0.4 ✅ | 迁移 `project_meta/020_analytics_resource_archive.sql`：9 个语义列（`kind`/`content_hash`/`file_rel_path`/`readonly`/`promoted_from`/`source_connection_id`/`source_table`/`definition_sql`/`archived_at`）+ `file_rel_path` 部分唯一索引（软删行不参与）+ kind/指纹索引；**不改 007** | `crates/engine/migrations/project_meta/` |
| P0.5 ✅ | 领域类型：`ArchiveKind` / `ReproductionStrength` / `ArchiveStatus` / `ArchiveBinding` / `ArchiveRequest` / `CheckoutRequest` / `CheckoutOutcome`（含 4 项单测） | `src/model.rs` |
| P0.6 ✅ | 本体层 `PayloadStore`：`resolve` 越界守卫（拒绝对/根相对路径、`..`、点前缀）、归档搬运（`rename` → 跨设备复制兜底、目标存在即拒绝）、只读标记、sha256 指纹、历史副本与裁剪（含 8 项单测） | `src/payload.rs` |
| P0.7（部分） | 已修：分页除零与负数、`LIKE` 转义、连接嵌套（新增 `get_resource_by_id_on`）、更新无事务（`BEGIN IMMEDIATE`）、影响 0 行不报错、`parent_version_id` 语义（指向快照行）、JSON 解析双策略（统一宽容 + warn）、乱码副本名；**待做**：新列接入（kind 过滤 / 指纹回填 / `file_rel_path` 唯一性） | `src/resource.rs`、`src/version.rs` |
| P0.9（部分） | `save_resource_version_on`：在调用方事务内写快照、返回快照行 id；并发冲突不再被静默吞（裸 `INSERT` 替代 `INSERT OR IGNORE`）。**待做**：指纹触发版本 + `keepVersions` 保留策略落库 | `src/version.rs` |
| P0.12（部分） | `mod tests` 补声明（**此前 560 行用例在 v2 从未编译**，`cargo test` 报 0 项）；新增 t016 库层契约测试（列 / 默认值 / `CHECK` 生效）。**待做**：测试改走 `engine::migration` 公共入口（现仍 `include_str!` 直执两段 SQL） | `src/lib.rs`、`src/tests.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **28 项全绿**（16 存储 + 4 领域 + 8 本体）；`cargo check -p rds-analytics-resource -j 2` 零告警 | — |

**本轮修正的两处「搬运期遗漏」**（属实修，不只是改文档）：

1. `crates/analytics_resource/src/tests.rs`（560 行）在 Round 11 搬运时**未在 `lib.rs` 声明 `mod tests`**，因此在 v2 从未被编译——文档里"15 项基线"实际是"0 项"。
2. v1 的 `t015_concurrent_update_same_resource` 断言"3 条版本（original + 2 updates）"，在写前快照 + `UNIQUE(resource_id, version)` 语义下**任何并发交错都不可满足**（两次更新最多落两条写前快照，当前版本不进表）。已按真实不变式重写：两次更新各留一条快照（v1/v2）、资源行版本号单调递增到 3。

**未做（需跟 crate 或属后续阶段，均已留档）**：engine 连接池 `busy_timeout` / `acquire` 超时（P0.2）、`.RSmeta` 常量去重（P0.3，现 5 处各自声明 + 1 处字面量）、`ProjectTrash` 上提中性化与 `recycle.rs` 废弃（P0.8）、`service.rs` / `indexer.rs`（P0.10/P0.11）。
>
> ~~视图四处占位（Phase 1）~~ **已完成**（2026-09-20 核实：四个文件都已是真实现，占位渲染已随 P1.1 下线；§1.2 的表是 v1 时点的盘点快照，留作追溯用）。

### 已确认决策（2026-09-15）

| # | 决策 | 出处 |
| --- | --- | --- |
| 1 | **语义取 C（混合模型）**：按 kind 分本体，统一对外"归档凭证"语义 | 架构 §0 D1/D2 |
| 2 | **命名**：模块 = 资产库；实体 = 分析存档；动作 = 归档 / 取回；"提升"一词只给 M1 | 架构 §2.4 |
| 3 | 三种 kind（`file` / `analysis` / `table_ref`）；**第一期只做 `file`** | 架构 §2.2 |
| 4 | 版本以 `content_hash` 触发，默认保留最近 5 份历史内容 | 架构 §5 |
| 5 | 回收站统一项目级 `ProjectTrash`，`origin = "resources"` | 架构 §7.1 |
| 6 | `scope` 改派生只读；`config` 降级为扩展位 | 架构 §4.2/§4.3 |
| 7 | 标签为主 + 单层分组；不做多级文件夹树 | 原型 §11 |
| 8 | 归档后只读，修改走取回；面板不提供"编辑资源" | 原型 §1 |
| 9 | 视图入本 crate（对齐 `overview.md`），若后续拍板"留 workbench"，§12 文件落点平移 | 架构 §8.2 |

## 1. 现状盘点

### 1.1 后端：持久层逐字搬运（可用，但带 12 项缺陷）

| 项 | 结论 |
| --- | --- |
| 可用资产 | `store` 层约 1300 行（`resource` 462 / `folder` 207 / `tag` 314 / `version` 83 / `models` 110 / `helpers` 30 / `tests` 560），**约 55% 可直接留用** |
| 逐字搬运的证据（Round 11 当时） | `diff --strip-trailing-cr` 对比 v1：**只差 `use` 路径一行**；`007_analytics_resources.sql` 与 v1 **完全相同**。注：Phase 0 首切片后就地修了 `resource.rs` / `version.rs`，这两个文件已不再逐字一致 |
| 必须作废 | `recycle.rs`（419 行）整体让位给 `ProjectTrash`；`version.rs` 重写为内容指纹版本 |
| 继承缺陷 | 12 项，逐条见架构 §13.1（`permanent_delete` 不彻底 / `total_pages` 除零 / 无事务 / 连接嵌套 / `parent_version_id` 恒等自身 id / 恢复丢归属 / 乱码 `(鍓湰)` 等） |

### 1.2 视图与命令：四处占位（**v1 时点快照**，现均已是真实现）

> 下表是 2026-09-15 盘点 v1 遗留时记的现状；四处占位已在 P1.x 批次全部落地
> （`resource_view.rs` 现 2402 行 / `model.rs` 370 / `commands.rs` 36 / `recycle_bin_dialog.rs` → `dialogs/trash.rs` 564）。
> 留作追溯：它记录了「开工前的样板量」。

| 文件 | 行数 | 状态 |
| --- | --- | --- |
| `src/model.rs` | 3 | 占位 |
| `src/commands.rs` | 3 | 占位 |
| `src/resource_view.rs` | 3 | 占位 |
| `src/recycle_bin_dialog.rs` | 3 | 占位 |
| `workbench/src/panels/` | `render_resources_placeholder`（当前 5846 起）| 占位文案仍是 v1 语义（"数据源连接引用 / DuckDB 分析表"）|

### 1.3 接线缺口（不补则视图永远落不了地）

| 缺口 | 落点 |
| --- | --- |
| workspace 未声明别名 | `Cargo.toml` `[workspace.dependencies]`（当前 41–52 行，行号会漂移） |
| workbench 未依赖 | `crates/workbench/Cargo.toml` |
| crate 无入口文档 | `crates/analytics_resource/README.md`（`project` / `scratchpad` 均有） |

### 1.4 底座缺陷（属 `engine`，但 M6 会被它卡死）

连接池 `Drop` 丢连接 + `acquire` 无限自旋 + 无 `busy_timeout`（架构 §13.2 #13/#14）——M6 的 `update_resource` 一次操作占 2 条连接而池只有 3 条，**Phase 0 必须先在 engine 层修掉**。

## 2. Phase 0 — 地基（无 UI，可独立验收）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P0.1 | 接线三件：workspace 别名 + workbench 依赖 + crate README | `Cargo.toml`、`crates/workbench/Cargo.toml`、`crates/analytics_resource/README.md` | `cargo check --workspace --all-targets -j 2` 零告警 |
| P0.2 | **engine 连接池修复**：`busy_timeout`、`acquire` 超时返回 `Err`（不再无限自旋）、归还语义不再静默丢连接 | `crates/engine/src/persistence/project_db.rs` | 单测：池压满后 `acquire` 在超时后报错而非挂起；并发写不再 `database is locked` |
| P0.3 | 项目元数据目录常量**收敛为单点定义**（拼写已在连接模块 C22 ③ 统一为 `.RSmeta`，残的是去重：现 5 处各自声明——`project::store::RS_META_DIR_NAME`、`project::lock::META_DIR`、`engine::connection_org_store::RS_META_DIR_NAME`、`scratchpad::store::META_DIR_NAME` + `engine::project_db` 字面量）；单一来源放 `engine`（`project → engine` 方向已定） | `crates/engine/src/…` + 各消费方 | 全仓 grep 无重复字面量/常量声明；Linux 大小写敏感场景有回归测试（参 `insight::rule_registry` 的 `test_project_rules_dir_uses_canonical_meta_dir` 写法） |
| P0.4 | 迁移 `project_meta/020_analytics_resource_archive.sql`（**建文件前重新核对编号**：编号先到先得，019 已被 insight 规则索引占用）：加 `kind` / `content_hash` / `file_rel_path` / `readonly` / `promoted_from` / `source_connection_id` / `source_table` / `definition_sql` / `archived_at`；**不改 007** | `crates/engine/migrations/project_meta/` | 迁移幂等；老库升级后旧行 `kind` 默认 `file`、`content_hash` 为空（首次打开标 `待指纹`） |
| P0.5 | 领域类型：`ArchiveKind` / `ArchiveSource` / `ArchiveStatus`（正常/缺失/内容已变）/ 请求响应结构 | `crates/analytics_resource/src/model.rs` | 单测：kind 与状态序列化稳定 |
| P0.6 | 本体层 `payload.rs`：`resources/` 定位与越界拒绝（含 `.RSmeta` 与点前缀）、move 与跨设备 copy 兜底、只读设置、`sha256` 指纹、历史副本读写 | `crates/analytics_resource/src/payload.rs` | 单测：越界路径全部被拒；只读设置失败只警告；指纹对同一内容稳定 |
| P0.7 | store 改造：加列读写、kind 过滤、**修 12 项继承缺陷**（尤其 `total_pages` 除零、`page_size ≤ 0`、`LIKE` 转义、事务化、连接不再嵌套、`created_by`/乱码） | `src/{resource,folder,tag}.rs` | v1 的 15 个用例全绿（改为走迁移系统）；新增边界用例 |
| P0.8 ✅ | **`ProjectTrash` 上提 + 中性化**（2026-09-18 落）：类型去 M5 化（`TrashKind`，还原返回 `TrashRestoreOutcome`）、来源是字符串 `origin`（校验交给调用方）、按来源清空的窄口 `empty_origin`、归属移到 `engine::persistence::trash`；`scratchpad/src/trash.rs` 删除并重导旧路径；M6 侧接入移入 / 还原 / 永久删除 / 清空 + 回收站对话框 | `crates/engine/src/persistence/trash.rs` ← `crates/scratchpad/src/trash.rs` | M5 现有回收站测试全绿（36 项）；M6 删除→还原往返（t124/t125）+ 跨模块拒绝（t125/t128）+ 按来源清空（engine 用例）全绿 |
| P0.9 | 版本重写：`content_hash` 触发、`parent_version_id` 语义修正（或删列）、历史内容按 `keepVersions` 保留/裁剪 | `src/version.rs` | 单测：**hash 未变不产生新版本**；hash 变则 +1 且旧内容仍在 |
| P0.10 | 服务门面 `service.rs`：归档/取回/检索/修复编排 + `ResourcesChanged` 事件（含 `reason`） | `src/service.rs` | 单测：归档全链路含回滚；事件载荷正确 |
| P0.11 | 索引修复 `indexer.rs`：三类孤儿检测与**人工确认**后的修复动作 | `src/indexer.rs` | 单测：有文件无记录 / 有记录无文件 / 指纹不匹配 三态各一用例 |
| P0.12 | 测试改道：`tests.rs` 的 `include_str!("…007…")` 改为走 `engine::migration` 公共入口 | `src/tests.rs` | 新增 020 后测试自动带出新列 |

## 3. Phase 1 — 能用的资产库（面板 + 归档/取回闭环）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P1.1 | 面板骨架：面板头（标题 + `＋▾` + `⋯`）、工具栏（搜索 / 筛选 / 排序）、行列表（虚拟化）、底部状态行 | `src/resource_view.rs`、`workbench/src/panels/` | 切换活动栏可见；`>100` 项流畅；状态行计数正确 |
| P1.2 | 行渲染：kind 图标（`muted`）+ 显示名 + 版本徽标（v1 不显示）+ **强度徽标** + 尾部字段（按字段优先级规则） | `src/resource_view.rs` | 三类 kind 行可区分；240px 无异常折行（溢出省略 + tooltip） |
| P1.3 | 详情属性面板（右侧，默认 20rem，宽度记忆）：基本信息 / 来源 / 版本摘要 / 标签与分组 / 内容预览 / 危险区；`file` 型首版 | `src/detail_view.rs`、`workbench/src/panels/` | 选中行切换联动；只读锁标记与"需取回编辑"提示常显 |
| P1.4 | **归档入口**：草稿箱右键「归档为存档…」+ 面板头「从草稿箱归档…」；确认对话框（显示名 / 目标位置只读 / 分组 / 标签 / 来源连接自动带出 / 保留历史 / 冲突处理） | `src/dialogs/archive.rs`、`crates/scratchpad/src/…`（发起） | 归档后草稿消失、资源只读、两侧面板同步刷新（事件链路通） |
| P1.5 | **取回（检出）**：右键 → 对话框（目标名 / 目标目录 / 是否打开）→ 复制到草稿箱 + 草稿侧记 `derived_from_resource_id` | `src/dialogs/checkout.rs`、`scratchpad` | 本体不动；重复取回自动改名避让 |
| P1.6 | 只读三重守卫：写入 API 拒绝（应用守卫）+ 编辑器只读打开 + 文件系统属性（辅助） | `payload.rs`、`workbench` EditorPanel | 任何写入路径返回明确错误文案；编辑器以只读态打开 |
| P1.7 | 术语与入口收尾：活动栏标签 `资源分析`→**资产库**；Quick Open 文案同步；删占位渲染；`LeftPanel::Resources` 图标复核 | `workbench/src/view.rs`、`panels/` | 全仓无"资源分析"作为模块名出现；无占位文案残留 |
| P1.8 | Action 与快捷键：`Ctrl+F` / `↑↓` / `Enter` / `F2` / `Delete` / `Ctrl+A` / `Esc`；`Ctrl+Shift+A`（草稿箱上下文） | `src/commands.rs`、`crates/app/src/main.rs` | 窗口测试：漫游与打开/删除分支 |

## 4. Phase 2 — 组织与检索

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P2.1 ✅ | 标签：新建/改名/删除（**补 v1 缺失的改名与删除**）、打标/去标、按标签检索、chips 渲染 —— **已落（2026-09-18，第一 / 三刀）**：存储层四项 + `dialogs/tag.rs`（勾选 / 新建并打上 / 行内 ⋯：重命名 / 删除）+ 详情 chips + 筛选菜单标签维（id 多选并集） | `src/tag.rs`（改名 / 删除 / 批量）、`src/dialogs/tag.rs`（未单独建 `tag_view.rs`：标签 UI 就藏在详情面板、筛选菜单与这个对话框里，没有独立视图） | 同名（未删）拒绝；删除标签清关联——t017 + `dialogs::tag` 三项单测钉住 |
| P2.2 | 分组：单层分组的新建/改名/删除/移动（含批量移动与拖拽到分组头）—— **已全落**（第二 / 三 / 七 / 十二刀）：建/改/删 + 移动语义 + 折叠区 + 「移动到分组 ›」+ 分组头右键 + 折叠态记住（`resources.collapsed_groups`，按项目分桶）+ **拖拽行到分组头**（`src/dnd.rs`，落点只有分组头） | `src/folder.rs`（已落）、`src/resource_view.rs`（分区 + 两个菜单 + 拖拽落点）、`src/dnd.rs` | 空分组可见（已满足：头恒在） |
| P2.3 | 搜索与筛选：名称 / 别名 / 标签 / 来源表；筛选三维（kind / 强度 / 标签）；排序（名称 / 归档时间 / 更新时间 / 大小 / 版本）—— **已全落**（排序 = 第四刀；搜索匹配面 = 第十一刀，另含尾部字段；强度维并入 kind + “只看需处理”） | `src/resource.rs`、`src/resource_view.rs`、`src/filter.rs`（搜索与排序） | 转义 `%`/`_`；非法排序字段回退；`page_size ≤ 0` 不再 panic |
| P2.4 | 设置项：`keepVersions` / 默认排序 / 默认分组 → `settings.json`（**不用 localStorage**，对照 v1）—— **`keep_versions` / `default_sort` / `collapsed_groups` 已落**（第五 / 六 / 七刀）；**余**：默认分组 | `crates/settings`、`src/service.rs` | 重启后保持 |
| P2.5 | 多选与批量：批量打标签 / 批量移动 / 批量删除（含数量提示） | `src/resource_view.rs`、`src/commands.rs` | 多选态菜单按数量自适应（v1 的缺陷） |

## 5. Phase 3 — 版本与恢复

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P3.1 | 版本历史对话框：版本表 + 相邻差异摘要 + **选中版本动作栏**（还原为当前版本 / 取回该版本为草稿 / 删除该版本内容副本） | `src/version_view.rs` | 不提供行级 diff；还原生成新版本而非覆盖 |
| P3.2 | 历史内容保留策略落地：`keepVersions`（默认 5，`0` = 只留元数据）+ 副本缺失标记 | `src/payload.rs`、`src/version.rs` | 裁剪只删副本，版本行保留；副本缺失有徽标 |
| P3.3 | 回收站对话框（仅 `origin = "resources"`）+ 跨模块条目禁用提示 + 撤销条 | `src/recycle_view.rs` | 永久删除真删 payload；跨模块还原被拒 |
| P3.4 | 索引修复对话框：三分组 + 动作；状态行异常段可点 | `src/dialogs/index_repair.rs` | 三类问题各可修复；**无自动修复路径** |
| P3.5 | 异常态呈现：本体缺失（灰显 + `danger` 点 + 横幅）、内容已变（`warning` 徽标） | `src/resource_view.rs`、`src/detail_view.rs` | 缺失项不隐藏、仍可删除/还原 |

## 6. Phase 4 — `Analysis` 档（DuckDB 表）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P4.1 | `analysis` 型本体层：表/视图存在性、`definition_sql` 采集（`SHOW`/`duckdb_tables`）、行数×列数、结构摘要指纹 | `src/payload.rs`（analysis 分支） | 指纹对结构变化敏感、对行数变化策略明确（见风险 R4） |
| P4.2 | 上游接入：**M7 Mock 产物**归档（对齐 `mock_persist_as_asset` 的文档/实现落差，二选一并同步文档） | `crates/mock`、`src/service.rs` | Mock 生成后可一键归档，指纹与定义可查 |
| P4.3 | 上游接入：**M5 编辑器结果落库后归档** | `workbench` EditorPanel、`src/service.rs` | 归档后可"重新执行定义"复算 |
| P4.4 | 打开路径：`analysis` 型双击 → 结果表格（复用编辑器结果区组件） | `workbench` | 只读呈现 |
| P4.5 | 上游接入：**草稿箱数据文件 → `analysis`**（CSV / Parquet / Excel / JSON：归档时文件作本体，分析走配方） | `crates/scratchpad` 的 C6/D2 路径（提升/归档）、`src/service.rs` | 归档后能洞察能复算；溯源（`promoted_from` + `content_hash`）齐全 |
| P4.6 | 上游接入：**数据库导航表 → 分析资产**（先定档位：`table_ref` 记引用 vs `analysis` 物化快照，见下方建议） | `crates/database` 导航菜单 + `src/service.rs` | 归档后能洞察；引用型带失效警告与“立即校验”（同时回答 Phase 5 的前置问题） |

> **数据要不要复制？（2026-09-18 建议 · 待拍板）**
>
> **默认不复制**：归档只存「**配方**（`definition_sql`）+ **指纹**（`content_hash` / 行数×列数 / 结构摘要）」，
> 本体就是 `resources/` 里那份已只读冻结的文件（草稿箱归档本就是 **move**，零额外磁盘）。
> 理由：**溯源不需要复制数据**——「文件 + 配方 + 指纹」三件套可审计；反而一张物化表的来源更说不清。
> 文件被动手脚的情况由 M6 既有的「内容已变 / 本体缺失」异常态覆盖（存档不静默变形）。
>
> **按需物化**（只用于反复扫 / 跨源 JOIN 这类重查询）走洞察侧的临时表机制（`tmp_i_` + TTL/上限），
> **不进资产**——延伸一条边界：**洞察不物化，归档默认也不物化；“谁为了查询快而物化，谁负责回收”**。
>
> **确实要长期物化时：复制成 Parquet**（列存 + 压缩，常比原 CSV 更小），**不要** `CREATE TABLE AS SELECT`
> 灌进 `analytics.duckdb`（内联存储可能 1–2×）。并按 `content_hash` 做**内容寻址去重**
> （`resources/by-hash/<sha256>.parquet`）：同一份内容被多个资产引用时只存一份。
>
> **导航表升级（P4.6）按同一判据：默认 `table_ref`（记引用）**——源表是“活的”，物化快照会**立刻过时**
> （挂着“分析资产”的名字却早已不是那个数，比失效引用更容易骗人）；真要快，走上面的按需物化。

## 7. Phase 5 — 待定（不承诺）

| 项 | 前置条件 |
| --- | --- |
| `TableRef` 档（远端表引用） | 先回答"用户真的需要这个书签吗"；若做，必须带失效警告条与"立即校验" |
| M1 系统级提升（项目→系统级） | 等 M1 设计落地；本模块已把 `scope` 改为派生只读，届时只需按库位置派生 |
| 依赖追踪（v1 设计文档的 `resource_references`） | 先有真实消费方（"删除前检查被谁引用"），否则不建表 |
| FTS5 全文检索 | 先量：条目 > 1000 且 `LIKE` 实测不可用再做 |

## 8. 明确不做

| 项 | 原因 |
| --- | --- |
| 多级文件夹树 / 面包屑 / 手动排序号 | v1 是残的（无改名/删除/移动/排序），投入产出比差；标签 + 单层分组已够 |
| 页码分页（v1 的 10/20/50/100） | 桌面应用语义：滚动 + 虚拟列表，不做翻页 |
| 前端 LRU + TTL 缓存 | 本地 SQLite 无 IPC 成本；v1 该实现本身有缺陷 |
| 拖拽到 SQL 编辑器 | 有"取回→打开"路径，复杂度不值 |
| `config` JSON 手工编辑 | 字段进表单、扩展进详情面板 |
| 版本行级 diff | 文本 diff 归编辑器 |
| 自动索引修复（静默导入/删除） | 会让"存档"变成用户没同意过的东西 |
| 归档本体的原地编辑 | 一旦允许，`content_hash` 与版本失去意义 |
| v1 设计文档中从未实现的 17 条命令（`extract_table` / `generate_sql_reference` / `check_delete_safe` / `cleanup_expired` …） | 无消费方、无真实需求；**不承诺** |

## 9. 测试场景

| # | 场景 | 期望 |
| --- | --- | --- |
| T1 | 归档普通 `.sql` 草稿 | 文件移至 `resources/`、只读、登记行 `kind=file`、`content_hash` 非空、v1、事件发出 |
| T2 | 归档目标已存在 | 询问改名/取消，**不静默覆盖**；取消后源文件仍在草稿箱 |
| T3 | 归档第 4 步成功、第 6 步失败（模拟索引写失败） | 本体 move 回滚，草稿箱恢复原状 |
| T4 | 取回 | 草稿出现副本、本体未变、再次归档版本 +1 |
| T5 | 归档内容未变（取回后原样再归档） | **不产生新版本** |
| T6 | `keepVersions = 5` 且已 7 个版本 | 只保留最近 5 份**内容副本**，7 个版本行全在 |
| T7 | 删除 → 回收站 | 本体进 `.RSmeta/trash/`、`origin = "resources"`、登记行移除；还原可回原路径 |
| T8 | 永久删除 | payload 真删（对照 v1 的"只删回收站行"缺陷） |
| T9 | 跨模块还原 | 草稿箱条目在资产库还原被拒，提示"请在草稿箱还原" |
| T10 | 本体缺失（手工删文件） | 行灰显 + `danger` 点；详情横幅；可"删除记录"/"从回收站还原" |
| T11 | 指纹不匹配（手工改文件） | `warning` 徽标"内容已变"；可"接受当前内容（生成新版本）" |
| T12 | 有文件无记录（手工放文件进 `resources/`） | 重建索引**列出**候选，用户确认后才补登 |
| T13 | 越界写入 | 经 API 写 `resources/` 或 `.RSmeta/**` 一律被拒，错误文案指向"先取回" |
| T14 | 搜索边界 | 输入 `%` 不命中全表；`page_size = 0` / `page = -1` 不 panic；非法排序字段回退 |
| T15 | 跨设备归档（`rename` 失败） | 退回复制 + 删除，结果一致（可用不同卷的临时目录模拟） |
| T16 | 非 ASCII 名 / 超长名 / 含空格名 | 归档、取回、还原全链路正常；分组头与列表显示正确 |

**基线**：v1 的 15 个存储用例全绿（改造后不得减少），新增用例随 Phase 落地。

## 10. 风险

| # | 风险 | 对策 |
| --- | --- | --- |
| R1 | 双真相源（文件系统 + 索引）不一致 | 明确"文件系统权威"；三类孤儿都有检测与人工修复入口；归档按"先本体、后索引、失败回滚"顺序（架构 §6.3） |
| R2 | 归档是对用户不可逆的动作（草稿从工作区消失） | 底部撤销条（复用 M5）+ 归档确认对话框明示"文件将移动到 resources/ 并变为只读" |
| R3 | 只读属性在 Windows/网络盘不可靠 | 应用层守卫为主，属性为辅；设置失败只警告（不阻塞归档） |
| R4 | `Analysis` 型指纹语义含混（表数据会变，结构不变） | 第一期不做；第二期先定"指纹覆盖定义+结构，行数只作元信息"并写进 UI 文案 |
| R5 | 面板塞不下（240px）信息 | 字段优先级规则 + tooltip；必要时放宽 Dock 起步宽（需同步 `ui.rs` 与契约测试） |
| R6 | 与 M5 归档发起方的耦合 | M6 只接受 `PathBuf` + 元数据入参，不依赖 `ScratchpadStore` 类型；依赖方向 `scratchpad → analytics_resource` |
| R7 | 事件链路再次"发了没人听"（v1 教训） | 事件必须带 `reason`，且**发/收两端同批落地**；验收含"两侧面板同步刷新" |
| R8 | 历史内容副本导致磁盘膨胀 | 默认只留 5 份；裁剪只删副本；详情面板显示"历史内容占用" |
| R9 | engine 池缺陷未修完就做 M6 | P0.2 是 Phase 1 的硬前置（否则 UI 会卡死，且难定位） |

## 11. 验证命令

```sh
# 模块回归
cargo test -p rds-analytics-resource --lib -j 2
cargo test -p rds-scratchpad --lib -j 2     # 回收站上提后的回归
cargo test -p rds-engine --lib -j 2         # 池修复与目录常量

# 契约（零裸色 / 零裸 px）
cargo test -p rds-workbench --test ui_contract -j 2

# 全工作区守卫
cargo check --workspace --all-targets -j 2

# 真机
cargo run -p rds-app -j 2
```

- 真机矩阵：明暗主题 × 三类 kind × 异常三态（缺失 / 内容已变 / 引用未校验）。
- 平台矩阵：Windows（只读属性最弱）/ macOS / Linux（大小写敏感 + 无只读属性语义差异）。

## 12. 实现位置映射（决策 → 文件）

| 决策 / 能力 | 文件 |
| --- | --- |
| 语义裁决、kind 模型、数据流 | `analytics-resource-architecture.md` §0/§2/§6 |
| 面板 / 列表 / 状态行 | `crates/analytics_resource/src/resource_view.rs` |
| 详情属性面板 | `crates/analytics_resource/src/detail_view.rs` |
| 版本历史 / 回收站 / 分组 / 标签对话框 | `src/{version_view,recycle_view,folder_view,tag_view}.rs` |
| 归档 / 取回 / 索引修复对话框 | `src/dialogs/{archive,checkout,index_repair}.rs` |
| 领域类型 | `src/model.rs` |
| 本体层（fs + 只读 + 指纹 + 历史副本） | `src/payload.rs` |
| 索引层（登记 / 分组 / 标签 / 版本） | `src/{resource,folder,tag,version}.rs` |
| 服务门面 + 事件 | `src/service.rs` |
| 索引修复 | `src/indexer.rs` |
| Action / 快捷键 | `src/commands.rs` + `crates/app/src/main.rs`（`analytics-resource` context） |
| 左 Dock 装配（仅协议） | `crates/workbench/src/{view.rs,panels/}` |
| 迁移 | `crates/engine/migrations/project_meta/020_analytics_resource_archive.sql` |
| 项目级回收站（上提后） | `crates/engine/src/…`（现 `crates/scratchpad/src/trash.rs`） |
| 尺寸常量 | 与外壳同语义的重导出：`crates/analytics_resource/src/ui.rs` ← `crates/workbench_shell/src/ui.rs`（`ROW_HEIGHT` / `PANEL_HEADER_HEIGHT` / `ICON_SIZE_SM` / `CONTROL_HEIGHT_SM` / `GROUP_BAR_WIDTH` / `HAIRLINE`，单一来源）；**本 crate 自有**的（对话框宽 / 表格列宽 / 列表上限 / 徽标高 / 工具栏高）就在 `crates/analytics_resource/src/ui.rs`（见原型 §7） |
| 行态口径与共用原语 | 行外壳 `resource_view.rs::list_row`（压掉 `ListItem` 默认 `py_1`，行距 = `ui::ROW_HEIGHT`）· 分组头展开指示 `workbench_shell::tree::{disclosure_slot,disclosure_icon}` · 分组头 `Enter` 折叠 `resource_view.rs::{header_fold_key,set_selected_index,confirm}` · 对话框行 `dialogs/version.rs::version_line`（`ListItem`）· 勾选 `dialogs/tag.rs`（`Checkbox` + `set_checked`） |
| 契约测试范围 | `crates/workbench/tests/ui_contract.rs`（**不扫本 crate**，见原型 §7 脚注）；本 crate 的尺寸 / 行态口径靠 `tests/{panel_window,dialog_window}.rs` 的断言守着 |
