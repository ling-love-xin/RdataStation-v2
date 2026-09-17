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
//! ## 本切片（切片二）的门控口径
//!
//! | 通道 | 条件 |
//! | --- | --- |
//! | 本地加速 | 连接存在 · **开启了 DuckDB 联邦**（`use_duckdb_fed`）· 驱动类型可加速（mysql / postgres / sqlite / duckdb）· **扩展可用**（试过且失败才拦，见 `accel::extension_state`） |
//! | 联邦 | 尚未接入（切片三：外部源注册入口） |
//!
//! 判据的顺序 = **谁能先改**：连接开关（用户自己就能改）→ 驱动类型（换连接）→ 扩展
//! （联网装一次 / 放离线包）。行尾给的原因就是第一条过不去的。
//!
//! **扩展为什么是“试过才拦”**：安装扩展要联网（首次）也真的会失败，但菜单每帧都要画，
//! 不能在这里做 I/O。所以没试过就不拦（让用户能选、执行时在**工作线程**上真装一次），
//! 试过且失败就把原因记下来，之后菜单行尾就说那条原因（重试成功会清掉）。
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
        } else {
            match engine::duckdb::accel::AccelKind::from_db_type(&connection.driver) {
                Err(reason) => ChannelAvailability::blocked(reason),
                // 扩展试过且失败 → 如实挡着（原因就是装扩展失败的原话）；没试过不拦
                Ok(kind) => match engine::duckdb::accel::extension_state(kind) {
                    Err(reason) => ChannelAvailability::blocked(reason),
                    Ok(()) => ChannelAvailability::ok(),
                },
            }
        };

        ChannelAvailabilitySet {
            // 联邦查询的入口（注册外部源）还没接：与其给个“看着能用”的选项，不如如实说
            federated: ChannelAvailability::blocked("联邦查询尚未接入（外部源注册入口）"),
            accelerated,
        }
    }
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
        assert!(
            gate.available,
            "开关开了、扩展没试过、驱动能加速 → 可造：{:?}",
            gate.reason
        );
        // 联邦的入口还没接：必须挂在“尚未接入”上，而不是模模糊糊的“不可用”
        let federated = availability.for_channel(ExecChannel::Federated);
        assert!(!federated.available);
        assert!(federated.reason.unwrap_or_default().contains("尚未接入"));
    }

    /// 驱动类型不能加速（比如将来接的 ClickHouse）→ 原因说在类型上，不报“开关没开”
    #[test]
    fn a_driver_without_acceleration_says_so() {
        let mut item = connection("P_c", true);
        item.driver = "clickhouse".to_string();
        let availability = port(vec![item]).availability(Some("P_c"));
        let gate = availability.for_channel(ExecChannel::Accelerated);
        assert!(!gate.available);
        assert!(gate.reason.unwrap_or_default().contains("clickhouse"));
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
