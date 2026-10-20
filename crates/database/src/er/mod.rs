//! ER 建模视图（M4 数据源管理 / 导航）——原生 GPUI 绘制，不引入 webview。
//!
//! 设计：`docs/architecture/database/database-er-model-view-design.md`；
//! 与插件的关系：`docs/architecture/plugin/plugin-prototype-design.md` §11.3 / §14.3。
//!
//! 分层（逐片接入）：
//! - [`model`]：ER 输入模型 + 「元数据 → 模型」桥（纯函数）；
//! - [`layout`]：分层布局（纯函数、确定性）；
//! - [`edge`]：连线几何（纯函数：正交折线 + 端点吸附到列行）；
//! - [`loader`]：**唯一有 I/O 的一层**（元数据服务 → 图）；只应在后台任务线程调用；
//! - [`view`]：GPUI 渲染（实体卡片 + `canvas` 连线），不引 webview。
//!
//! 分期（对照设计文档 §10）：① 模型 / 桥 / 布局（已落地）；② 只读渲染 + 面板装配（本片）；
//! ③ pan/zoom + 拖拽排布 + 布局持久化；④ DDL 导出；⑤ 写回源库（后置）。

pub mod edge;
pub mod layout;
pub mod loader;
pub mod model;
pub mod view;

pub use edge::{edge_paths, ErEdge, ErPoint};
pub use layout::{layered_layout, EntityBox, ErRect, LaidOutDiagram, LayoutMetrics};
pub use loader::{load_diagram, MAX_TABLES};
pub use model::{
    build_diagram, EntityKey, ErColumn, ErColumnRef, ErDiagram, ErEntity, ErRelation, TableInput,
};
pub use view::{metrics_for_theme, render_er_diagram};
