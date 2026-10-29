//! 【#20】分析模式内核注册表端口（工作台实现）：探测一次 + 内存快照
//!
//! ## 一句话
//!
//! editor 侧 [`crate::kernel::KernelsPort`] 只消费「本机有哪些内核可用」的快照。
//! 这里是它的工作台实现：`attach` 时在**后台线程**跑一次 [`rds_kernel::detect_kernels`]
//! （起子进程探测解释器 + ipykernel，几十到几百毫秒，不该阻塞 UI 线程），结果缓存进
//! 内存；`list()` 是纯内存读（渲染路径可调）。
//!
//! ## 为什么探测要后台线程
//!
//! `detect_kernels` 会 `Command::new(..).output()` 起子进程（py / python / Rscript /
//! julia 的 `--version` + ipykernel `find_spec`）：每次几十毫秒起步，若解释器路径失效
//! 还可能挂到超时。放进 attach 的同步路径会卡开窗；渲染路径做 I/O 又违反 GPUI-kit
//! 纪律。所以：attach 时 `cx.background_executor().spawn` 一次，完成后写缓存。

use std::rc::Rc;

use crate::kernel::KernelsPort;
use crate::shared::EditorShared;

/// 端口实现：只持有探测结果缓存（attach 后由后台任务写入）
struct WorkbenchKernels {
    // Arc 而非 Rc：缓存要共享给 background 任务（spawn 的 future 必须 Send）
    cache: std::sync::Arc<std::sync::Mutex<Vec<rds_kernel::KernelSpec>>>,
}

impl KernelsPort for WorkbenchKernels {
    fn list(&self) -> Vec<rds_kernel::KernelSpec> {
        self.cache.lock().unwrap().clone()
    }
}

/// 把内核注册表端口接到编辑器共享状态（**宿主启动时调用一次**）
///
/// 探测结果**未就绪前** `list()` 返回空（菜单如实置灰，不假装探测过）；
/// 后台任务完成即写入，下一次渲染就有真值。
pub fn attach(shared: &EditorShared, cx: &gpui_kit::App) {
    let cache = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let port = Rc::new(WorkbenchKernels { cache: cache.clone() });
    shared.attach_kernels(port);
    let _ = cx.background_executor().spawn(async move {
        // 探测一次（后台线程：起子进程查解释器 / ipykernel）
        let specs = rds_kernel::detect_kernels();
        if let Ok(mut slot) = cache.lock() {
            *slot = specs;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 探测前 list 为空、探测完成写入（端口契约：未就绪如实空）
    #[test]
    fn port_reports_empty_until_detection_writes() {
        let port = WorkbenchKernels {
            cache: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        assert!(port.list().is_empty());
        let spec = rds_kernel::KernelSpec {
            id: "python".to_string(),
            display_name: "Python (系统)".to_string(),
            language: rds_kernel::KernelLanguage::Python,
            command: "python".to_string(),
            ready: true,
            note: None,
        };
        *port.cache.lock().unwrap() = vec![spec];
        assert_eq!(port.list().len(), 1);
    }
}
