//! 执行域：目标解析 / 执行端口 / 队列 / 结果存储 / 就地编辑 / 导出 / 脚本环境 / 分析请求
//!
//! 依赖方向：本目录是 editor 的执行骨干；宿主经 `QueryRunner` 端口注入执行器，
//! editor 不知道连接 / 驱动从哪来（`QueryRunner` / `ChannelsPort` 等均为注入端口）。

/// 【M8 输入】分析请求（`AnalysisRequest`：结果集「洞察此列」的下游输入）
pub mod analysis;
/// 【T2a】结果集就地编辑：变更集 pending/committed/conflict 三态
pub mod changes;
/// 【B13】执行通道：源库直连 / DuckDB 本地加速 / 联邦查询（三者互斥 + 门控如实）
pub mod channel;
/// 【T2a】结果集就地编辑的六道防线（纯函数）
pub mod editability;
/// 【2b】脚本执行环境（EnvPort / Environment / 选择器数据侧）
pub mod env;
/// 执行编排：`ExecTarget` 解析（纯函数）/ `QueryRunner` 端口 / `ExecQueue` 线程队列 / 回执类型
///
/// 自 execution.rs 拆分（2026-10-29）：目标解析 → `target`、执行端口 → `runner`、
/// 回执类型 → `notes`、队列 → `queue`；`execution` 保留兼容层 re-export（`crate::execution::*`
/// 旧路径不变）。
pub mod execution;
/// 【A14】执行目标（纯函数）：`ExecTarget` 解析 / 菜单映射 / 批量拆分
pub mod target;
/// 【A14】执行端口：`QueryRunner` trait（宿主注入）+ `QueryData` 产物
pub mod runner;
/// 【A14】执行回执：结论 / 事务 / 提交 / 导出 / 来源动作各 Note 与 `CommitReport` / `SubmitError`
pub mod notes;
/// 【A14】执行队列：按连接分组的工作线程 + mpsc + 主线程轮询泵
pub mod queue;
/// 【B7】导出（CSV / JSON / INSERT 编辑侧编码 + Parquet / XLSX 经 DuckDB COPY）
pub mod export;
/// 【A14】结果集存储（每文档一份结果集列表 + 选中项，上限 5）
pub mod store;
