# 草稿箱 · 缺口清单与设计判断（2026-11-28）

> **本文定位**：`crates/scratchpad` 的**未完成项台账** + **本轮修复记录** + **设计取舍判断**。
> 与 [`scratchpad-architecture.md`](./scratchpad-architecture.md) §13 互指：§13 记「问题是什么、怎么修」，
> 本文记「还差多少、修没修、值不值得换设计」。
> **口径**：文中所有规模数字均为 2026-11-28 本机实测（命令见 §5），不引用旧文档的过时口径。

## 1. 完成度结论

一句话：**面板自身约 90%，跨模块闭环约 60%**；作为「可日常使用的草稿箱」已经成立，作为
「VSCode 级文件树」还差 3 项（跨根展开、多文件 Tab 边界、替换预览）。

| 维度 | 完成度 | 判据（均可在代码里指到） |
| --- | --- | --- |
| 域逻辑与存储 | 95% | 路径防护（`resolve_path` 拒绝 `..` / 隐藏目录 / 越界）、旧布局迁移、递归复制、回收站跨模块还原拒绝、引用重定位、`diff_with_content`、标签关联表，均有实现 + 测试 |
| 编辑器联动（Phase C） | 100%（模块侧） | 打开 / 脏点 / 冲突 Diff / 拖入插入 / 执行回执回填连接，全部经 `ScratchpadHost` 端口；本模块不认识编辑器类型 |
| 面板 UI | 90% | 工具栏 / 空态 / 搜索面板 / 引用 / 回收站 / 撤销栏 / 状态行齐备；组件标准化：裸 hex **0**、`rounded_full` **0**、`cursor_pointer` 仅 **6** 处（折叠头 + 树行的允许例外） |
| 剪贴板 | **100%（本轮补齐）** | 剪切 / 复制 / 粘贴三件套闭环，粘贴有 `ctrl-v` 与右键「粘贴」两个入口 |
| 导入 | **100%（本轮补形态）** | 应用内 Dialog：路径输入 +「加入」+「浏览…」回填 + 清单 + 去重；与「添加外部引用」同形态 |
| 自动化测试 | 85% | **56 项** lib 测试全绿；缺「粘贴真落盘」与「导入弹窗交互」的窗口级用例（§4 W2） |
| 文档 | 70% → **85%（本轮补记）** | 六件套（README / architecture / prototype-design / prototype.html / showcase / user-guide）齐；进度记录原停在「三十一次」且缺 B1–B4，本轮补记 + 新增本文 + §13.5 |

## 2. 缺口清单

| # | 级别 | 问题 | 证据（本轮实测） | 状态 |
| --- | --- | --- | --- | --- |
| P1 | 高（断链） | 剪贴板只有「剪切 / 复制」，**没有任何粘贴入口**：右键粘不进去、`ctrl-v` 不响应 → 功能只做了一半 | `paste_scratchpad_clipboard` 曾带 `#[allow(dead_code)]` 且零调用方；`commands.rs` 无粘贴动作；`app/main.rs` 无 `ctrl-v` 绑定 | **✅ 已修（本轮）** |
| P2-1 | 中 | 导入走系统原生 `prompt_for_paths`，引用走应用内 `Dialog` → 两个弹窗观感不成一套（用户「弹出窗口没实现」体感的来源） | `actions.rs::pick_scratchpad_imports` 用 `cx.prompt_for_paths`；`chrome.rs::render_ref_dialog` 用 `Dialog` | **✅ 已修（本轮，用户拍板改应用内）** |
| P2-2 | 中 | **批量替换没有预览**：`replace_*` 直接原子写盘，用户在「搜索命中」和「写回」之间没有确认环节 | 全仓 `preview_diff` / `diff_preview` / `替换预览` 零命中；`jobs::ReplaceAll` 是「匹配 → 写回 → 重搜」单程 | ❌ 未做（§4 W1） |
| P3-1 | 待拍板 | 外部引用目录**不能在树内展开**，只作「打开位置」入口 | `external_refs` 只渲染一行；跨根路径树的读写策略未定义 | ❌ 待拍板（架构 §13.3 K6） |
| P3-2 | 待拍板 | 多文件 Tab 与草稿箱的边界：Tab 体系属编辑器侧，脏点/冲突已接但「草稿是否该有自己的 Tab」未定 | `open_in_editor` 只保证「同路径激活」 | ❌ 待拍板（K7） |
| P3-3 | 低 | 冲突改真模态（现为「冲突条 + 中央 Diff 面板」，内容等价、交互不是模态） | `render_scratchpad_diff_pane` | ❌ 待拍板（K8 相邻） |
| P4-1 | 工程债 | 三个巨型文件：`chrome.rs` **2668 行** / `store.rs` **2525 行** / `actions.rs` **2095 行**；渲染分支与状态变更混在同一个 `impl` 里 | §5 实测行数 | ❌ 未做（§7 建议 4） |
| P4-2 | 工程债 | 工具栏「显示不全」缺复现口径（是宽度预算不足，还是按钮组换行被裁） | 无最小复现步骤 | ❌ 待补证据 |
| P4-3 | 文档滞后 | `scratchpad-dev-plan.md` 未记 B1–B4；测试基线两处不同步：`scratchpad-dev-plan.md` 写「37 项」、`docs/.../README.md` §5 写「52 项」（实测 56） | §5 实测 | **✅ 本轮同步** |

## 3. 本轮修复明细（P1 + P2-1）

### 3.1 粘贴入口（P1）

| 改动 | 文件 | 说明 |
| --- | --- | --- |
| 新增动作 `ScratchpadPaste` | `scratchpad/src/commands.rs` | 与 `ScratchpadNewFile` 同组；`workbench/src/commands.rs` 重导 |
| 绑定 `ctrl-v`（context `scratchpad`） | `app/src/main.rs` | 只在本面板内生效，不抢编辑器/输入框的粘贴 |
| `.on_action(ScratchpadPaste)` | `scratchpad_view/chrome.rs` | 面板根元素上接住动作 |
| 右键「粘贴」 | `scratchpad_view/rows.rs` | **剪贴板为空时不出现**（不放「点了没反应」的入口）；落点 = 本行 |
| 落点语义 | `scratchpad_view.rs::scratchpad_paste_target_of` | 文件夹 → 自身；文件 → **父目录**（VSCode 同级语义）；根级文件 → 模块根 |
| 落点取数 | `actions.rs::scratchpad_paste_target` | 唯一选中项 → 绝对路径 → `relative_path_of` → 相对路径（`jobs::Paste.target` 口径） |
| 源路径换算 | `actions.rs::paste_scratchpad_clipboard` | 剪贴板存绝对路径，入队前统一转相对（不依赖 `Path::join` 遇绝对路径替换前缀的隐式行为）；一个都转不出时提示「已不在模块内」 |
| 剪切的一次性 | `actions.rs::paste_scratchpad_clipboard` | 剪切搬运成功后清空剪贴板，避免二次粘贴引用已不存在的源 |
| 编辑态早退 | 同上 | `edit.is_some()` 直接 return：内联重命名的 `ctrl-v` 归输入框文本粘贴 |
| 移除 `#[allow(dead_code)]` | 方法 + `primitives.rs::ScratchpadClipboard` | 字段与模式从此都是读写字段 |

**未拍板项（如实标注）**：用户只拍了「导入改应用内」，没有明确「选中文件时粘到父目录」。
本次按 VSCode 同级语义实施；若不认可，只改 `scratchpad_paste_target_of` 的文件分支
（返回 `String::new()` 即退回「粘到模块根」），其余不动。

### 3.2 导入改应用内 Dialog（P2-1）

| 改动 | 文件 | 说明 |
| --- | --- | --- |
| 状态 | `scratchpad_view.rs` | `import_dialog_open` / `import_paths: Vec<String>` / `import_path_input` |
| 输入框懒创建 | `actions.rs::ensure_scratchpad_inputs` | 占位「文件或目录路径（多个用换行 / 分号分隔）」 |
| 打开 | `actions.rs::pick_scratchpad_imports` | **方法名不变**（工具栏「⬇」与空态「导入」两个调用点零改动），内部改为清空 + 开弹窗 + 聚焦 |
| 浏览 | `actions.rs::browse_scratchpad_imports` | 系统选择器**只回填清单**，不再直接导入 |
| 加入 / 移除 | `add_scratchpad_import_paths` / `remove_scratchpad_import_path` | 换行与分号分隔；去重（大小写不敏感）；移除按路径文本（不用下标，避免删前一项后漂移） |
| 确认 | `confirm_scratchpad_imports` | 合并输入 → 过滤不存在的路径（提示跳过条数）→ 入队后台复制 → 关窗；一个有效路径都没有时提示并保持打开 |
| 渲染 | `chrome.rs::render_import_dialog` | `Dialog` + `Input` + 两个 `Button` + 清单行（路径 + ✕）；挂载点在 `ref_dialog_el` 之前 |
| 文案 | 同文件 | 空清单给一句指路：粘贴路径后点「加入」，或点「浏览…」 |

## 4. 未完成项（不在本轮范围）

| # | 事项 | 为什么现在不做 | 建议 |
| --- | --- | --- | --- |
| W1 | 批量替换的 Diff 预览（P2-2） | 要改 job 契约：先「匹配 + 算 diff、零写盘」，用户确认后再「校验版本戳 + 写盘」。不是 UI 加个提示就能糊过去的 | 见 §7 建议 3；排到下一个迭代 |
| W2 | 粘贴真落盘 / 导入弹窗交互的窗口级测试 | 需要临时目录 + 宿主夹具（现有 56 项是纯函数 + 轻量视图） | 与 W1 同批做，或先补「粘贴到文件夹后文件出现」一条 |
| W3 | 引用目录跨根展开（P3-1） | 产品决定未定（只读引用要不要可展开） | 等拍板 |
| W4 | 工具栏「显示不全」（P4-2） | 缺复现口径（窗口宽度 / 缩放 / 哪些按钮被裁） | 需要一张截图或明确步骤 |
| W5 | 巨型文件拆分（P4-1） | 属于结构性重构，与本轮功能收口不应混在一次提交 | 见 §7 建议 4 |
| W6 | **复制固定加 `_copy` 后缀，跨目录也加**：`store::copy_entry` 第一次尝试就用 `{stem}_copy{ext}`，所以「复制到另一个文件夹」得到的是 `notes_copy` 而不是 `notes`（VSCode / 资源管理器按「同目录才改名」） | 属 store 既有行为，改它会影响既有测试与粘贴语义，本轮不动 | 下一迭代：仅在同名冲突时加后缀（`i == 0` 先用原名，失败再 `_copy`），并补「跨目录保留原名 / 同目录自动改名」两条用例 |
| W7 | **剪贴板状态锁在草稿箱模块内**（`ScratchpadClipboard` 是 `ScratchpadView` 的字段），跨模块搬运（归档 → 草稿、项目文件 → 草稿）没有通道，只能绕系统文件管理器 | 要动 workbench 层的跨模块接口，属于架构收敛而非本轮功能修复 | 见 §7 建议 6 |

## 5. 实测与验证（2026-11-28）

命令（仓库根）：

```powershell
cargo test  -p rds-scratchpad -j 2      # 56 passed; 0 failed
cargo build -p rds-app -j 2             # Finished（app 层动作导入 + ctrl-v 绑定编译通过）
```

规模（`Get-ChildItem -Recurse -Filter *.rs` + `Measure-Object -Line`，实测非估算）：

| 文件 | 行数 |
| --- | ---: |
| `scratchpad_view/chrome.rs` | 2668 |
| `store.rs` | 2525 |
| `scratchpad_view/actions.rs` | 2095 |
| `jobs.rs` | 1184 |
| `scratchpad_view/rows.rs` | 1022 |
| `scratchpad_view/tests.rs` | 1008 |
| `scratchpad_view/primitives.rs` | 827 |
| `scratchpad_view.rs` | 589 |
| 其余 7 个文件 | ≤267 |
| **合计（15 个文件）** | **12769** |

规范残留（同批实测）：裸 hex **0** 处；`rounded_full` **0** 处；`cursor_pointer` **6** 处
（折叠头 2 + 树行 4，属允许例外）；裸 `px(数字)` **1** 处且只在测试夹具（`tests.rs` 的窗口尺寸）。

**未做的验证（如实说明）**：本轮没有真机点按验收。原因：用户已明令禁止使用虚拟桌面截图通道；
自绘透明标题栏窗口只能用 PrintWindow 或用户截图佐证。因此 §8 给出手动验收步骤，需要一次真点击确认。

## 6. 文档同步动作

- 本文（新增）：缺口台账 + 设计判断。
- `scratchpad-architecture.md` §13.5（新增小节）：指向本文，并登记本轮两条修复（K13 / K14）。
- `docs/architecture/scratchpad/README.md` §5 测试数 52 → 56；§6 文档地图加本文一行；§7 下一步加本轮两项。
- `crates/scratchpad/README.md` 能力表：导入补「应用内弹窗」、面板侧补粘贴落点与剪切一次性语义。
- `scratchpad-dev-plan.md`：补记 B1–B4（提交 `ea270f37`）与本轮（三十二次）；测试基线 37 → 56。

## 7. 更优设计方案（如果由我重做）

按「收益 / 代价」排序，不是许愿清单。

### 建议 1：把「粘贴落点」从选择集里解耦出来（收益高，代价小）

现状用 `selected`（多选集合）推断落点：多选时只能落到模块根，且「右键某行 → 粘贴」还得
先把选择集改成那一行（副作用式改写 UI 状态）。更干净的做法是引入**锚点**：

- `paste_anchor: Option<String>`：右键点的行、或最后一次单击的行；
- `ctrl-v` 的落点 = 锚点 → 若无锚点 → 唯一选中项 → 否则模块根；
- 粘贴**不改** `selected`，选择集只表达「选了什么」，不兼职「粘到哪」。

收益：多选场景可支持「选中 3 个 → 右键某文件夹 → 粘进去」；`selected` 不再被事件路径偷偷改写。
代价：多一个状态字段与两处取数逻辑（约 30 行）。

### 建议 2：导入的主通道应该是「拖入」，弹窗只服务「路径字符串」（收益中，代价中）

现在弹窗里还留一个「浏览…」调系统框 —— 形态统一了一半，因为系统框本身不可能被应用内替代
（多选、网络位置、最近访问都在 OS 侧）。更合理的分工：

- **拖入面板**（`ExternalPaths` 已有）= 推荐主通道，文案直接引导；
- **应用内 Dialog** = 只处理「我手上有一串路径」（从日志、文档、聊天里复制来的），
  这正是应用内弹窗不可替代的价值；
- 「浏览…」保留但降为次要按钮（`ghost`），不与「加入」抢视觉。

收益：两条通道各自回答一个真问题，不再互相扮演对方。
代价：文案与按钮层级调整；不需要新代码。

### 建议 3：替换预览做成两阶段任务，而不是 UI 提示（收益高，代价中高）

`ReplaceAll` 现在是「匹配 → 写回 → 重搜」单程，中间没有用户确认点。正确形态是：

1. `PlanReplace { query, replacement, include, exclude }` → 返回
   `{ plan_id, hits, 每文件 diff 摘要, 涉及文件数 }`，**零写盘**；
2. 中央 Diff 面板展示计划（复用现有 `DiffLineKind` 渲染）；
3. `ApplyReplace { plan_id, 文件 mtime+size 指纹 }` → 写盘前逐个校验指纹，
   不一致就整批拒绝并提示「文件已被外部修改，请重新生成预览」。

收益：用户看到的是**真实将发生的变化**，而不是一句「确定替换 12 处吗？」；指纹校验顺带堵住
TOCTOU（预览期间文件被外部改动）。
代价：`jobs` 增两类任务、计划态需要生命周期管理（计划过期/项目切换要失效），约 200–300 行。

### 建议 4：巨型文件的根因是「渲染与状态混在一个 impl」，拆文件解决不了（收益中，代价中）

`chrome.rs` 2668 行里绝大部分是 `render_*` 分支（工具栏 / 搜索 / 引用 / 回收站 / 撤销 / 状态行），
它们只读状态、只吐元素，却和「状态变更方法」共用一个 `impl ScratchpadView`。更优的切法是
**按职责切类型**，而不是按行数切文件：

- `ScratchpadState`（`RefCell` 里的纯数据，已有）；
- `ScratchpadChrome<'a>`：纯渲染结构，入参 `&ScratchpadState + &dyn Host + 回调束`，无 `&mut self`；
- `ScratchpadView`：只留状态变更 + 动作派发（`actions.rs` 那半边）。

收益：渲染函数无法偷偷改状态（编译期保证「render 纯读」），巨型 `impl` 自然消失。
代价：回调束的样板代码；一次性迁移风险（需要逐块搬迁 + 渲染快照测试兜底）。

### 建议 5：把「测试数 / 行数」从文档里挪到可生成的表（收益低，代价低）

现在两处文档写「37 项」/「52 项」而实测 56 —— 不是谁写错了，而是**手写数字必然滞后**。
可以在 `tools/` 放一个 10 行的脚本（`cargo test -- --list` + 行数统计）生成一张表，
文档只引用「见 `docs/.../metrics.md`（由脚本生成）」。单人项目不必上 CI，手动跑一次即可。

### 建议 6：剪贴板不该是模块字段，而应是应用级「文件操作总线」（收益高，代价高）

现状：`ScratchpadClipboard { paths: Vec<PathBuf>, cut: bool }` 是 `ScratchpadView` 的字段，
复制 / 剪切 / 粘贴三个动作也都在 scratchpad 内部闭环。后果有两个：

- **跨模块搬运无通道**：归档、项目文件、草稿箱各自有自己的剪贴板语义，用户想把归档里的文件
  挪进草稿箱，只能绕系统文件管理器（离开应用再做一次，丢失「草稿」这层归属转换）；
- **一次性剪切是各模块自己实现的约定**：本轮靠在粘贴成功后手动 `view.clipboard = None`，
  下一个模块大概率会漏掉这一行。

更优形态：workbench 层放一个 `FileOpsBus`（或独立 `file_ops` crate）：

| 概念 | 形态 |
| --- | --- |
| 源 | `Vec<FileRef>`，`FileRef { provider: ProviderId, key: String }`（`key` 由提供方解释：草稿用模块内相对路径、归档用资产 id） |
| 操作 | `Copy` / `Cut` / `Move` + `generation: u64`（一次性剪切靠 generation 失效，不靠各模块自觉清空） |
| 目标 | `DropTarget { provider, dir_key }` |
| 提供方 | `trait FileOpsProvider { copy_in / move_in / copy_out / move_out / exists }`，草稿箱、项目文件、归档各实现一份 |

收益：跨模块拖拽与粘贴天然成立（`ExternalPaths` 拖入只是 `ProviderId::System` 的一种源）；
「剪切只能生效一次」「跨提供方移动要复制 + 删除」变成总线里的单点规则，不再每个模块各写一遍。

代价：接口收敛 + 三个提供方适配，约 300–400 行；建议先做**只含草稿箱 ↔ 项目文件**的最小总线
（两个提供方、只支持 `Copy` / `Move`），验证语义后再接归档。

## 8. 手动验收清单（需要一次真点击）

前置：`Start-Process target\debug\rds-app.exe -WorkingDirectory 'D:\RdataStation\RDS\RdataStation-v2'`，
打开示例项目，侧栏进「草稿」。

1. **导入弹窗**：点工具栏「⬇」→ 出现应用内 Dialog（标题「导入外部文件」），**不再直接弹系统框**。
2. **浏览回填**：点「浏览…」→ 系统框选 2 个文件 → 回来后清单出现 2 行（弹窗不关）。
3. **手动加入**：在输入框粘一个路径（`;` 分隔两条也行）→ 点「加入」→ 清单 +1/+2；
   重复加入同一路径 → 提示「这些路径已在列表里」，不重复。
4. **移除**：点某行「✕」→ 该行消失，其余行不错位。
5. **确认导入**：点「导入」→ 弹窗关闭，草稿根出现文件，badge 计数增加。
6. **粘贴-文件夹**：右键 `notes` →「复制」→ 右键 `sql` →「粘贴」→ `sql/notes_copy` 出现
   （`copy_entry` 固定加 `_copy` 后缀，跨目录也加，见 §4 W6）。
7. **粘贴-文件同级**：右键 `notes/1.sql` →「复制」→ 右键 `notes/README.md` →「粘贴」→
   `notes/1_copy.sql` 出现（不是粘进 `README.md` 里面，也不覆盖 `1.sql`）。
8. **粘贴-快捷键**：单击 `sql` 文件夹（不右键）→ `Ctrl+V` → 同样粘进 `sql`。
9. **剪切一次性**：右键某文件「剪切」→ 粘到另一目录 → 再按 `Ctrl+V` → **无第二次粘贴**
   （剪贴板已清空，右键菜单里「粘贴」也不再出现）。
10. **编辑态不误触**：`F2` 重命名某文件 → 在输入框里按 `Ctrl+V`（先复制一段文字）→
    文本粘进输入框、草稿树不发生任何粘贴。
