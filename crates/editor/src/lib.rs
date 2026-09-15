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
//!   （主按钮 = 执行「选区优先 → 当前语句」；菜单 = 当前语句 / 选区 / 全部，后两项各自独立）。
//!   其余控件（格式化 / 历史 / 更多 / 执行位置 / 连接）**未实现就不放**（原型 §2.2 分层）
//! - ✅ 关闭口径（B16）：脏文档 `closable == false`（Dock 的 ✕/关闭菜单按**当前标签**取该判据，
//!   脏时入口直接消失），关闭脏文档的唯一入口是 `Ctrl+W` → 三态确认
//! - ✅ `connection`（B1 切片一）：文档绑定连接（与 `mode` / `read_only` 同类）· 端口注入
//!   （`shared.attach_connections`，宿主给列表 + 自动建连）· 工具栏「连接 ▾」（未完绑定→跟随当前连接）
//!   · 状态栏最左的连接段（`●P·orders` / `○ 未绑定连接`）· 执行时绑定随目标走执行通道
//! - ⬜ 1b 待做：`completion`（B9）· 格式化 / 转译 / 执行计划（B10）· 结果区分栏可拖拽与多结果集（B5）
//!   · 中断与超时（B3）· 事务（B4）· 绑定随会话持久化（B1 余项）· 执行位置三通道（B13）
//! - ⬜ 1c：`session` / `notebook`（Cell / Output / Session）
//!
//! 语句切分（「执行当前语句」与「批量执行」的基础）落在 `engine::sql::split`：它是不带编辑器
//! 状态的 SQL 文本原语，属 engine（`core/sql` 是 sqlglot 的唯一接入点），editor 直接消费，
//! 不在本 crate 重复实现。

pub mod commands;
pub mod connection;
pub mod edit;
pub mod execution;
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
