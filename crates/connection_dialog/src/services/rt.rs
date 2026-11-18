//! 进程内共用 tokio 运行时。
//!
//! 为什么不是每处 `Runtime::new()`：对话框首开时会串起十几条「同步 UI → 异步 store」的
//! 查询，每条都新建一个 `Runtime`（自有线程池 + IO driver）。9 次创建 ≈ 9 次线程池
//! 启停，全压在同一帧的 UI 线程上——首次打开明显发涩。
//!
//! 这里只做**一件事**：给整进程一个惰性初始化的 `Runtime`，调用方拿 `&'static Runtime`
//! 直接 `block_on`。注意两条边界：
//! - **不要**在 async 上下文里调 `runtime()` 再 `block_on`（tokio 会 panic：
//!   Cannot start a runtime from within a runtime）；本模块的调用点全是同步 UI / 服务入口。
//! - 运行时只初始化一次，`Runtime::new()` 失败（极少见）就返回 `None`，由调用方降级。

use std::sync::OnceLock;
use tokio::runtime::Runtime;

/// 共用运行时（惰性初始化；不可用时返回 `None`）。
pub(crate) fn runtime() -> Option<&'static Runtime> {
    static RT: OnceLock<Option<Runtime>> = OnceLock::new();
    RT.get_or_init(|| Runtime::new().ok()).as_ref()
}

/// 运行时不可用时的统一降级文案（调用方拼进结果行 / 返回值，不静默吞掉）。
pub(crate) const RT_UNAVAILABLE: &str = "异步运行时不可用";

#[cfg(test)]
mod tests {
    /// 共用运行时只初始化一次：同一进程内两次取到的是同一个实例（换回「每处 new 一个」
    /// 会让首开重新出现十余次线程池启停）。
    #[test]
    fn shared_runtime_is_initialized_once() {
        let first = super::runtime().map(|rt| rt as *const _);
        let second = super::runtime().map(|rt| rt as *const _);
        assert!(first.is_some(), "测试环境应能创建运行时");
        assert_eq!(first, second, "两次取到必须是同一个运行时");
    }
}
