//! 编辑器执行通道端口的工作台实现（B13）：源库 / 本地加速 / 联邦 三档的门控真值
//!
//! ## 职责边界
//!
//! 编辑器只问一件事：“这个连接上「本地加速 / 联邦」现在能用吗？不能用是为什么？”
//! （`crate::channel::ChannelsPort`）。**能不能用是工作台的事实**：它知道连接的驱动类型、
//! 项目把这它登记成了哪种分析源、分析引擎（DuckDB）起没起来、外部源注册了几个。
//!
//! ## 能力与启用分开（2026-11-27 `use_duckdb_fed` 解耦）
//!
//! 连接记录上**没有“开关”这个字段**了。两件事各有唯一判据：
//!
//! - **能力**（这条连接的驱动做不做得了源 / 加不加速）：驱动常量，`AccelKind::from_db_type`
//!   与 `local_accel_support()`。
//! - **启用**（项目要不要拿它做源）：**意图清单**，`analysis_sources` 里的 `mount_kind`；
//!   执行侧读它的进程级快照（`mount_kind_for_connection`），不回查项目库。
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
//! | 本地加速 | 连接存在 · **已登记为加速源**（`analysis_sources` 的 `mount_kind = Accel`）· 驱动类型可加速（mysql / postgres / sqlite / duckdb）· **该驱动本身支持本地加速**（Oracle 这类 L2 只做联邦源）· **扩展可用**（试过且失败才拦，见 `accel::extension_state`） |
//! | 联邦 | **至少一个**「驱动可挂」且（**项目登记过**（任意档）/ 本文档绑定 `focus`）的连接（含 Oracle 这类 L2；不要求已连接；见 [`federated_availability`]） |
//!
//! 判据的顺序 = **谁能先改**：登记（面板里挂上 / 取消）→ 驱动类型（换连接）→ 本地加速支持
//! （L2 换联邦档）→ 扩展（联网装一次 / 放离线包）。行尾给的原因就是第一条过不去的。
//!
//! **联邦不再要求两个源**（2026-11-25 用户口径）：联邦与二次分析是同一件事，只差入口和主源。
//! 从源库档切联邦最常见的诉求就是「把手上这条连接挂进 DuckDB 当主源、SQL 一字不改」，
//! 所以一条源照给入口、照样成会话；主源 = 本文档绑定的连接。
//! 摆两个入口只会让人猜“这俩差在哪”——与其给个看着能用、实则复制的选项，不如把差的那一个源
//! 说出来。
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

use crate::channel::{ChannelAvailability, ChannelAvailabilitySet, ChannelsPort};
use crate::host::EditorHost;
use crate::shared::EditorShared;
use workbench_shell::model::ConnectionItem;

/// 工作台实现：三档通道的可用性（读内存快照，不做 I/O）
struct WorkbenchChannels {
    host: Rc<dyn EditorHost>,
}

impl ChannelsPort for WorkbenchChannels {
    /// 弹窗与"挂上"都是宿主的活（这里只转交；连接清单 / 能力判断宿主自己读得到）
    fn confirm_federated_mount(
        &self,
        conn_id: &str,
        on_decision: Box<dyn FnOnce(bool, &mut gpui_kit::App)>,
        cx: &mut gpui_kit::App,
    ) {
        self.host.confirm_federated_mount(conn_id, on_decision, cx);
    }
    fn availability(&self, conn_id: Option<&str>) -> ChannelAvailabilitySet {
        let Some(conn_id) = conn_id else {
            // 未绑定连接时没有“要加速的源”：加速档缺的是源，联邦档缺的是外部源
            return ChannelAvailabilitySet {
                accelerated: ChannelAvailability::blocked("先绑定一个连接"),
                federated: ChannelAvailability::blocked("先绑定一个连接"),
            };
        };
        let connection = self
            .host
            .connections()
            .into_iter()
            .find(|item| item.id == conn_id);
        let Some(connection) = connection else {
            return ChannelAvailabilitySet::blocked("该连接已不在当前列表里");
        };

        ChannelAvailabilitySet {
            federated: federated_availability(&self.host.connections(), Some(conn_id)),
            accelerated: accelerated_availability(&connection),
        }
    }
}

/// 加速档的可用性（**纯函数**，只看连接快照 + 登记快照：渲染路径可调）
///
/// 2026-11-27 起**能力**由驱动常量答（`AccelKind`），**启用**由项目意图清单答
/// （`analysis_sources` 的 `mount_kind = Accel`）。连接记录上不再有开关字段，
/// 所以“没登记”与“驱动不行”是两条不同的原因，各自点名。
fn accelerated_availability(item: &ConnectionItem) -> ChannelAvailability {
    let kind = match engine::duckdb::accel::AccelKind::from_db_type(&item.driver) {
        Ok(kind) => kind,
        Err(reason) => return ChannelAvailability::blocked(reason),
    };
    match engine::persistence::analysis_sources::mount_kind_for_connection(&item.id) {
        None => {
            return ChannelAvailability::blocked(
                "这条连接还没登记为分析源（在数据面板里挂上它、选「本地加速」即可）",
            )
        }
        Some(engine::persistence::analysis_sources::AnalysisMountKind::Accel) => {}
        Some(other) => {
            return ChannelAvailability::blocked(format!(
                "这条连接登记的是「{}」档，不是本地加速源（要加速请在数据面板里改挂载方式）",
                other.label()
            ))
        }
    }
    // 两条能力判据：本地加速支持（L2 只做联邦源）→ 扩展（试过且失败才拦）
    if let Err(reason) = kind.local_accel_support() {
        return ChannelAvailability::blocked(reason);
    }
    if let Err(reason) = engine::duckdb::accel::extension_state(kind) {
        return ChannelAvailability::blocked(reason);
    }
    ChannelAvailability::ok()
}

/// 联邦档的可用性（**纯函数**，只看连接快照 + 登记快照：渲染路径可调）
///
/// 可用 = **至少一个**「算源」且驱动能挂的连接（主源 + 外部源）。**谁算源是两源合一**
/// （2026-11-27 起不再看连接开关）：项目登记过（任意档）/ `focus`（本文档绑定的连接，
/// 单源主源）。
/// **含 Oracle 这类 L2**（它不能本地加速，但联邦源能挂——扫描器读得到）。
/// **不要求已连接**：源是从连接记录组装并 `ATTACH` 的（DuckDB 自己建连），没原生驱动的库
/// （Oracle 这类）也是这样进来的——“连不上”不是“不能做源”。
/// 不可用时把「还差什么」说出来：一个源都没选 / 驱动不支持——两种原因两种修法。
fn federated_availability(
    connections: &[ConnectionItem],
    focus: Option<&str>,
) -> ChannelAvailability {
    let mut unsupported: Vec<String> = Vec::new();
    let mut ready: Vec<String> = Vec::new();
    // 两源合一：项目登记过 / 本文档绑定（`focus`）——登记是唯一启用判据，没有开关字段可看
    for item in connections {
        let is_focus = focus == Some(item.id.as_str());
        let registered = engine::persistence::analysis_sources::is_registered(&item.id);
        if !registered && !is_focus {
            continue;
        }
        if engine::duckdb::accel::AccelKind::from_db_type(&item.driver).is_err() {
            unsupported.push(item.name.clone());
            continue;
        }
        ready.push(item.name.clone());
    }
    if ready.is_empty() && unsupported.is_empty() {
        return ChannelAvailability::blocked(
            "还没有可作为联邦源的连接（在数据面板里把连接挂上、登记成分析源即可）",
        );
    }
    if !ready.is_empty() {
        return ChannelAvailability::ok();
    }
    // 走到这里 = 有「算源」的连接，但驱动都挂不上：逐条点名
    ChannelAvailability::blocked(format!(
        "联邦查询至少需要一个可作为联邦源的连接（{}）",
        format!("{} 的驱动还不能做联邦源", unsupported.join("、"))
    ))
}

/// 把通道端口接到编辑器共享状态上（**启动装配调用一次**）
pub fn attach(shared: &EditorShared, host: Rc<dyn EditorHost>) {
    shared.attach_channels(Rc::new(WorkbenchChannels { host }));
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard};

    use super::WorkbenchChannels;
    use crate::channel::{ChannelsPort, ExecChannel};
    use crate::host::EditorHost;
    use engine::persistence::analysis_sources::{
        publish_aliases, AnalysisMountKind, AnalysisSourceKind, AnalysisSourceRow,
    };
    use gpui_kit::App;
    use workbench_shell::model::ConnectionItem;

    /// 测试宿主：只提供连接快照（不接真实导航 / 项目）
    struct StubHost {
        connections: Vec<ConnectionItem>,
    }

    impl EditorHost for StubHost {
        /// 测试桩没有窗口，也不该弹窗：确认一律放行（用例直接验切档结果）
        fn confirm_federated_mount(
            &self,
            _conn_id: &str,
            on_decision: Box<dyn FnOnce(bool, &mut gpui_kit::App)>,
            cx: &mut gpui_kit::App,
        ) {
            on_decision(true, cx);
        }
        fn connections(&self) -> Vec<ConnectionItem> {
            self.connections.clone()
        }
        fn project_root(&self) -> Option<PathBuf> {
            None
        }
        fn project_read_only(&self) -> bool {
            false
        }
        fn set_connection_connected(&self, _conn_id: &str, _connected: bool) {}
        fn is_connected(&self, _conn_id: &str) -> bool {
            false
        }
        fn connect_entry(&self, _conn_id: &str) -> Result<(), String> {
            Err("测试宿主未接入".to_string())
        }
        fn open_history(&self, _cx: &mut App) {}
        fn open_insight_source_column(
            &self,
            _request: crate::shared::InsightColumnRequest,
            _cx: &mut App,
        ) {
        }
        fn open_insight_result_set(
            &self,
            _request: crate::shared::AnalyzeResultSetRequest,
            _cx: &mut App,
        ) {
        }
        fn persist_result_set(
            &self,
            _request: crate::shared::PersistResultSetRequest,
            _cx: &mut App,
        ) {
        }
        fn note_result_set_materialized(
            &self,
            _notice: crate::shared::MaterializedResultSetNotice,
            _cx: &mut App,
        ) {
        }
        fn request_reveal(&self, _object: engine::ObjectRef) {}
    }

    /// 测试用的连接快照。`use_duckdb_fed` 已**不参与门控**（2026-11-27 解耦），这里固定
    /// `None` 正是在断言「门控一眼都不看它」；字段本身随 K4c 从连接模型里删掉。
    fn connection(id: &str) -> ConnectionItem {
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
            environment: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// 门控读的是**进程级登记快照**；用例并行跑，得把这一个静态串行化。
    fn registry_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn row(conn_id: &str, alias: &str, kind: AnalysisMountKind) -> AnalysisSourceRow {
        AnalysisSourceRow {
            id: format!("s_{conn_id}"),
            name: conn_id.to_string(),
            kind: AnalysisSourceKind::Conn,
            connection_id: Some(conn_id.to_string()),
            path: None,
            alias: alias.to_string(),
            mount_kind: kind,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn port(connections: Vec<ConnectionItem>) -> WorkbenchChannels {
        WorkbenchChannels {
            host: std::rc::Rc::new(StubHost { connections }),
        }
    }

    /// 没登记成加速源的连接：加速档置灰，原因**说清缺的是登记**（不是含糊的“不可用”）
    #[test]
    fn an_unregistered_connection_blocks_the_accelerated_channel() {
        let _lock = registry_lock();
        publish_aliases(&[]);
        let availability = port(vec![connection("P_a")]).availability(Some("P_a"));
        let gate = availability.for_channel(ExecChannel::Accelerated);
        assert!(!gate.available);
        assert!(
            gate.reason.unwrap_or_default().contains("还没登记"),
            "原因要说清缺的是登记"
        );
    }

    /// 登记成加速源 → 加速档可用（驱动能加速、扩展没试过不拦）；联邦档同样可用（登记过）
    #[test]
    fn a_registered_accel_source_makes_the_channel_available() {
        let _lock = registry_lock();
        publish_aliases(&[row("P_a", "rds_src", AnalysisMountKind::Accel)]);
        let availability = port(vec![connection("P_a")]).availability(Some("P_a"));
        let gate = availability.for_channel(ExecChannel::Accelerated);
        assert!(
            gate.available,
            "登记成加速源、扩展没试过、驱动能加速 → 可造：{:?}",
            gate.reason
        );
        // 联邦：单源也合法（2026-11-25 用户口径）——它就是主源 → 可用
        let federated = availability.for_channel(ExecChannel::Federated);
        assert!(
            federated.available,
            "单源联邦应当可用：{:?}",
            federated.reason
        );
    }

    /// 登记成**联邦档**的连接：联邦能用，加速档**不解锁**——两档各有各的意图，不能互相顶替
    #[test]
    fn a_federation_registration_does_not_unlock_acceleration() {
        let _lock = registry_lock();
        publish_aliases(&[row("P_a", "shop_src", AnalysisMountKind::Federation)]);
        let availability = port(vec![connection("P_a")]).availability(Some("P_a"));
        assert!(
            availability.for_channel(ExecChannel::Federated).available,
            "登记过的连接就该能当联邦源"
        );
        let gate = availability.for_channel(ExecChannel::Accelerated);
        assert!(!gate.available, "联邦档登记不该解锁加速");
        assert!(
            gate.reason.unwrap_or_default().contains("联邦源"),
            "原因要说清登记的是哪一档"
        );
    }

    /// 联邦：**登记过 / 本文档绑定（focus）即算源**（不要求已连接——源是记录组装的）
    #[test]
    fn the_federated_channel_counts_registered_or_focused_connections() {
        let _lock = registry_lock();
        // 只登记 P_a；P_b 没登记也不是 focus → 它不算源
        publish_aliases(&[row("P_a", "shop_src", AnalysisMountKind::Federation)]);
        let mut second = connection("P_b");
        second.name = "仓库".to_string();
        // 特意不连上：联邦源不需要应用先建连（DuckDB 自己 `ATTACH`）
        second.connected = false;

        // 登记过的那条就够了（哪怕另一条既没登记也没连）→ 可用
        let availability =
            port(vec![connection("P_a"), second.clone()]).availability(Some("P_a"));
        assert!(
            availability.for_channel(ExecChannel::Federated).available,
            "登记过一条就该给选（单源放开）：{:?}",
            availability.for_channel(ExecChannel::Federated).reason
        );

        // 一条都没登记，但本文档绑定了它（`focus`）→ 也算源：绑定即主源（单源联邦）
        let availability = port(vec![second]).availability(Some("P_b"));
        assert!(
            availability.for_channel(ExecChannel::Federated).available,
            "本文档绑定的连接不算源的话，单源联邦就永远点不开：{:?}",
            availability.for_channel(ExecChannel::Federated).reason
        );
    }

    /// 登记过但驱动不能挂（ClickHouse 这类还没接入的）：原因说在驱动上。
    ///
    /// 口径（2026-11-25 单源放开）：**有一条能挂就可用**，挂不上的那些只进说明；
    /// 全都挂不上才拦，并把挂不上的逐条点名。（旧断言“联邦还差一个”是 ≥2 源口径的残留）
    #[test]
    fn the_federated_channel_reports_a_driver_it_cannot_mount() {
        fn clickhouse(id: &str) -> ConnectionItem {
            let mut item = connection(id);
            item.name = "归档库".to_string();
            item.driver = "clickhouse".to_string();
            item
        }
        let _lock = registry_lock();
        publish_aliases(&[
            row("P_a", "shop_src", AnalysisMountKind::Federation),
            row("G_ch", "archive_src", AnalysisMountKind::Federation),
        ]);
        // 一条能挂 + 一条挂不上 → 联邦可用（不能被“驱动不能挂”那条拖成不可用）
        let availability =
            port(vec![connection("P_a"), clickhouse("G_ch")]).availability(Some("P_a"));
        assert!(
            availability.for_channel(ExecChannel::Federated).available,
            "有一条能挂就该放行（单源放开）"
        );
        // 只有挂不上的那些 → 拦下，原因点名驱动与连接
        let availability = port(vec![clickhouse("G_ch2")]).availability(Some("G_ch2"));
        let gate = availability.for_channel(ExecChannel::Federated);
        assert!(!gate.available, "全挂不上必须拦");
        let reason = gate.reason.unwrap_or_default();
        assert!(reason.contains("归档库"), "{reason}");
        assert!(reason.contains("驱动还不能做联邦源"), "{reason}");
    }
    /// L2（Oracle）算联邦源、**不算本地加速**：登记成加速档也只会挡在能力上，联邦档照给
    #[test]
    fn an_l2_source_counts_for_federation_but_not_for_acceleration() {
        let mut oracle = connection("G_ora");
        oracle.name = "老库".to_string();
        oracle.driver = "oracle".to_string();
        let _lock = registry_lock();
        publish_aliases(&[
            row("P_a", "shop_src", AnalysisMountKind::Federation),
            // 故意登记成加速档：能力判定要压过登记，别让「登记了就等于能用」
            row("G_ora", "rds_src", AnalysisMountKind::Accel),
        ]);

        let availability =
            port(vec![connection("P_a"), oracle]).availability(Some("G_ora"));
        assert!(
            availability.for_channel(ExecChannel::Federated).available,
            "Oracle 能做联邦源了：{:?}",
            availability.for_channel(ExecChannel::Federated).reason
        );

        let gate = availability.for_channel(ExecChannel::Accelerated);
        assert!(!gate.available, "L2 不能做本地加速");
        let reason = gate.reason.unwrap_or_default();
        assert!(reason.contains("Oracle"), "{reason}");
        assert!(reason.contains("联邦源"), "原因要说清它能干什么：{reason}");
    }

    /// 驱动类型不能加速（比如将来接的 ClickHouse）→ 原因说在类型上，不报“没登记”
    #[test]
    fn a_driver_without_acceleration_says_so() {
        let mut item = connection("P_c");
        item.driver = "clickhouse".to_string();
        let _lock = registry_lock();
        publish_aliases(&[row("P_c", "rds_src", AnalysisMountKind::Accel)]);
        let availability = port(vec![item]).availability(Some("P_c"));
        let gate = availability.for_channel(ExecChannel::Accelerated);
        assert!(!gate.available);
        assert!(gate.reason.unwrap_or_default().contains("clickhouse"));
    }

    /// 未绑定连接 / 连接不在列表：两档都要给可读原因，而不是静默不可用
    #[test]
    fn no_source_means_a_readable_reason() {
        let unbound = port(Vec::new()).availability(None);
        assert!(unbound
            .for_channel(ExecChannel::Accelerated)
            .reason
            .unwrap_or_default()
            .contains("绑定"));

        let gone = port(Vec::new()).availability(Some("P_gone"));
        assert!(gone
            .for_channel(ExecChannel::Federated)
            .reason
            .unwrap_or_default()
            .contains("不在当前列表"));
    }

    /// 源库档不在这儿回答（它看连接是否已建连，由编辑器一侧判）
    #[test]
    fn the_source_channel_is_always_reported_as_ok_here() {
        let availability = port(Vec::new()).availability(Some("P_gone"));
        assert!(availability.for_channel(ExecChannel::Source).available);
    }
}
