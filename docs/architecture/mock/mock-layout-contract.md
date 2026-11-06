# Mock 布局契约：FigCraft 设计稿作为几何真源

> **状态**：链路已验证（2026-11-06，B 方案试跑）。设计稿 = `figcraft/mock-column-zone.figcraft`，
> 可重放输入 = `figcraft/mock-column-zone.json`，渲染参照 = `figcraft/mock-column-zone.png`。
> 目的：把「布局对不对」从**肉眼验收**变成**可对照的数值**，减少同一 UI 问题反复返工。

## 1. 为什么要有这份文件

`mock-column-zone-redesign.md` §4.1 的数值表是文字口径（「行高 34px」「列名格 10rem」），
它不产生任何可校验产物：代码可以偏离而没人发现，偏离也只能等打开 app 才看见。
本文件补上两样东西：

1. **一份可打开的稿**（`.figcraft`）：布局意图画出来，每个元素的 x/y/宽/高、padding、gap 都可读回数值。
2. **一张对账表**（§3）：稿上的值 ↔ `ui.rs` 常量 / render 原语 ↔ 结论（一致 / 差异 / 推断）。

§4.1 的数值以本文件 §2 的实测值为准；改口径顺序是：**先改稿 → 再改本表 → 再改代码**。

## 2. 链路与重放

本机已装 FigCraft 0.15.1（`D:\Program Files\ArtCraft\FigCraft\`），CLI `figcraft-cli.exe`
**不在 PATH**，必须绝对路径调用。全部操作走 `run`（无头执行 JSON 命令脚本，整脚本一个 undo 步、
all-or-nothing），完稿后用 `inspect.properties` / `node.get` 读回结构化数值。

```powershell
$cli = "D:\Program Files\ArtCraft\FigCraft\figcraft-cli.exe"
$d   = "<repo>\.rds\tmp\figcraft"

# 出稿（覆盖旧稿）
& $cli run "$d\mock-column-zone.json" --out "$d\mock-column-zone.figcraft" --automation-write-root $d

# 视觉参照
& $cli export "$d\mock-column-zone.figcraft" -o "$d\mock-column-zone.png" `
    --automation-read-root $d --automation-write-root $d

# 数值（逐节点 bounds / sizing / layout），python 批量 node.get 后成表
python "$d\measure_all.py"
```

脚本语法要点（踩过的坑，别再试）：

- 脚本必须是 **JSON 数组** `[{"command": id, "params": {...}}]`，单个对象会报 `expected value`。
- `node.create` 的顶层参数**只有** `type/x/y/width/height/parent/index/name/text/characters/path/points/closed/props/select`；
  `sizingH`、`padding`、`fill` 这些必须放进 `props`，放顶层直接 `unknown parameter`。
- **`padding` / `layout` / `gap` 只能给 frame**：给 TEXT 会报 `padding is an auto layout property`，
  表格单元格要先包一层 frame 再把文字放进去。
- 引用：`"as": "name"` + `{"$ref": "name"}`，取子字段 `{"$ref": "name", "field": "ids/0"}`。
- `node.get` 只返**一层** children；`outline: true` 也只给一层轮廓（id/name/type/childCount），
  要全树就按 id 批量 `node.get`。

## 3. 设计稿实测几何（1280×800，正文区宽 1280）

| 元素 | bounds (x,y w×h) | sizing | layout |
| --- | --- | --- | --- |
| 段1 基本信息 | 0,0 1280×100 | fixed/fixed | vertical gap 8 / pad 8 |
| ├ 动作行 | 8,54 1264×32 | fill/fixed | horizontal gap 8 |
| │ ├ 生成 | 8,54 64×32 | fixed | pad [0,10,0,10] |
| │ ├ 持久化到项目分析库 | 80,54 152×32 | fixed | pad [0,10,0,10] |
| │ ├ 追加到既有表 ▾ | 240,54 116×32 | fixed | pad [0,10,0,10] |
| │ ├ 保存到草稿箱 ▾ | 364,54 112×32 | fixed | pad [0,10,0,10] |
| │ ├ 另存为 ▾ | 484,54 88×32 | fixed | pad [0,10,0,10] |
| │ └ 状态格 | 580,54 692×32 | fill/fixed | horizontal gap 8，**右对齐**（mainAlign=max） |
| 段2 列管理区 | 0,100 1280×400 | fill/fill | vertical gap 4 / pad 20 |
| ├ 列区操作行 | 20,120 1240×24 | fill/fixed | horizontal gap 8 |
| │ ├ 计数「3 列」 | 20,120 23×15 | hug | — |
| │ ├ 场景模板 ▾ | 51,120 96×24 | fixed | pad [0,8,0,8] |
| │ ├ 弹性空档 | 155,120 689×24 | **fill** | — |
| │ ├ 筛选（输入框） | 852,120 144×24 | fixed | pad [0,8,0,8]，内凹底 + 描边 |
| │ ├ 预设 ▾ | 1004,120 72×24 | fixed | pad [0,8,0,8] |
| │ ├ 导入结构 | 1084,120 88×24 | fixed | pad [0,8,0,8] |
| │ └ ＋加列 | 1180,120 80×24 | fixed | pad [0,8,0,8]，**最右**（右端 1260 = 1280−20） |
| ├ 列行（3 行） | 20,148 / 20,186 / 20,224 1240×**34** | fill/fixed | horizontal gap 8 / pad **[4,12,4,12]** |
| │ ├ 列名条 | x+12 **176×20** | fixed | pad **[0,4,0,4]**，内凹底 + 描边 + 圆角 3 |
| │ ├ 类型徽标 | x+196 52×18 | fixed（内容定宽） | pad [0,4,0,4]，**独立成段** |
| │ ├ 生成器格 | x+256 144×20 | fixed | pad [0,4,0,4] |
| │ ├ 参数格 | x+408 720×20 | fill | — |
| │ └ 操作格 | x+1136 92×20 | fixed | horizontal gap 4，4 个 20×20 |
| 段3 预览 | 0,500 1280×300 | fill/fixed | vertical gap 8 / pad 8 |
| └ 预览标题行 | 8,508 1264×24 | fill/fixed | mainAlign=spaceBetween |

设计稿与实机差异（有意保留，非错误）：

- 段1 第一行「表名 / 行数 / 种子 / 语言」实机是 4 组输入框（`NUM_INPUT_WIDTH=5rem`），
  稿里用一句占位文本表达，免得每次改输入框宽都要重画。
- 段2 的 pad 20 = 代码的 `p_2()`（8，外层）+ `card` 内层 `p_3()`（12）；稿里合并为一层，
  代码是两层夹一个卡片框（见 §3 差异 D1）。
- 预览表体（表头 26 / 数据行 26）来自组件库 `DataTable` 的 XSmall 档，不是 render 里手写的常数。

## 4. 对账表（稿 ↔ 代码）

| # | 项 | 稿 | 代码 | 结论 |
| --- | --- | --- | --- | --- |
| 1 | 段1 初始高 | 100 | `DETAIL_HEADER_HEIGHT=px(100.)` | 一致 |
| 2 | 段1 拖拽范围 | 未表达 | 64..220（`DETAIL_HEADER_MIN/MAX_HEIGHT`） | 契约表登记；**下界必须小于初始高** |
| 3 | 段1 内距 / 行距 | 8 / 8 | `p_2()` / `render_design_head` 的 `gap_2()` | 一致 |
| 4 | 动作行高 | 32 | `Button` Medium = `h_8()`（vendor `button.rs:701`） | 一致（初稿误写 28，已改） |
| 5 | 动作行 gap | 8 | `render_sinks` 的 `gap_2()` | 一致 |
| 6 | 动作行按钮顺序 | 生成→持久化→追加→草稿箱→另存为 | `render_sinks` children 同序 | 一致（初稿虚构「批量落库 / 导出」，已删） |
| 7 | 状态格对齐 | 右对齐（flex 吃剩余） | `flex_1().min_w_0().justify_end()` | 一致（初稿画成定宽 320 左对齐，已改） |
| 8 | 状态格进度条 | 96×4 | `w(rems(6.))=96` + `JobBarSize::Mini`(4px) | 一致（初稿 64，已改） |
| 9 | 取消按钮 | 48×20 | `button(...).xsmall()` = `h_5()`=20 | 高一致；宽由文案定 |
| 10 | 列区段内距 | 20（合并示意） | `p_2()` 8 + `card` + `p_3()` 12 | **差异 D1**（结构层数） |
| 11 | 列清单行距 | 4 | `gap_1()` | 一致 |
| 12 | 列行高 | 34 | `COLUMN_ROW_HEIGHT` 但挂的是 **`min_h`** | 一致（注意是下界不是定高） |
| 13 | 列行内距 | [4,12,4,12] | `py_1()`=4 + `px_3()`=12 | 一致（初稿误写 6，已改） |
| 14 | 列行 gap | 8 | `gap_2()` | 一致 |
| 15 | 列名条宽 | 176 | `w(rems(11.))=176`（2026-11-06 由 10 改 11） | 一致 |
| 16 | 列名格内 gap | 8 | `gap_2()` | 一致（初稿误写 4，已改） |
| 17 | 列名字号 | 14 | `card_title` 的 `text_sm()` | 一致 |
| 18 | 类型徽标 | 高 18、pad 4、独立段 | `chip`：`px_1()`=4、`text_xs()`=12，**无显式高度**；已移出列名段（`meta_cell`） | 高度属推断（D4） |
| 19 | 生成器格宽 | 144 定宽 | `flex_none().max_w(rems(9.))` = **上限** | **差异 D2**（意图与实现不符） |
| 20 | 参数格 | 吃剩余 | `card_note` 自带 `flex_1().min_w_0()` | 一致 |
| 21 | 操作格宽 | 92 = 4×20 + 3×4 | `icon_button` = `Button::ghost().xsmall()`（`size_5()`=20）+ `gap_1()`=4 | 一致 |
| 22 | `CARD_OPS_WIDTH` | — | 7rem=112，**detail.rs 未引用** | **差异 D3**（常量脱节） |
| 23 | 筛选框宽 | 144 | `COLUMN_FILTER_WIDTH=9rem=144` | 一致 |
| 24 | 预览段内距 | 8 | `p_2()` | 一致（初稿误写 12，已改） |
| 25 | 预览表头 / 行高 | 26 | `PREVIEW_TABLE_SIZE=ComponentSize::XSmall`（26px） | 一致 |

### 需要拍的四个差异

- **D1 段2 双层内距**：代码是「`p_2` 外层 → `card()` 框 → `p_3` 内层」，稿里合并成 20。
  合并会让「卡片框贴哪」说不清；建议**稿补一层卡片框**（round 6 + 1px border），不要改代码。
- **D2 生成器格 `max_w` vs 定宽**：`ui.rs` 的注释写着「固定宽度是竖着扫能对齐的前提」，
  而 `detail.rs:1675` 用的是 `max_w` —— 内容短就收缩，四段基线其实**没对齐**。
  这是注释意图与实现相反，属真缺陷；建议改 `w(rems(9.))`（改代码，1 行）。
- **D3 `CARD_OPS_WIDTH=112` 未被引用**：实现是 `flex_none` 自然宽（实测 92）。
  `CARD_OPS_WIDTH` 的注释理由是「悬停显形时右侧不变宽」——但操作已改常驻，理由失效。
  两条路：删常量，或改实现为 `.w(rems(7.))`。建议删除并在 §4.1 登记「操作区自然宽」。
- **D4 chip 高度**：代码没有显式高度，稿里 18 是推断。要么 §4.1 明确「chip 高度由 padding+字号决定，不设常量」，要么加常量。

## 4.1 第二轮：实机反馈驱动的修订（2026-11-06）

第一版稿「看着合理、实机很差」的原因不是稿画错了比例，而是**稿把三个不同的东西画成了同一个格子**，
外加两处代码本身就没做到它注释里说的意图。下面每条都是「稿上成立、实机上不成立」的具体机制：

| # | 现象 | 根因（代码事实） | 改法 | 落点 |
| --- | --- | --- | --- | --- |
| R1 | 字段名显示区域小、长名一律截断 | 列名与「类型 / 置信度 / 引用」三枚徽标**挤在同一个 160px 定宽格**里，名字只剩约 90px | 徽标拆成独立段（`meta_cell`），列名条独占 176px；名字文本可占满整条 | `ui.rs` `COLUMN_NAME_CELL_WIDTH` 10→11；`detail.rs:1522` |
| R2 | 无列态 / 有列态布局变了一次 | 两态**元素集合不同**：无列只有「提示 + 场景 + 来源按钮」，有列多出「计数 + 筛选框」，加第一列后整排右移 | 抽成同一套工具栏（计数 → 场景 → 弹性 → 筛选 → 来源按钮），无列只把筛选降透明度，位置不动 | `detail.rs:1998`–`2050` |
| R3 | 筛选输入框看不出是可输入的 | `Input` 默认描边用 `theme.colors.input`，暗色主题下几乎与卡片底同色；高度也没跟 24px 工具栏对齐 | 自绘内凹容器（`background` 底 + `border` 描边 + 圆角 + 24px 高），`Input` 关掉自带边框（`appearance(false).bordered(false).xsmall()`） | `panel/mod.rs:675` |
| R4 | 加列不靠右 | 整排是 `h_flex` 左对齐，加列只是"这组按钮的最后一个"，不是"整行的最后一个" | 加弹性空档 `flex_1().min_w_0()`，来源按钮组整体推到右端，加列在最右 | `detail.rs:2010` |
| R5 | 字段名与参数摘要糊在一片 | 列名与参数摘要同底色、同字号档，行内没有第二层结构 | 列名条加 `bg(background)` + `border` + `rounded(3)` + `px_1()`，形成内凹输入条 | `detail.rs` `name_cell` |

R2 的代价写清楚：无列时筛选框**仍然占 144px**（只是 0.45 透明度）。换来的是「两种态位置恒定」，
若以后嫌空，可选的是把它换成同宽的静态提示（同样不跳），而不是让它消失。

### 2026-11-06 第二轮补（实机截图驱动）

| # | 现象（截图） | 根因 | 改法 |
| --- | --- | --- | --- |
| R6 | 操作行「场景模板」在左、「预设 / 导入结构」在右，批量起手被劈成两头 | 语义分组没做：同一种意图占两个位置 | 顺序改为 计数 → 场景模板 → **预设 → 导入结构** → 弹性空档 → 筛选 → ＋加列；左组=批量起手，右组=定位+新增 |
| R7 | 列名条左侧一条橙红色竖线常驻 | `current_bar`（`primary` 色 3px 左缘条）在新表首列上一直亮着，与列名条内凹底叠在一起像 bug | 撤掉调用（`current_bar` 函数 `#[allow(dead_code)]` 保留，以后真要「对话框关后回指刚改的列」再挂回） |
| R8 | 列区内容距卡片边太厚（外层 `p_2`=8 + 内层 `p_3`=12 = 20px） | 两层 padding 叠 | 外层 `p_2` 去掉（card 自己 `p_0` + 内容 `p_3`=12）；段 1 / 段 3 的 `p_2` 一并改 `p_3`，三段内距统一 12px |
| R9 | `CARD_OPS_WIDTH`（7rem=112px）全仓无引用，实测操作格自然宽 92px | 死常量 | 删除 |

## 5. 探针能断言什么（诚实边界）

gpui headless **能读到**实测 bounds（`TestWindowExt::find` → `ElementSnapshot`），
但**读不到没有 `.id()` + `.test_support()` 的元素**。按当前代码，可断言程度分三档：

| 档 | 断言对象 | 现状 | 成本 |
| --- | --- | --- | --- |
| 立即可用 | 三段高度 `ResizableState.sizes()` | 分栏 state 已有 id `mock-detail-split-{tab}` | 0（`window.use_state::<ResizableState>` 或由 panel 暴露） |
| 需补 id | 列行高 34 / 列行 padding / 列名格左缘 160 | 卡片有 `mock-field-{id}` 但无 `.test_support()` | 每个待断言块加 `.id()` + `.test_support()` |
| 读不到 | 内部 chip / 生成器格 / 操作格 | 无 id | 同上，或改断言行级 bounds（放弃格级） |

据此，**可以现在就钉死的 5 条几何不变量**（改代码后必须仍成立）：

1. 段1 高度 = `DETAIL_HEADER_HEIGHT`，且拖拽下界 < 初始高（防「分隔条拖不动」复发）。
2. 列行高度 = 34（`min_h` 生效后实测值）。
3. 列名格左边缘 = 列行左 + 12；列名格宽 = 160。
4. 生成器格右边缘 ≤ 列名格右 + 8 + 144。
5. 操作格 4 个按钮的相邻间距 = 4，且最右按钮右缘距列行右缘 = 12。

**断言不了的是审美**：稿能固定「行高 / 内距 / 间距 / 固定列宽 / 字号」，判不了「这样好不好看」。
审美判据必须一次说清、写进本文件，否则下一轮还是肉眼调。

## 6. 使用约定

1. 改布局前：先改 `.figcraft` 稿 → 重跑 §2 → 把新数值同步进 §3/§4 → 再改代码。
2. 代码偏离本表 → 先在 §4 记一行「为什么偏离」，再改；不要静默改常数。
3. 一轮只动一个维度（行高 / 列宽 / 间距 / 显隐），否则测不出是哪一项引起的副作用。
4. FigCraft 是 egui + 自有 auto layout，本项目是 gpui flex：`Hug/Fill/Fixed` 与 `basis/grow/shrink`
   不是一一映射，**只能对照常量级口径，不追逐像素一致**；稿是「意图」，不是「渲染结果」。
