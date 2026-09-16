//! rds-database — M4 数据库导航：元数据浏览器、对象属性面板。
//!
//! 规范说明：本 crate 是 v2 骨架占位，按 GPUI-kit 编码规范组织：
//! 同一业务能力的 model / service / view / command / dialog / workflow 放在一起。
//! 迁移源与逐项映射见 `docs/migration/v1-to-v2-mapping.md`。
#![allow(dead_code)]

pub mod cache;
pub mod metadata_service;

pub mod commands;
pub mod database_view;
pub mod model;
pub mod navigator_service;
pub mod nav_host;
pub mod nav_jobs;
pub mod nav_store;
pub mod nav_view;
pub mod property_panel;
pub mod sql_gen;
