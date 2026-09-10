//! Build script for Rdata Station
//!
//! v3.0: ts-rs → specta 迁移，类型导出改为 specta::collect_types! + ts::export
//! 导出逻辑移至 lib.rs（调试模式启动时自动执行）和测试中。

fn main() {
    // Tauri build: 自动设置 rerun-if-changed 并校验 config/capabilities
    tauri_build::build();

    // 额外的 specta 相关重编译触发器
    println!("cargo:rerun-if-changed=src/core/types.rs");
    println!("cargo:rerun-if-changed=src/commands");
    println!("cargo:rerun-if-changed=src/core/persistence");
    println!("cargo:rerun-if-changed=src/core/services");
    println!("cargo:rerun-if-changed=src/core/models.rs");
    // specta 类型导出见 lib.rs #[cfg(debug_assertions)] 块
}
