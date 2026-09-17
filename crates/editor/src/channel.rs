//! 执行通道（B13）：源库直连 / DuckDB 本地加速 / 联邦查询
//!
//! **三者互斥**（原型 §5.7）：它是**文档级**属性（与连接绑定同类，随会话持久化），与执行族
//! （“执行什么”）正交。UI 载体 = 工具栏右侧「执行位置 ▾」。
//!
//! ## 通道决定能力边界（不能只在按钮上体现）
//!
//! | 维度 | 源库直连 | 本地加速 / 联邦 |
//! | --- | --- | --- |
//! | 作用**源库对象**的写语句 / DDL | 允许（受连接只读策略） | **禁止**（本地副本只读） |
//! | 事务 | 支持 | 不支持 |
//! | 数据新鲜度 | 实时 | ATTACH 时的**快照**（徽标与状态栏要显式提示） |
//!
//! ## 门控如实
//!
//! 「本地加速 / 联邦」要 DuckDB 侧真就绪（扩展 + `ATTACH` / Secret）才可用；**没就绪就不给选**，
//! 并在行尾写原因（原型：置灰 + 不可用原因）。这条事实来自宿主注入的 [`ChannelsPort`]——
//! 编辑器不猜 DuckDB 的状态。

use engine::sql::{SqlDialect, SqlEngine, SqlStatementType};

/// 执行通道（三者互斥，默认源库直连）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecChannel {
    /// 源库直连（实时数据；写语句与事务都在这档）
    #[default]
    Source,
    /// DuckDB 本地加速（`ATTACH` 源库副本；只读快照）
    Accelerated,
    /// 联邦查询（DuckDB + 多个外部源）
    Federated,
}

impl ExecChannel {
    pub const ALL: [ExecChannel; 3] = [Self::Source, Self::Accelerated, Self::Federated];

    /// 界面文案（菜单项 / 状态栏 / 徽标共用同一套词）
    pub fn label(self) -> &'static str {
        match self {
            Self::Source => "源库",
            Self::Accelerated => "本地加速",
            Self::Federated => "联邦",
        }
    }

    /// 结果集标签上的徽标短码（原型 §5.4：`源库` / `加速` / `联邦`）
    ///
    /// 与 [`Self::label`] 不同：徽标挤在标签里，用两字短码。
    pub fn badge(self) -> &'static str {
        match self {
            Self::Source => "源库",
            Self::Accelerated => "加速",
            Self::Federated => "联邦",
        }
    }

    /// 作用**源库对象**的写语句 / DDL 允许吗（本地副本只读）
    pub fn allows_source_writes(self) -> bool {
        matches!(self, Self::Source)
    }

    /// 事务允许吗（加速 / 联邦没有事务语义）
    pub fn allows_transactions(self) -> bool {
        matches!(self, Self::Source)
    }

    /// 数据是不是 `ATTACH` 时的快照（界面要显式提示新鲜度）
    pub fn is_snapshot(self) -> bool {
        !matches!(self, Self::Source)
    }

    /// 拒绝写语句时给用户的话（原型 §5.7 的口径）
    pub fn write_refusal(self) -> &'static str {
        self.label()
    }

    /// 从持久化的短码还原（认不出回源库——不假装记住了别的）
    pub fn from_code(code: &str) -> Self {
        match code {
            "accelerated" => Self::Accelerated,
            "federated" => Self::Federated,
            _ => Self::Source,
        }
    }

    /// 持久化用的短码
    pub fn code(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Accelerated => "accelerated",
            Self::Federated => "federated",
        }
    }
}

/// 一个通道的可用性（门控结果）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelAvailability {
    pub available: bool,
    /// 不可用的原因（原型：置灰 + 行尾小字给原因）——可用时是 `None`
    pub reason: Option<String>,
}

impl ChannelAvailability {
    pub fn ok() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    pub fn blocked(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
        }
    }
}

/// 「本地加速 / 联邦」两档的门控结果（源库档的可用性看连接是否已建立）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChannelAvailabilitySet {
    pub accelerated: ChannelAvailability,
    pub federated: ChannelAvailability,
}

impl Default for ChannelAvailability {
    fn default() -> Self {
        Self::blocked("尚未接入")
    }
}

impl ChannelAvailabilitySet {
    /// 全部不可用（宿主还没接通道能力时的诚实状态）
    pub fn blocked(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            accelerated: ChannelAvailability::blocked(reason.clone()),
            federated: ChannelAvailability::blocked(reason),
        }
    }

    pub fn for_channel(&self, channel: ExecChannel) -> ChannelAvailability {
        match channel {
            // 源库档由“连接是否已建立”决定，这里不替它回答
            ExecChannel::Source => ChannelAvailability::ok(),
            ExecChannel::Accelerated => self.accelerated.clone(),
            ExecChannel::Federated => self.federated.clone(),
        }
    }
}

/// 通道门控端口（宿主注入）
///
/// **渲染路径会读它**（工具栏每帧都要画菜单）：实现必须是内存快照或纯计算，不做 I/O。
pub trait ChannelsPort: 'static {
    /// 某连接上「本地加速 / 联邦」的可用性
    ///
    /// `conn_id` = 文档绑定的连接（`None` = 未绑定 → 按“没有可加速的源”回答）。
    fn availability(&self, conn_id: Option<&str>) -> ChannelAvailabilitySet;
}

/// 端口句柄（与 `QueryRunner` / `SessionStore` 同一模式：宿主注入、可缺省）
pub type ChannelsHandle = std::rc::Rc<dyn ChannelsPort>;

/// 通道菜单里的一项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelMenuItem {
    /// 菜单上的文案（不可用时把原因缀在行尾）
    pub label: String,
    pub channel: ExecChannel,
    pub available: bool,
    /// 这项是不是当前选中的通道（菜单里打勾）
    pub current: bool,
}

/// 生成「执行位置 ▾」的菜单项（**纯函数**：哪项能点、为什么不能点都在这儿定）
///
/// `connected` = 文档绑定的连接是否已建立（源库档的门控真值）。
pub fn menu_items(
    current: ExecChannel,
    connected: bool,
    availability: &ChannelAvailabilitySet,
) -> Vec<ChannelMenuItem> {
    ExecChannel::ALL
        .into_iter()
        .map(|channel| {
            let (available, reason) = match channel {
                ExecChannel::Source if !connected => {
                    (false, Some("连接未建立".to_string()))
                }
                ExecChannel::Source => (true, None),
                other => {
                    let gate = availability.for_channel(other);
                    (gate.available, gate.reason)
                }
            };
            let label = match (&reason, available) {
                (Some(reason), _) => format!("{}（{reason}）", channel.label()),
                (None, _) => channel.label().to_string(),
            };
            ChannelMenuItem {
                label,
                channel,
                available,
                current: channel == current,
            }
        })
        .collect()
}

/// 状态栏那一段文案（原型 §2.5：`通道 源库`；非源库档要显式带“快照”提示）
///
/// 新鲜度是**能力边界**（原型 §5.7 那张表）：加速 / 联邦看到的是 `ATTACH` 那一刻的数据，
/// 界面不提示就等于让用户拿旧数据当实时数据用。
pub fn status_text(channel: ExecChannel) -> String {
    if channel.is_snapshot() {
        format!("通道 {}（快照）", channel.label())
    } else {
        format!("通道 {}", channel.label())
    }
}

/// 切通道后的那一行提示（原型 §5.7 规则 2：“通道已切换，旧结果来自 <旧通道>”）
///
/// 只回答“为什么这份结果是灰的”；没有任何旧结果时要提示吗？不要——切换本身已经写在
/// 状态栏与工具栏上了，多一句只是噪音（由调用方决定挂不挂）。
pub fn stale_notice(result_channel: ExecChannel, current: ExecChannel) -> String {
    format!(
        "通道已切换，旧结果来自{}（重新执行即取{}数据）",
        result_channel.label(),
        current.label()
    )
}

/// 这条语句在指定通道上允许执行吗（不允许就给**可读原因**）
///
/// 判定用引擎的语句类型（`SqlEngine::parse_and_route`，Ansi 方言足够区分 DML/DDL）：
/// 作用源库对象的写语句在加速 / 联邦通道上**直接拒绝**（原型 §5.7：本地副本不可写，
/// 请切回源库）。认不出类型就放行——那是驱动该报的错，编辑器不越位。
pub fn statement_allowed(channel: ExecChannel, sql: &str) -> Result<(), String> {
    if channel.allows_source_writes() {
        return Ok(());
    }
    let (kind, _) = SqlEngine::parse_and_route(sql, SqlDialect::Ansi);
    match kind {
        SqlStatementType::Insert
        | SqlStatementType::Update
        | SqlStatementType::Delete
        | SqlStatementType::Ddl => Err(format!(
            "{}通道不能写源库（本地副本只读），请切回源库再试",
            channel.write_refusal()
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChannelAvailability, ChannelAvailabilitySet, ExecChannel, menu_items, stale_notice,
        statement_allowed, status_text,
    };

    #[test]
    fn channels_are_mutually_exclusive_with_real_capabilities() {
        assert_eq!(ExecChannel::ALL.len(), 3);
        assert!(ExecChannel::Source.allows_source_writes());
        assert!(ExecChannel::Source.allows_transactions());
        assert!(!ExecChannel::Source.is_snapshot());

        for channel in [ExecChannel::Accelerated, ExecChannel::Federated] {
            assert!(!channel.allows_source_writes(), "{} 不能写源库", channel.label());
            assert!(!channel.allows_transactions());
            assert!(channel.is_snapshot(), "是 ATTACH 快照，要提示新鲜度");
        }
    }

    #[test]
    fn labels_and_codes_round_trip() {
        for channel in ExecChannel::ALL {
            assert_eq!(ExecChannel::from_code(channel.code()), channel);
        }
        assert_eq!(ExecChannel::from_code("nonsense"), ExecChannel::Source);
        assert_eq!(ExecChannel::Accelerated.badge(), "加速");
        assert_eq!(ExecChannel::Federated.badge(), "联邦");
    }

    #[test]
    fn menu_items_grey_out_with_reasons() {
        // 门控：加速开着（DuckDB 就绪），联邦说清原因
        let availability = ChannelAvailabilitySet {
            accelerated: ChannelAvailability::ok(),
            federated: ChannelAvailability::blocked("尚未注册外部源"),
        };
        let items = menu_items(ExecChannel::Source, true, &availability);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label, "源库", "可用项不带原因");
        assert!(items[0].available && items[0].current);
        assert_eq!(items[1].label, "本地加速");
        assert!(items[1].available);
        assert_eq!(items[2].label, "联邦（尚未注册外部源）");
        assert!(!items[2].available, "不可用项要置灰");
        assert!(!items[2].current, "当前选中的不是它");
    }

    #[test]
    fn the_source_channel_needs_a_live_connection() {
        let availability = ChannelAvailabilitySet::blocked("尚未接入");
        let items = menu_items(ExecChannel::Accelerated, false, &availability);
        assert_eq!(items[0].label, "源库（连接未建立）");
        assert!(!items[0].available);
        assert!(items[1].current, "当前是加速档（就算它暂时不可用也如实标着）");
        assert!(!items[1].available);
    }

    #[test]
    fn the_status_segment_names_the_snapshot_channels() {
        assert_eq!(status_text(ExecChannel::Source), "通道 源库");
        assert!(
            status_text(ExecChannel::Accelerated).contains("快照"),
            "加速看到的是 ATTACH 那一刻的数据，必须显式说：{}",
            status_text(ExecChannel::Accelerated)
        );
        assert!(status_text(ExecChannel::Federated).contains("快照"));
    }

    /// 切通道后顶部那一行提示要说清**旧结果来自哪档**（否则“灰了”等于没解释）
    #[test]
    fn the_stale_notice_names_the_old_channel() {
        let text = stale_notice(ExecChannel::Source, ExecChannel::Accelerated);
        assert!(text.contains("旧结果来自源库"), "{text}");
        assert!(text.contains("本地加速"), "提示里要带新通道，用户才知道现在是哪档：{text}");
        assert!(text.contains("通道已切换"), "{text}");
    }

    #[test]
    fn source_writes_are_refused_on_snapshot_channels() {
        // 源库档：写语句随便发（受连接只读策略约束）
        assert!(statement_allowed(ExecChannel::Source, "INSERT INTO t VALUES (1)").is_ok());

        for channel in [ExecChannel::Accelerated, ExecChannel::Federated] {
            let refused = statement_allowed(channel, "UPDATE t SET a = 1").expect_err("要拒绝");
            assert!(refused.contains("请切回源库"), "{refused}");
            assert!(
                statement_allowed(channel, "DELETE FROM t").is_err(),
                "DELETE 也要拒"
            );
            assert!(
                statement_allowed(channel, "CREATE TABLE t (a INT)").is_err(),
                "DDL 也要拒"
            );
            // 读语句与认不出的都放行（后者交给驱动报错）
            assert!(statement_allowed(channel, "SELECT * FROM t").is_ok());
            assert!(statement_allowed(channel, "WITH x AS (SELECT 1) SELECT * FROM x").is_ok());
            assert!(statement_allowed(channel, "这不是 SQL").is_ok());
        }
    }
}
