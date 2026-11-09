//! Quick Open（统一检索 / 命令面板）—— Phase 0。
//!
//! 规格：`docs/architecture/quick_open/quick-open-prototype-design.md`；进度：`quick-open-dev-plan.md`。
//!
//! 分层（P2-c 下沉 + 重构 S0–S3 后）：
//! - [`model`] / `commands`：**纯逻辑**（模式前缀解析、匹配评分、命中区间、元数据行映射、
//!   渲染上限、命令目录）已下沉到 `editor::quick_open`，本模块重导（`crate::quick_open::model::*`
//!   路径不变）；
//! - [`delegate`]：`List` 委托（行快照 → 组件行；选中 / hover / 漫游 / 空态归组件）；
//! - [`palette`]：浮层本体（独立视图实体：输入 + 结果 + 键盘通道 + 防抖与回填），
//!   宿主只挂 overlay + 注入动作端口；
//! - [`host`]：宿主装配与副作用（标题栏入口控件 / 懒创建 / 挂载 / 动作落地），
//!   `crate::view` 只留 `toggle_quick_open` 与调用点。
//!
//! 覆盖：键盘通道（↑↓ / ↵ 三态 / Esc）、打开即聚焦、`List` 组件化结果区、命中高亮、
//! 元数据名称档（跨连接索引搜索：防抖 + 过期批次丢弃 + 属性面板动作）。

pub(crate) mod delegate;
pub(crate) use editor::quick_open::model;
pub(crate) mod host;
pub(crate) mod palette;

#[cfg(test)]
mod tests;
