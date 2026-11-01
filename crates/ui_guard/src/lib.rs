//! `rds-ui-guard` —— 全工作区 UI 契约守门（**只有测试，没有运行时代码**）。
//!
//! 之所以单独成一个 crate：既有的 `crates/{workbench,insight}/tests/ui_contract.rs` 只覆盖
//! 各自 crate（25 个 crate 里仅 2 个有守门），本守门扫 `crates/**/src/**/*.rs` 全量。
//! 它零产品依赖、只做源码与资产扫描，因此**不受单个 crate 编译状态影响**
//! （例如某个 crate 改到一半时，其余守门仍可跑）。
//!
//! 守门口径见 `tests/workspace_ui_contract.rs`。
