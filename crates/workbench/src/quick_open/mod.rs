//! Quick Open（统一检索 / 命令面板）—— Phase 0。
//!
//! 规格：`docs/architecture/quick_open/quick-open-prototype-design.md`；进度：`quick-open-dev-plan.md`。
//!
//! 分层：
//! - [`model`]：**纯逻辑**（模式前缀解析、匹配评分、命中区间、元数据行映射、渲染上限），无 GPUI 依赖，可单测；
//! - [`commands`]：命令目录（稳定 id + 关键词 + 动作），同样是纯数据；
//! - [`delegate`]：`List` 委托（行快照 → 组件行；选中 / hover / 漫游 / 空态归组件）；
//! - [`palette`]：浮层本体（独立视图实体：输入 + 结果 + 键盘通道 + 防抖与回填），
//!   宿主只挂 overlay + 注入动作端口；
//! - 宿主装配与副作用：`crate::view`（`render_quick_open` / `execute_quick_open_action`）。
//!
//! 覆盖：键盘通道（↑↓ / ↵ 三态 / Esc）、打开即聚焦、`List` 组件化结果区、命中高亮、
//! 元数据名称档（跨连接索引搜索：防抖 + 过期批次丢弃 + 属性面板动作）。

pub(crate) mod commands;
pub(crate) mod delegate;
pub(crate) mod model;
pub(crate) mod palette;

#[cfg(test)]
mod tests;
