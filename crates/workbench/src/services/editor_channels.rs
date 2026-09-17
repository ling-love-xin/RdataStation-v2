//! 编辑器执行通道端口的工作台实现（B13）：源库 / 本地加速 / 联邦 三档的门控真值
//!
//! ## 职责边界
//!
//! 编辑器只问一件事：“这个连接上「本地加速 / 联邦」现在能用吗？不能用是为什么？”
//! （`editor::channel::ChannelsPort`）。**能不能用是工作台的事实**：它知道连接的
//! `use_duckdb_fed` 开关、分析引擎（DuckDB）起没起来、外部源注册了几个。
//!
//! ## 为什么必须“如实”
//!
//! 端口方法会被**渲染路径**调用（工具栏每帧画「执行位置 ▾」）。门控错成“能用”比错成
//! “不能用”严重得多：用户会以为自己在看本地副本 / 联邦结果，实际跑的还是源库——
//! 那是一条**静默的语义错误**。所以这里逐条给原因，宁可不给选。
//!
//! ## 本切片（切片一）的门控口径
//!
//! | 通道 | 条件 |
//! | --- | --- |
//! | 本地加速 | 连接存在 · **开启了 DuckDB 联邦**（`use_duckdb_fed`）· **执行侧已接入加速路由** · 分析引擎已起来 |
//! | 联邦 | 同上 + **已注册外部源**（切片二/三才接） |
//!
//! 判据的顺序 = **谁能先改**：连接开关（用户自己就能改）→ 版本能力（执行侧接没接）→
//! 运行时就绪（引擎起没起来）。行尾给的原因就是第一条过不去的，而不是一堆条件的合集。
//!
//! “执行侧尚未接入”这一条是**如实**的关键：宿主执行器（`editor_exec.rs`）目前只会
//! 把语句发到源库，所以这一档现在不可选——原因就写在菜单行尾。切片二接上真正的
//! `ATTACH` 路由后，去掉这条判据即可（端口是唯一的开关处）。
//!
//! ## 不查库、不建连
//!
//! 连接信息读 `Shared::connections`（导航维护的**内存快照**，与界面上看到的是同一份）；
//! 分析引擎的“起没起来”用 `DuckDBManager::is_initialized()`（纯读，不触发初始化）。

use std::rc::Rc;

use editor::channel::{ChannelAvailability, ChannelAvailabilitySet, ChannelsPort};
use editor::shared::EditorShared;

use crate::panels::Shared;

/// 工作台实现：三档通道的可用性（读内存快照，不做 I/O）
struct WorkbenchChannels {
    shared: Shared,
}

impl ChannelsPort for WorkbenchChannels {
    fn availability(&self, conn_id: Option<&str>) -> ChannelAvailabilitySet {
        let Some(conn_id) = conn_id else {
            // 未绑定连接时没有“要加速的源”：加速档缺的是源，联邦档缺的是外部源
            return ChannelAvailabilitySet {
                accelerated: ChannelAvailability::blocked("先绑定一个连接"),
                federated: ChannelAvailability::blocked("先绑定一个连接"),
            };
        };
        let connection = self
            .shared
            .connections
            .borrow()
            .iter()
            .find(|item| item.id == conn_id)
            .cloned();
        let Some(connection) = connection else {
            return ChannelAvailabilitySet::blocked("该连接已不在当前列表里");
        };

        let accelerated = if !connection.use_duckdb_fed {
            ChannelAvailability::blocked("该连接未开启本地加速（连接设置里的 DuckDB 联邦）")
        } else if !execution_supports_accelerated() {
            // 这一条是**如实**：宿主执行器还没接加速路由，选了也会发到源库
            ChannelAvailability::blocked("本地加速的执行尚未接入")
        } else if !engine::duckdb::DuckDBManager::is_initialized() {
            ChannelAvailability::blocked("分析引擎尚未就绪")
        } else {
            ChannelAvailability::ok()
        };

        ChannelAvailabilitySet {
            federated: if accelerated.available {
                ChannelAvailability::blocked("尚未注册外部源")
            } else {
                // 连加速都不通就谈不上联邦：把第一道缺口如实说出来（别让用户以为是“缺外部源”）
                accelerated.clone()
            },
            accelerated,
        }
    }
}

/// 本进程的执行器有没有“把语句发到本地加速副本”的能力
///
/// 切片二接上 `federation::attach_data_source` 之后这里改回 `true`——门控只有一个开关处，
/// 免得散在界面上（用户改一处就生效）。
fn execution_supports_accelerated() -> bool {
    false
}

/// 把通道端口接到编辑器共享状态上（**启动装配调用一次**）
pub fn attach(shared: &EditorShared, workbench: &Shared) {
    shared.attach_channels(Rc::new(WorkbenchChannels {
        shared: workbench.clone(),
    }));
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::WorkbenchChannels;
    use editor::channel::{ChannelsPort, ExecChannel};
    use workbench_shell::model::ConnectionItem;

    use crate::panels::Shared;

    fn connection(id: &str, fed: bool) -> ConnectionItem {
        ConnectionItem {
            id: id.to_string(),
            name: id.to_string(),
            driver: "mysql_native".to_string(),
            connected: true,
            host: None,
            port: None,
            database: None,
            schema: None,
            description: None,
            use_duckdb_fed: fed,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn port(connections: Vec<ConnectionItem>) -> WorkbenchChannels {
        WorkbenchChannels {
            shared: Shared::with_connections(connections, None),
        }
    }

    /// 没开加速开关的连接：加速档置灰，原因**指到具体那个开关**（不是含糊的“不可用”）
    #[test]
    fn a_connection_without_the_switch_blocks_the_accelerated_channel() {
        let availability = port(vec![connection("P_a", false)]).availability(Some("P_a"));
        let gate = availability.for_channel(ExecChannel::Accelerated);
        assert!(!gate.available);
        assert!(
            gate.reason.unwrap_or_default().contains("未开启本地加速"),
            "原因要说清是哪个开关"
        );
    }

    /// 开关开着但执行侧没接：**照样不可用**（宁可不给选，也不要选了却发到源库）
    #[test]
    fn an_unwired_execution_side_still_blocks_the_channel() {
        let availability = port(vec![connection("P_a", true)]).availability(Some("P_a"));
        let gate = availability.for_channel(ExecChannel::Accelerated);
        assert!(!gate.available, "执行侧还没接加速路由，不许说能用");
        assert!(gate.reason.unwrap_or_default().contains("尚未接入"));
        // 联邦的缺口按“第一道没通的”说（尚未注册外部源属更靠后的一档）
        let federated = availability.for_channel(ExecChannel::Federated);
        assert!(!federated.available);
        assert!(!federated.reason.unwrap_or_default().is_empty());
    }

    /// 未绑定连接 / 连接不在列表：两档都要给可读原因，而不是静默不可用
    #[test]
    fn no_source_means_a_readable_reason() {
        let unbound = port(Vec::new()).availability(None);
        assert!(
            unbound
                .for_channel(ExecChannel::Accelerated)
                .reason
                .unwrap_or_default()
                .contains("绑定")
        );

        let gone = port(Vec::new()).availability(Some("P_gone"));
        assert!(
            gone.for_channel(ExecChannel::Federated)
                .reason
                .unwrap_or_default()
                .contains("不在当前列表")
        );
    }

    /// 源库档不在这儿回答（它看连接是否已建连，由编辑器一侧判）
    #[test]
    fn the_source_channel_is_always_reported_as_ok_here() {
        let availability = port(Vec::new()).availability(Some("P_gone"));
        assert!(availability.for_channel(ExecChannel::Source).available);
    }
}
