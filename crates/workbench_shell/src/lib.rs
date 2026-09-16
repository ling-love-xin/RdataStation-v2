//! 工作台外壳（workbench-shell）
//!
//! 定位：承接「面板状态 + 视图共用资产」，让 `workbench` 与**特性 crate**
//! （`database` / `scratchpad` / …）都能依赖它，从而解开视图下沉时的循环依赖。
//!
//! 为什么需要这一层：视图下沉到特性 crate 后，视图要用的面板状态与尺寸常量原本都在
//! `crates/workbench` 里；而 `workbench` 已经依赖那些特性 crate（`database` / `scratchpad`），
//! 于是「特性 crate → workbench」与「workbench → 特性 crate」构成环。把共用资产移到
//! 本 crate 后，依赖变成两侧都指向它，环被打断。
//!
//! 依赖方向：`workbench` → `workbench-shell`；`database` / `scratchpad`（下沉视图后）→ `workbench-shell`。
//! **本 crate 不得依赖任何特性 crate**（否则环会以另一种形状回来）。
//! 设计方案与逐步迁移计划见 `docs/architecture/layout/panels-coupling-plan.md` §8/§9。

pub mod model;
pub mod ui;
