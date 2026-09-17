//! RdataStation v2 SQL 编辑器 crate（editor）
//!
//! **一个内核 + 三档能力**：
//! - **文本模式**：带高亮的记事本，**不与数据库通信**（无连接、无执行、无结果区）
//! - **SQL 模式**：DBeaver 一档的脚本窗口（连接绑定 + 执行族 + 结果集 + 历史）
//! - **分析模式**：单元（Cell）+ 会话（Session）+ 输出（Output）的可执行笔记
//!
//! 设计文档：`docs/architecture/editor/`（原型设计 / 架构与决策 / 开发方案 / 交互稿）。
//!
//! ## 依赖方向（硬约束）
//!
//! `workbench → editor → engine / database / shared`；editor **不得**依赖 workbench。
//!
//! ## 当前状态（Phase 0 地基 + 1a 进行中）
//!
//! - ✅ `model`：文档 / 模式 / 只读 / 能力表
//! - ✅ `mode`：模式判定规则表（纯函数 + 表驱动测试）
//! - ✅ `service`：文档集合与生命周期（打开/关闭/激活/重命名/脏状态，A1）
//! - ✅ `shared` + `view/host`：宿主面板（一面板 = 一标签 = 一份文档；标题 / 脏点 / 编辑内核 / 状态栏，A2+A3+A6+A7）
//! - ✅ `view/highlight`：SQL 语义着色（A4）· ✅ `mode` 切换矩阵（A5）· ✅ `persist` 打开/保存与外部修改检测（A9）
//! - ✅ `edit` 文本操作纯函数（行注释开关）· ✅ `commands` 动作声明 + 面板 `key_context("editor")`（A10）
//! - ✅ 已注册键位（`crates/app`）：`ctrl-s` 保存 · `ctrl-/` 行注释 · `ctrl-w` 关闭当前文档
//!   （`ctrl-w` 的处理器在宿主 workbench：面板在自己的 `update` 里让 Dock 移除自己会重入）
//!   · `ctrl-enter` 执行（选区 > 当前语句）· `ctrl-shift-enter` 执行全部
//!   · `ctrl-o` 打开文件 · `ctrl-shift-s` 另存为（两者都弹系统文件对话框，宿主实现）
//! - ✅ `execution` + `store` + `view/widgets/result_grid`：最小执行闭环（A14，真机四库实测通过）
//! - ✅ 查找 / 替换（A11）：**内核能力 + 组件库面板**（`Ctrl+F` / `Ctrl+H`），本 crate 零自建；
//!   内核在 `Input` context 里先拿到按键且已注册 listener，外层再绑收不到（架构 §12 #24）
//! - ✅ `session`：会话端口（光标 / 选区 / 模式落库；A12，宿主注入实现）
//! - ✅ `limits`：文件档位（>50MB 关重能力 / ≥200MB 不读进内存；A13）· ui_contract 已覆盖本 crate（A15）
//! - ✅ `view/dialogs` + 关闭三态（A9）：三个对话框（关三态 / 保存失败 / 模式切换确认）· 落地入口
//!   `request_close_document` / `resolve_close_choice` / `request_save_as` · 另存为的路径选择是
//!   **注入端口**（`shared.attach_save_path_picker`，宿主用 `rfd` 实现——本 crate 不依赖 `rfd`）
//! - ✅ `ui`：本 crate 的结构尺寸常量（不反向依赖 workbench）
//! - ✅ `view/host` 的工具栏：最左**模式指示器**（`[SQL] ▾` → 三档菜单）+ SQL 模式的**执行 ▾**
//!   （主按钮 = 执行「选区优先 → 当前语句」；菜单 = `ExecMenuKind::ALL` 五项：当前语句 / 选区 / 全部 /
//!   批量（逐条独立）/ 在新结果标签中执行；没选区或不足两句的项**置灰**而不是无响应）。
//!   其余控件（格式化 / 历史 / 更多 / 执行位置）**未实现就不放**（原型 §2.2 分层）
//! - ✅ 执行族与多结果集（B2）：`ExecTarget::Batch` + `batch_target()`（词法级切分，逐条独立）·
//!   `ResultPlacement{Replace, NewSet}`（在新结果标签中执行 = 追加且**原结果集保持选中**）·
//!   通道按 **job** 粒度执行（一条 job 多条语句顺序跑、逐条回填、**失败不中断**，忙标记覆盖整批）·
//!   `ResultStore` 每文档一份**结果集列表 + 选中项**（上限 5，淘汰最旧的未选中项）·
//!   结果集标签条 `view/widgets/result_sets.rs`（组件库 `TabBar::segmented`；两份以上才画）·
//!   结果区唯一读点 `EditorHostPanel::sync_result_view`
//! - ✅ 中断与超时（B3）：入口 = 状态栏 `■ 中断`（原型 §5.1 给的就是状态栏 ■；
//!   `Ctrl+Break` 在 GPUI / 本机键盘上不可得，「等价入口」就是它）+ 耗时累加 `执行中 3.4s…`；
//!   `QueryRunner::cancel` 是三态可读的端口（默认实现明说“不支持中断”），中断在一次性线程上做、
//!   回执不吞；批量中断后**剩余语句标“已取消”且不再发给驱动**；超时按连接的 `query_timeout`
//!   交给引擎（到点自动取消 + `Query timed out after Nms`；连接没配就是没有超时）
//! - ✅ 事务（B4）：活动事务是**连接级会话状态**（引擎侧挂 `ConnectionManager`，事务对象持有那条
//!   物理连接）；`begin/commit/rollback` 走驱动接口，语句在事务里就走事务对象；自动提交关闭时
//!   执行先自动开一个（DDL 除外）。编辑器侧：端口 `transaction(action)` / `transaction_snapshot`，
//!   事务状态**随执行结论回填**，状态栏 TX 区 = 状态 + 时长 + 自动提交开关 + 提交/回滚
//!   （只在执行器真支持事务时出现）
//! - ✅ 关闭口径（B16）：脏文档 `closable == false`（Dock 的 ✕/关闭菜单按**当前标签**取该判据，
//!   脏时入口直接消失），关闭脏文档的唯一入口是 `Ctrl+W` → 三态确认
//! - ✅ `connection`（B1 切片一）：文档绑定连接（与 `mode` / `read_only` 同类）· 端口注入
//!   （`shared.attach_connections`，宿主给列表 + 自动建连）· 工具栏「连接 ▾」（未完绑定→跟随当前连接）
//!   · 状态栏最左的连接段（`●P·orders` / `○ 未绑定连接`）· 执行时绑定随目标走执行通道
//! - ✅ 对外接口（B11）：`EditorHostPanel::run_all`（宿主发起执行：导航「查看数据」打开即跑）；
//!   导航的「在 SQL 编辑器中打开 / 查看数据 / 生成 SQL / 拖拽」统一走宿主 `open_query_document`
//!   （请求形状在宿主侧：`panels::QueryRequest`）
//! - ⬜ 1b 待做：`completion`（B9）· 格式化 / 转译 / 执行计划（B10）· 结果区分栏可拖拽（B5）·
//!   结果工具栏与标签的通道徽标 / 血缘摘要（B5，依赖 B13/B14/B15）· 导出（B7：CSV / JSON / INSERT 已做，
//!   Parquet / XLSX 待 DuckDB 扩展）
//!   · 绑定随会话持久化（B1 余项）· 执行位置三通道（B13）
//!   · **项目只读模式对执行的拦截**（旧路径有，1b 删除时有遗失，需在新执行入口补回）
//!   · 事务内的语句暂不可取消 / 无超时（驱动 `Transaction` trait 没有取消入口）
//! - ⬜ 1c：`session` / `notebook`（Cell / Output / Session）
//!
//! 语句切分（「执行当前语句」与「批量执行」的基础）落在 `engine::sql::split`：它是不带编辑器
//! 状态的 SQL 文本原语，属 engine（`core/sql` 是 sqlglot 的唯一接入点），editor 直接消费，
//! 不在本 crate 重复实现。

pub mod commands;
pub mod connection;
pub mod diagnostics;
pub mod edit;
pub mod execution;
pub mod export;
pub mod limits;
pub mod mode;
pub mod model;
pub mod persist;
pub mod service;
pub mod session;
pub mod shared;
pub mod store;
pub mod ui;
pub mod view;
