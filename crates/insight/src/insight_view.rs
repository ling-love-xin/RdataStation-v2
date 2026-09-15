//! 右 Dock 洞察面板的**内容视图**（M8 Phase 1 落地）。
//!
//! 规划结构（见 `docs/architecture/insight/insight-prototype-design.md` §2、§3）：
//! - 面板头：当前目标名 + 类型徽标 + 动作（规则管理 ⚙ / 刷新 ⟳）
//! - Tab 条：列 / 表 / 多列 / 结构 / 历史（五项 × 2 字，在 17.5rem 面板宽内不溢出）
//! - 内容按目标分派：列画像四区（基础统计 / 数据分布 / 数据质量 / 样本）+ 质量评分卡
//!
//! 归属备注：视图是否落到本 crate 尚待拍板（方案 A 入本文件 / 方案 B 留
//! `workbench/panels.rs`），见 `insight-dev-plan.md` §3.1。若选 B，本文件删除、内容进 workbench。
//!
//! 本文件当前为空占位。
