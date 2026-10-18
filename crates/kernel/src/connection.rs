//! 内核进程的发现与启动（kernelspec 解析 + 进程 spawn + connection file 读写）
//!
//! 与 jupyter-zmq-client 的 KernelManager 职责重叠的取舍：
//! - 本 crate 自己管理**进程与 connection file**（精确控制超时 / 路径 / 清理），
//!   ZMQ 四通道连接则委托 jupyter-zmq-client 的 connection 构造器。
//! - 本模块暂不做 kernelspec 目录扫描（`jupyter kernelspec list` 的完整解析），
//!   先支持「按内核命令直接启动」（ipykernel 的 `-m ipykernel_launcher -f <connfile>`），
//!   kernelspec 发现留给后续版本。
