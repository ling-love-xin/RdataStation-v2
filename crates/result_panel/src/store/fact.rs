//! 事实档载体状态（L0 事实层）——自 store.rs 拆分（2026-10-29）
//!
//! [`FactState`] 三态状态机（`None` / `InMemory` / `OnDuckDb`）与方案 §4.1 对齐；
//! 可追加段事实档合并（[`merge_fact_state`]）随 `ResultStore` 归 store 子模块。

use ::shared::FactFrame;
use std::sync::Arc;
/// 【A2】事实档载体状态（L0 事实层；方案 §4.1 三态状态机）
///
/// 与 [`ResultEntry`] 平行存放在 `DocumentResults.facts`，**不进 `ResultEntry`**：
/// `Arc<FactFrame>` 深拷贝代价高，挂进 `ResultEntry` 的 `Clone` / `PartialEq` 会触发
/// 逐值比较与整帧深拷贝。本枚举同样不 derive `Clone`，槽位整体替换 / 淘汰。
///
/// - [`FactState::None`]：失败结果 / 写语句 / 加速档 / 联邦档 / JSON 往返 —— 显示层走
///   internal 猜型降级档，界面与报告显式标注；
/// - [`FactState::InMemory`]：内存档，持有共享事实帧；触发式物化（分析 / 带类型导出 /
///   IronCalc 快照）后常驻复用，重复分析不重建；
/// - [`FactState::OnDuckDb`]：表档（B2 接通）：常驻表已物化，帧上挂表名且 batches 已
///   释放，只保留 schema / 列名 / 行数供元数据与类型族访问；表档上的本地交互一律 SQL
///   下推，不把表读回 Rust 堆（方案 §4.1 表档边界）。
///
/// 【A3】分段追加不改动帧的 `Arc` 所有权：可追加分支把旧段 + 新段 batches 合并后重建帧
/// （[`FactFrame::new`] 统一做形状校验；校验失败整档作废 `None`），不要求 `FactFrame: Clone`。
#[derive(Debug)]
pub enum FactState {
    None,
    InMemory(Arc<FactFrame>),
    OnDuckDb(Arc<FactFrame>),
}

impl FactState {
    /// 无事实档起步态（A2：上游 A3 尚未接入事实帧，入槽一律 `None`）
    pub const fn none() -> Self {
        Self::None
    }

    /// 从回执事实候选落档：`Some` → 内存档；`None` → 无事实档（显示矩阵降级档）
    pub fn from_fact(fact: Option<Arc<FactFrame>>) -> Self {
        match fact {
            Some(frame) => Self::InMemory(frame),
            None => Self::None,
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// 内存档事实帧引用；表档 / 无事实档返回 `None`
    pub fn in_memory(&self) -> Option<&FactFrame> {
        match self {
            Self::InMemory(frame) => Some(frame),
            _ => None,
        }
    }

    /// 表档事实帧引用；内存档 / 无事实档返回 `None`
    pub fn on_duckdb(&self) -> Option<&FactFrame> {
        match self {
            Self::OnDuckDb(frame) => Some(frame),
            _ => None,
        }
    }

    /// 统一取事实帧的 `Arc` 句柄（内存档 / 表档都给；无事实档 = `None`）。
    /// 调用方克隆 `Arc` 出借用边界后只读 schema / 列名 / 行数 / 表名；取行列值必须先区分
    /// 载体（表档不回读 Rust 堆）。
    pub fn fact(&self) -> Option<&Arc<FactFrame>> {
        match self {
            Self::InMemory(frame) | Self::OnDuckDb(frame) => Some(frame),
            Self::None => None,
        }
    }

    /// 常驻表名（`None` = 尚未物化；内存档挂表名后也会回这里）。
    pub fn temp_table(&self) -> Option<&str> {
        self.fact().and_then(|frame| frame.temp_table())
    }

    /// 当前载体标签（状态栏 / 右栏占用可见性展示用；方案 §4.1 三态对齐）。
    pub fn tier(&self) -> ::shared::CurrentTier {
        match self {
            Self::None => ::shared::CurrentTier::None,
            Self::InMemory(_) => ::shared::CurrentTier::InMemory,
            Self::OnDuckDb(_) => ::shared::CurrentTier::OnDuckDb,
        }
    }

    /// 已占用的事实档估算字节：内存档按列式 buffers 估算；表档 batches 已释放，
    /// Rust 堆占用报 0（表内占用归 DuckDB 侧统计，不在这里估）；无事实档报 0。
    pub fn estimated_bytes(&self) -> usize {
        match self {
            Self::InMemory(frame) => frame.estimated_bytes(),
            _ => 0,
        }
    }
}
