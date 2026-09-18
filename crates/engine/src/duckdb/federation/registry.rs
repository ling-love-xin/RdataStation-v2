//! 外部源**登记**：谁参与了联邦、叫什么别名、现在什么状态
//!
//! ## 职责（第一期落地）
//!
//! - **身份**：复用连接体系（`global_connections` + 密码加密），“哪些连接被用作联邦源”
//!   是这里的一列标记；本模块给每个源一个**稳定别名**（用户可改，**重名检测**）；
//! - **状态快照**：可用 / 挂载失败（带原因）/ 表数量 / 最后刷新时间 —— 这是界面「源清单」
//!   的数据源，必须是**内存快照**（渲染路径不做 I/O，与 `ChannelAvailability` 同一口径）；
//! - **不负责**连接与凭据本身（那是连接体系的事），也不负责挂载（那是 [`super::session`]）。
//!
//! ## 接口草案（第一期按此形状落地，名称可调）
//!
//! ```text
//! pub struct SourceEntry {
//!     pub id: String,          // 连接 id（与连接体系同一把钥匙）
//!     pub name: String,        // 展示名（连接名）
//!     pub alias: String,       // 挂载别名（SQL 里 `<别名>.<schema>.<表>` 用的就是它）
//!     pub driver: String,      // 驱动名（决定走 L1 / L2 / L3）
//!     pub state: SourceState,  // 可用 / 失败(原因) / 未验
//!     pub tables: Option<usize>,
//!     pub refreshed_at: Option<std::time::SystemTime>,
//! }
//!
//! pub enum SourceState { Ready, Failed(String), Unknown }
//!
//! impl SourceRegistry {
//!     pub fn entries(&self) -> Vec<SourceEntry>;                    // 内存快照
//!     pub fn primary(&self) -> Option<&str>;                        // 主源别名
//!     pub fn set_primary(&mut self, alias: &str) -> Result<(), String>;
//!     pub fn alias_conflict(&self, alias: &str) -> Option<&str>;    // 跨源重名检测
//! }
//! ```
//!
//! ## 一条规矩
//!
//! 源**挂不上**时不要把它从清单里删掉：保留条目 + `SourceState::Failed(原话)`，
//! 用户才知道“我配了这个源、它现在不可用、原因是这个”。静默消失是最糟的失败方式。
