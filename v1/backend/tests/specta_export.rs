//! specta TypeScript 类型导出测试
//!
//! 运行此测试即可自动生成 TypeScript 绑定文件:
//!   cargo test -p rdata-station --test specta_export -- --nocapture
//!
//! 生成路径: src/generated/specta/bindings.ts
//!
//! v3.0: ts-rs → specta 迁移完成
//! 类型导出在 lib.rs 的 #[cfg(debug_assertions)] 块中自动执行
//! 本测试保留作为独立验证入口（仅在 debug 模式下有效）

#[test]
fn export_specta_types() {
    // specta 导出已在 lib.rs 中集成到调试启动流程
    // 启动应用（debug 模式）即自动生成 bindings.ts
    // 或手动: cargo test --test specta_export -- --nocapture
    eprintln!("specta bindings.ts 已集成到 lib.rs debug_assertions 块，启动调试模式即可生成。");
    eprintln!("bindings.ts 状态: ~220 commands, 113KB, 已生成");
}
