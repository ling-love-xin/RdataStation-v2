# Quick Open 目录结构重构方案

> 本文只做**结构设计**，不含行为改动；行为缺陷与文档差异见
> `quick-open-implementation-audit.md`。基线 `main` @ `6868db72`。

## 0. 结论

1. **现在不抽 crate**。`rds-architecture` 的四条判据一条都不满足，原型设计 §16 的三个升级触发条件也都没发生；
   抽出去只会换来一个新的编译单元和一次跨 crate 的 DTO 变更，收益为零。
2. **真正需要动的是 `workbench` 侧的三个文件**：`palette.rs`（738 行、5 类职责）、`delegate.rs`（435 行、3 类职责）、
   `tests.rs`（538 行、13 个窗口用例），以及**散在 `view.rs`（2821 行）里的约 300 行 Quick Open 装配与副作用**。
3. 目标形态：`quick_open/` 下三个子目录（`palette/`、`delegate/`、`tests/`）+ 一个 `host.rs`，
   生产代码单文件 **≤ 260 行**，测试文件 **≤ 350 行**。
4. `editor` 侧的 `model/` **不需要再拆**（`types` 144 / `scoring` 88 / `sources` 109 / `groups` 205 已达标），
   只有 `model/tests.rs`（523）偏大，建议按主题拆三份。
5. 搬迁顺序设计成 **S0–S5，每一步都能单独编译、单独跑测、单独 commit**；不做一次性大挪移。

---

## 1. 现状量化

| 文件 | 行数 | 承担职责 | 问题 |
| --- | --- | --- | --- |
| `workbench/src/quick_open/palette.rs` | 738 | 实体生命周期 · 搜索调度 · 文件清单 · 选中与镜像 · 整块渲染 | **5 类职责压在一个 impl 里**，任何一处改动都要读完整文件 |
| `workbench/src/quick_open/delegate.rs` | 435 | `ListDelegate` 协议 · 行内容渲染 · 组头 / 组尾 / 空态 | 行渲染与协议耦合，改行样式要翻协议代码 |
| `workbench/src/quick_open/tests.rs` | 538 | 13 个窗口用例 + 夹具 | 夹具与用例同文件，新增用例只能追加 |
| `workbench/src/quick_open/mod.rs` | 20 | 模块地图 + 重导 | 正常 |
| `workbench/src/view.rs` | 2821 | 整个工作台装配 | Quick Open 的 `ensure/render/execute/host` 约 300 行混在其中（4 处不相邻） |
| `editor/src/quick_open/model/types.rs` | 144 | 模式解析 + 行 / 动作类型 | 正常 |
| `editor/src/quick_open/model/groups.rs` | 205 | 分组 + 上限 + Goto 排序 | 正常（接近上限，保持） |
| `editor/src/quick_open/model/sources.rs` | 109 | 三源 → 行映射 | 正常；**这是外部 DTO 的唯一接触面**（见 §5） |
| `editor/src/quick_open/model/scoring.rs` | 88 | 评分与命中区间 | 正常 |
| `editor/src/quick_open/model/tests.rs` | 523 | 16 个纯函数用例 | 偏大，建议拆 |
| `editor/src/quick_open/commands.rs` | 143 | 13 条命令目录 | 正常 |

`view.rs` 里 Quick Open 的四段：

| 位置 | 内容 | 行数 |
| --- | --- | --- |
| 标题栏入口（`view.rs:1889-1927`） | 入口控件 + `.occlude()` 注释 | ~40 |
| `toggle_quick_open` / `ensure_quick_open_palette` / `render_quick_open`（`:2356-2394`） | 开关与挂载 | ~40 |
| `execute_quick_open_action`（`:2429-2541`） | 10 个动作的副作用落地 | ~113 |
| `WorkbenchQuickOpenHost`（`:2982-2996`） | 宿主端口实现 | ~15 |

---

## 2. 目标结构

```
crates/workbench/src/quick_open/
├── mod.rs                 # 模块地图 + 重导（≤60）
├── host.rs                # 宿主装配与副作用（从 view.rs 搬出，≤280）
├── palette/
│   ├── mod.rs             # 实体字段 + new / on_opened / close / confirm（≤200）
│   ├── query.rs           # 输入 → Query/Mode；占位 / 档位 chip / 提示行文案（≤170）
│   ├── search.rs          # 防抖 / 去重键 / seq / 回填泵 / enqueue（≤250）
│   ├── files.rs           # 草稿箱清单：请求、可见文件、来源说明（≤140）
│   ├── selection.rs       # 业务键选中 / 镜像 / 移动（≤160）
│   └── render.rs          # overlay + 输入 + List 挂载 + 底栏（≤230）
├── delegate/
│   ├── mod.rs             # ListDelegate 协议 + 快照推送 + 选中回写（≤200）
│   ├── row.rs             # 行内容：标题 / snippet / why / 高亮（≤230）
│   └── chrome.rs          # 组头 / 组尾「还有 N 条」/ 空态（≤120）
└── tests/
    ├── mod.rs             # 夹具：temp_project、seed 连接、开面板
    ├── palette.rs         # 打字 / ↑↓ / ↵ / Esc / 遮罩 / ≥2 字门槛
    ├── goto.rs            # Goto 档三例（锁元数据 / ↵ 定位 / ⌥↵ 属性）
    └── host.rs            # 动作落地 + 打开即载草稿箱清单
```

设计依据（不是口味）：

- **一个文件一个职责**，对应 `gpui-kit-dev` 的「render 是纯读路径 / 副作用回事件路径」分界：
  `search.rs` 与 `files.rs` 承担事件与后台调度，`render.rs` 只读。
- `palette/` 用**同一类型跨文件拆 `impl` 块**（Rust 原生支持），字段声明只留在 `palette/mod.rs`，
  这样不必为了拆文件把状态搬进多个结构体（避免引入新的跨结构体同步问题）。
- `delegate/` 按「协议 / 行 / 外壳」三层切，`row.rs` 是唯一会随命中高亮与两行式布局增长的文件。
- `tests/` 按**被测档位**切，而不是按“新用例追加位置”切——新增 Goto 用例时只碰 `goto.rs`。

---

## 3. 搬迁映射（旧 → 新）

| 现在的位置 | 去向 |
| --- | --- |
| `palette.rs` 字段 + `new` / `on_opened` / `close` / `confirm` / `confirm_with` / `finish_confirm` / `confirm_locate` / `consume_pending_open` | `palette/mod.rs` |
| `palette.rs::ensure_entities`（输入 / List / 委托装配）、提示行与档位 chip | `palette/render.rs` + `palette/query.rs`（chip 与占位文案） |
| `palette.rs::refresh`（Query 解析 → 分组 → 推送） | `palette/query.rs` |
| `palette.rs::schedule_search` / `ensure_pump` / `pump_results` | `palette/search.rs` |
| `palette.rs::schedule_file_list` / `visible_files` / `files_note` | `palette/files.rs` |
| `palette.rs::set_selection` / `move_selection` / `mirror_selection` | `palette/selection.rs` |
| `palette.rs::render` | `palette/render.rs` |
| `delegate.rs` 的 `ListDelegate` 方法 + `syncing_from_host` + 选中回写 | `delegate/mod.rs` |
| `delegate.rs::row_element` / `title_element` / `snippet_element` + 高亮切分调用 | `delegate/row.rs` |
| `delegate.rs::render_section_header` / `render_section_footer` / `render_empty` | `delegate/chrome.rs` |
| `view.rs::ensure_quick_open_palette` / `render_quick_open` / `execute_quick_open_action` / `WorkbenchQuickOpenHost` | `quick_open/host.rs`（`view.rs` 只留 `toggle_quick_open` 与调用点） |
| `tests.rs` 13 个用例 + `temp_project` 夹具 | `tests/{mod,palette,goto,host}.rs` |
| `editor/model/tests.rs` | `model/tests/{mod,query,rank,groups_goto}.rs`（可选，独立一步） |

---

## 4. 为什么现在不抽 crate

按 `rds-architecture` 的 crate 归属判据逐条核：

| 判据 | Quick Open 现状 | 结论 |
| --- | --- | --- |
| 独立状态与生命周期 | 状态是 `Shared::quick_open`（窗口级 `Cell<bool>`），随工作台生命周期 | ✗ |
| 稳定公开边界 | 只有一个消费方（workbench）；`editor` 侧只靠 `pub use` 重导 | ✗ |
| 使用方 ≥2 | 1 | ✗ |
| 原型 §16 的三个升级触发 | ① 第二宿主 ✗ ② 插件 M9 注册命令 ✗ ③ 最近使用需持久化 ✗ | ✗ |

再加一条成本事实：`model/sources.rs` 现在直接消费 `editor` 侧的 `MetaEntry` / `FlatFile` / `ConnectionRow`。
抽 crate 时这些 DTO 要么一起搬（跨 crate 结构调整），要么加一层适配（多一层间接）。
在只有一个宿主的前提下，这两条路都比“目录内拆分”贵，且不解决任何实际问题。

**所以本次重构的边界是“目录内拆分”，不新增 crate。**

---

## 5. 将来要抽时的形状与触发条件（预埋接缝）

如果哪天真的要抽，形状已经定好，抽的过程应当是机械的：

```
crates/quick_open/               # crate 名 rds-quick-open
├── lib.rs                       # pub use model::*; pub use commands::*;
├── model/{types,scoring,sources,groups}.rs
├── commands.rs
└── sources.rs                   # 唯一接触外部 DTO 的适配层（迁移前先把它独立成 trait）
依赖方向：quick_open → shared（DTO）；不含 gpui；不依赖 editor、workbench
```

接缝就是 §1 里那句：**`sources.rs` 是外部 DTO 的唯一接触面**。只要这条纪律不破，
抽取时只需替换 `sources.rs` 的入参类型，`groups` / `scoring` / `types` 一行都不用改。

触发条件（任一满足才动手，否则维持现状）：

1. 出现**第二个宿主**（例如插件面板里也要一个 Quick Open）；
2. 插件 M9 需要往命令目录**注册命令**（目录要有稳定公开边界）；
3. 「最近使用」需要**跨会话持久化**（具备独立状态）；
4. 结构预算被击穿：`quick_open/` 目录生产代码 > 1800 行，或某个文件 > 400 行且**已无法再按职责切分**。

---

## 6. 迁移步骤（每步可编译 / 可测 / 可单独 commit）

| 步 | 动作 | 影响面 | 验收 |
| --- | --- | --- | --- |
| **S0** | `git mv palette.rs palette/mod.rs`；`git mv delegate.rs delegate/mod.rs` | 仅路径；`mod` 声明不用改 | `cargo test -p rds-workbench --lib quick_open` 绿 |
| **S1** | 从 `palette/mod.rs` 依次抽 `search.rs` → `files.rs` → `selection.rs` → `query.rs` → `render.rs`（一次一个文件，抽完即测） | `impl QuickOpenPalette` 跨文件；字段不动 | 每抽一个跑一次 lib 测试 |
| **S2** | `delegate/mod.rs` 抽 `row.rs`、`chrome.rs` | 同上 | 同上 |
| **S3** | `view.rs` 的 4 段搬进 `host.rs`；`view.rs` 只留 `toggle_quick_open` 与三处调用 | 副作用链**不变**，仅换文件 | lib 测试 + `ui_contract` 绿 |
| **S4** | `tests.rs` → `tests/{mod,palette,goto,host}.rs` | `#[cfg(test)] mod tests;` 变成目录 | 13 个用例计数不变 |
| **S5** | 契约登记 + 文档映射表回写（见 §7） | `ui_contract.rs`、`prototype-design.md` §19 | `ui_contract` 绿 + 映射表可逐条打开 |

每一步的验收命令：

```powershell
cargo test -p rds-workbench --lib quick_open
cargo test -p rds-workbench --test ui_contract
cargo check -p rds-workbench
```

---

## 7. 风险与规避（都已踩过，别再来一遍）

1. **`include_str!` 契约会漏扫，而不是报错。**
   `crates/workbench/tests/ui_contract.rs:253-260`（裸尺寸扫描）与 `:754-761`（裸色值扫描）
   都以**单个文件**为单位登记 `quick_open/delegate.rs` / `quick_open/palette.rs`。
   拆成目录后这两条路径失效，若只改登记而漏掉新文件，扫描会**静默放过**新文件里的裸 `px(N.)`。
   规避：登记时按目录列出**每一份**文件；并在 S5 做一次**反向验证**——
   故意在 `palette/render.rs` 里塞一个 `px(7.)`，确认契约变红，再删掉。
2. **测试模块的 `use super::*` 会把 `#[test]` 宏带进作用域**，与 `#[gpui_kit::test]` 冲突（`gpui-kit-dev` 明写）。
   拆 `tests/` 时每个文件显式列依赖，不要图省事。
3. **可见性**：`palette`/`delegate` 现在是 `pub(crate)`，拆目录后 `mod.rs` 里保持 `pub(crate) mod palette;`、
   子文件内部用 `pub(super)` / `pub(crate)`，对外路径 `crate::quick_open::palette::X` 不变。
4. **行号会全部漂移**：文档与注释里现在大量 `path:line`。S5 起把映射表改成
   **「符号 → 文件」**（例如 `execute_quick_open_action → quick_open/host.rs`），不再写行号；
   注释里的行号引用要么删，要么换成符号名。
5. **一次只抽一个文件**。S1 若一口气拆五个，编译错误会互相掩盖（缺 import、私有字段、`&mut self` 借用边界）。

---

## 8. 可选：给结构预算加一条门禁（建议，不默认上）

如果希望“以后不会又长回 700 行”，可以在 `ui_contract.rs` 加一条轻量断言：
`quick_open/` 下每个 `.rs` ≤ 300 行、`tests/` 下每个 ≤ 350 行。
代价是新增文件要提前想清楚职责，收益是**这条纪律不依赖人记性**。
建议在 S4 完成后加，避免重构期间被自己的门禁卡住。

---

## 9. 与本次批次 ① 的关系

批次 ①（注释债、入口宽度接常量、组头文案、空态换 `Empty`）已经落在这批文件里，
它与本次重构**互不阻塞**：先提交 ①，再做 S0–S5，`git log` 上两批目的清晰可分。
若顺序反过来（先搬目录再改文案），① 的 diff 会淹没在移动重命名里，review 成本变高。
