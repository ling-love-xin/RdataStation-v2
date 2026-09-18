//! Quick Open（统一检索 / 命令面板）—— Phase 0 第一刀。
//!
//! 规格：`docs/architecture/quick_open/quick-open-prototype-design.md`；进度：`quick-open-dev-plan.md`。
//!
//! 分层：
//! - [`model`]：**纯逻辑**（模式前缀解析、命令目录、匹配评分、查询词转义），无 GPUI 依赖，可单测；
//! - [`delegate`]：`List` 委托（行快照 → 组件行；选中 / hover / 漫游 / 空态归组件）；
//! - 宿主装配与副作用：`crate::view`（`WorkbenchView` 的 `render_quick_open` /
//!   `execute_quick_open_row`）——弹层挂在工作台 overlay 上，不引 `Dialog`。
//!
//! 本轮范围（Phase 0 第一刀）：键盘通道（↑↓ / ↵ 三态 / Esc）、打开即聚焦、
//! `List` 组件化结果区、命中高亮、类型标签；数据源仍是**本地两类**（连接 / 命令）。
//! 元数据（跨连接名称档与全文档）在 Phase 0 第二刀接入后台索引搜索。

pub(crate) mod delegate;
pub(crate) mod model;
