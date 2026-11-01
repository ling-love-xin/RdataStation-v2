# Mock 视图产品化（2026-11-01）

需求：「这个配色和布局太像是一个草稿了，不像一个真正的产品」——范围是整个 `crates/mock`
视图模块，不止列卡片。

## 一、诊断：草稿感来自"缺层级"，不是配色难看

所有东西摊在同一个平面上：

| # | 根因 | 具体表现 |
| --- | --- | --- |
| 1 | 卡片没有表面色 | 卡片是透明的，与面板同为 `background`（亮 = white），只靠一根 `border` 划界。白底 + 灰线框正是线框图（wireframe）的定义 |
| 2 | 内距 = 间距 | 卡片内 8px、卡片之间也 8px，眼睛分不出是一张卡还是两张 |
| 3 | chip 全是 outline | 13 种数据类型按数据族分色是对的，但一律 `Tag::outline()` 之后在卡片里几乎看不见——色相分族被稀释没了 |
| 4 | 每卡常驻 4 个灰按钮 | N 张卡 = 4N 个按钮铺满面板，把所有功能摊在表面上是草稿与产品的分界 |

另外两处：字色只有两档（卡片里除列名外全是同一种灰）；中央 tab 三段与右 Dock 内容区全部贴边。

## 二、视觉层级规范（落在 `crates/mock/src/mock_view/style.rs`）

| 层 | 底色 | 描边 |
| --- | --- | --- |
| 面板（最外） | `background` | 无 |
| **卡片** | `muted`（比面板抬一档） | `border` → 悬停 / 聚焦转 `ring` |
| 卡内分隔 | — | `border` 打 0.6 折（不抢卡片边界的视觉重量） |

| 维度 | 规范 |
| --- | --- |
| 间距 | 内距 12px / 卡片间 8px——**落差本身就是组的边界** |
| chip | tint：同色淡底 14% + 同色字 + 同色描边 28% |
| 字色 | 三档：主文字 `foreground` ｜ 弱化正文 `muted_foreground` ｜ 标签 / 提示 `muted_foreground.opacity(0.78)` |
| 段标题 | `foreground` + MEDIUM + 下方细线（此前是 `muted_foreground`，与内容同色就不是标题了） |
| 操作 | 次要操作悬停 / 聚焦才显形，隐藏时用固定宽度占位容器 |

### 为什么 hover 只动描边

默认主题（`vendor/gpui-component/src/theme/default-theme.json`）的中性色只有 100 / 200 两档：

| token | 亮 | 暗 |
| --- | --- | --- |
| `background` | white | neutral-950 |
| `border` | neutral-200 | neutral-800 |
| **`muted`** | **neutral-100** | **neutral-800** |
| `list_hover` | #f5f5f5 | — |
| `ring` | neutral-400 | neutral-500 |

卡片底在 100 那档，hover 该去的 150 档**不存在**：`list_hover` 在亮主题下与 `muted` 同为
#f5f5f5，换来等于没换；`table`（亮 = white）更是往反方向走。而 `ring` 在明暗两种主题下都比
`border` 明显一档，描边变深本身就是清晰反馈。

## 三、改动

| 位置 | 改动 |
| --- | --- |
| `style.rs` | 新增 `surface` / `card_divider` / `chip` / `text_hint`；`card` 加表面色 + 内距提到 12；`card_interactive` 改走 `ring`；`section` 字色提到主色 |
| 中央 tab | 列卡片与结果列卡片：表面 + 卡内分隔；顶部标签、说明文字归到弱化档；三段各加内距 |
| 右 Dock | 内容区内距；场景清单行、历史卡片、模板卡片的操作悬停 / 聚焦显形 + 占位容器；历史 / 模板行改走卡片原语 |
| 列编辑对话框 | 顶部行与主体两栏之间加卡内线；两栏内距；标签弱化 |

**操作显形的实现**：

- 显形条件 = 鼠标指着它 **或** 键盘焦点在它身上（后者是给键盘留的路，鼠标之外也要够得到）；
- 焦点句柄用 `window.use_keyed_state(元素 id, cx, |_, cx| cx.focus_handle())`，按元素 id 缓存，
  不必在 view 里维护 `HashMap`；
- 焦点状态**不存**——`FocusHandle::is_focused(window)` 当场查；hover 状态必须存（gpui 不提供
  "查询 hover"），且只在值变化时才 `cx.notify()`；
- 隐藏时用固定宽度占位容器（`CARD_OPS_WIDTH` 112px / `ROW_OPS_WIDTH` 80px），
  否则列名区与行数格会跟着按钮的有无左右跳。

## 四、验证

- `cargo test -p rds-mock --lib`：**234 项全绿**
- `cargo test -p rds-workbench --test mock_generator --test mock_jobs --test mock_job_cancel`：**24 项全绿**
- `cargo check -p rds-workbench` 通过；`cargo fmt --check -p rds-mock` CLEAN

## 五、本轮没做（P1 / P2）

| 项 | 说明 |
| --- | --- |
| 卡片"当前"态 | 正在编辑的那一列没有强调色左缘，列表里看不出焦点在哪 |
| 右 Dock 清单行卡片化 | 24px 紧凑行，卡片化会损失一行能放的信息，且下缘进度线要连着重新设计 |
| hover 过渡动画 | gpui 的 `.hover()` 是即时的；要插值得先把 hover 做成可读状态——现在这个状态已经有了，可以接 |
| 卡片密度分档 | 现在只有唯一档（紧凑 / 舒适未分） |
| 自定义生成器对话框 | 560px 表单，未逐项接入原语 |
