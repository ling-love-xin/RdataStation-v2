//! 本仓 vendor 补丁：wasm32-unknown-unknown 的 `std::time::Instant::now()`
//! 直接 panic（std sys/time/unsupported.rs: "time not implemented on this platform"），
//! 而上游 layout 的 stage metrics 用 Instant 计时（纯诊断用途，不进 SVG 产物）。
//! 该平台没有性能计数器（performance.now 不可用），提供恒 0 的替代计时器：
//! `elapsed()` 永远返回 ZERO，`saturating_add(0)` 后 metrics 保持初始值。
//!
//! 仅在 wasm 目标编译；native 仍用真实 `std::time::Instant`。
#[derive(Clone, Copy, Debug)]
pub struct Instant;

impl Instant {
    pub fn now() -> Self {
        Instant
    }

    pub fn elapsed(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}
