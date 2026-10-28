//! 联邦源清单（「源清单 ▾」浮层）的数据侧
//!
//! 编辑器不认识连接、也不碰 DuckDB：源清单是**宿主**的事实（谁挂着、挂上没有、几张表），
//! 编辑器只做两件事——把口述的形状画出来（[`menu_entries`] 是纯函数，可逐条断言），
//! 以及把动作发出去（走 `QueryRunner` 的 [`crate::execution::SourceAction`]）。
//!
//! ## 为什么要有独立的端口
//!
//! 快照要被**渲染路径**读（工具栏每帧画那行小字 / 浮层），所以宿主注入的端口只做**内存读**
//! （[`SourcesPort::snapshot`]）；真正会做 I/O 的动作（重挂 = `DETACH` + `ATTACH`）走执行器
//! 的旁路线程，与「重新挂载源库」同一条路。
//!
//! ## 名字与用词（与原型 §2 一致）
//!
//! - 主源：未限定表名在它里面解析（`●` 标的那个）；
//! - 状态：`✅ 可用（N 张表）` / `⚠️ 失败：<原话>`——**失败的行不消失**；
//! - 别名：跨源查询里写的那个名字（`<别名>.<schema>.<表>`；L2 源是两段名）。

/// 一个源的挂载状态（界面上就是“可用 / 失败 + 原话”两态）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceState {
    /// 挂上了（`tables` = 挂载时的表数量，纯展示）
    Ready { tables: usize },
    /// 挂不上 / 挂了又掉（原话在这里，不翻译不改写；口令已由引擎侧脱敏）
    Failed(String),
}

impl SourceState {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    /// 行尾状态文案（纯函数）
    pub fn text(&self) -> String {
        match self {
            Self::Ready { tables } => format!("可用 · {tables} 张表"),
            Self::Failed(reason) => format!("失败：{reason}"),
        }
    }

    /// 状态点（原型 §2 的 `✅` / `⚠️`）
    pub fn mark(&self) -> &'static str {
        if self.is_ready() { "✅" } else { "⚠️" }
    }
}

/// 源清单里的一行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRow {
    /// SQL 里的别名（跨源查询写它）
    pub alias: String,
    /// 库类型文案（`MySQL` / `PostgreSQL` / `SQLite` / `DuckDB` …）
    pub kind_label: String,
    pub state: SourceState,
    /// 主源（未限定名的解析者）
    pub primary: bool,
}

impl SourceRow {
    /// 行文案：`✅ mysql_src · MySQL · 可用 · 42 张表 · 主源`
    pub fn text(&self) -> String {
        let mut parts = vec![
            format!("{} {}", self.state.mark(), self.alias),
            self.kind_label.clone(),
            self.state.text(),
        ];
        if self.primary {
            parts.push("主源".to_string());
        }
        parts.join(" · ")
    }
}

/// 一次快照（界面读这个：**内存快照**，渲染路径可调）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcesSnapshot {
    pub sources: Vec<SourceRow>,
    /// 主源别名（`None` = 当前没有可用源）
    pub primary: Option<String>,
    /// 回退说明 / 汇总说明（界面上要说出来，不能悄悄换）
    pub note: Option<String>,
}

impl SourcesSnapshot {
    pub fn ready_count(&self) -> usize {
        self.sources.iter().filter(|row| row.state.is_ready()).count()
    }
}

/// 宿主注入的源清单端口（只有**内存读**；动作用 [`crate::execution::SourceAction`]）
pub trait SourcesPort: Send + Sync + 'static {
    /// 这个连接上的联邦会话快照；`None` = 还没有会话（还没执行过联邦查询）
    fn snapshot(&self, conn_id: &str) -> Option<SourcesSnapshot>;
}

/// 界面上持有的句柄（宿主注入；与 `ChannelsHandle` 同一种做法）
pub type SourcesHandle = std::rc::Rc<dyn SourcesPort>;

/// 源清单浮层里的一项（纯函数算出来，视图只负责画）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceMenuEntry {
    /// 纯信息行（不可点）：主源说明 / 空态说明
    Info(String),
    /// 一个源（不可点，只是“清单”）
    Source(SourceRow),
    /// 一项动作
    Action {
        label: String,
        action: crate::execution::SourceAction,
    },
}

/// 「源清单 ▾」浮层的内容（**纯函数**）
///
/// - 没有会话（还没执行过联邦查询）：告诉用户怎么让它有内容，而不是一个空浮层；
/// - 有一行以上源：先给汇总（主源 / 可用数），再逐源列出（失败行不消失），最后是动作；
/// - 动作只在与“动作对得上”的时候给：没有失败的源也给重挂（可能就是想刷新表清单），
///   但**设为主源只给可用的源**（不可用的设不了，引擎也会拒）。
pub fn menu_entries(snapshot: Option<&SourcesSnapshot>) -> Vec<SourceMenuEntry> {
    let Some(snapshot) = snapshot else {
        return vec![SourceMenuEntry::Info(
            "还没有联邦会话：执行一次联邦查询后再打开这里".to_string(),
        )];
    };

    let mut entries = vec![SourceMenuEntry::Info(match snapshot.primary.as_deref() {
        Some(primary) => format!(
            "联邦 · {} 个源 · 主源 {primary}",
            snapshot.sources.len()
        ),
        None => format!("联邦 · {} 个源 · 当前没有可用源", snapshot.sources.len()),
    })];
    if let Some(note) = &snapshot.note {
        entries.push(SourceMenuEntry::Info(note.clone()));
    }
    for row in &snapshot.sources {
        entries.push(SourceMenuEntry::Source(row.clone()));
    }

    if snapshot.sources.is_empty() {
        return entries;
    }

    entries.push(SourceMenuEntry::Action {
        label: "重新挂载全部源（刷新表清单）".to_string(),
        action: crate::execution::SourceAction::RefreshAll,
    });
    for row in snapshot.sources.iter().filter(|row| row.state.is_ready()) {
        entries.push(SourceMenuEntry::Action {
            label: format!("设为主源：{}", row.alias),
            action: crate::execution::SourceAction::SetPrimary {
                alias: row.alias.clone(),
            },
        });
    }
    for row in snapshot.sources.iter() {
        entries.push(SourceMenuEntry::Action {
            label: format!("重新挂载：{}", row.alias),
            action: crate::execution::SourceAction::RefreshSource {
                alias: row.alias.clone(),
            },
        });
    }

    entries
}

/// 「源清单 ▾」按钮上的那行小字（有会话时缀上源数与主源；没有会话就不缀）
pub fn button_label(snapshot: Option<&SourcesSnapshot>) -> String {
    match snapshot {
        Some(snapshot) => match snapshot.primary.as_deref() {
            Some(primary) => format!("源清单：{} 源 · 主源 {primary} ▾", snapshot.sources.len()),
            None => format!("源清单：{} 源 ▾", snapshot.sources.len()),
        },
        None => "源清单 ▾".to_string(),
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        SourceMenuEntry, SourceRow, SourceState, SourcesSnapshot, button_label, menu_entries,
    };
    use crate::execution::SourceAction;

    fn ready(alias: &str, tables: usize, primary: bool) -> SourceRow {
        SourceRow {
            alias: alias.to_string(),
            kind_label: "MySQL".to_string(),
            state: SourceState::Ready { tables },
            primary,
        }
    }

    fn failed(alias: &str) -> SourceRow {
        SourceRow {
            alias: alias.to_string(),
            kind_label: "Oracle".to_string(),
            state: SourceState::Failed("源 oracle_prod：ORA-12541 无监听程序".to_string()),
            primary: false,
        }
    }

    #[test]
    fn a_row_says_state_and_primary() {
        assert_eq!(
            ready("mysql_src", 42, true).text(),
            "✅ mysql_src · MySQL · 可用 · 42 张表 · 主源"
        );
        assert_eq!(
            ready("pg_warehouse", 3, false).text(),
            "✅ pg_warehouse · MySQL · 可用 · 3 张表"
        );
        let broken = failed("oracle_prod");
        assert!(
            broken.text().contains("ORA-12541"),
            "失败行要带原话：{}",
            broken.text()
        );
        assert!(broken.text().starts_with("⚠️"));
    }

    #[test]
    fn no_session_says_how_to_get_one() {
        let entries = menu_entries(None);
        assert_eq!(entries.len(), 1, "{entries:?}");
        let SourceMenuEntry::Info(text) = &entries[0] else {
            panic!("空态该是一条说明：{entries:?}");
        };
        assert!(text.contains("执行一次"), "{text}");
        assert_eq!(button_label(None), "源清单 ▾");
    }

    #[test]
    fn a_snapshot_lists_every_source_and_the_actions() {
        let snapshot = SourcesSnapshot {
            sources: vec![ready("mysql_src", 42, true), failed("oracle_prod")],
            primary: Some("mysql_src".to_string()),
            note: Some("主源 oracle_prod 不可用，已改用 mysql_src".to_string()),
        };
        let entries = menu_entries(Some(&snapshot));

        // 汇总 + 回退说明 + 两行源
        assert!(
            matches!(&entries[0], SourceMenuEntry::Info(text) if text.contains("主源 mysql_src")),
            "{entries:?}"
        );
        assert!(
            entries.iter().any(|e| matches!(e, SourceMenuEntry::Info(text) if text.contains("已改用"))),
            "回退说明不能吞：{entries:?}"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|e| matches!(e, SourceMenuEntry::Source(_)))
                .count(),
            2,
            "失败的行不消失：{entries:?}"
        );

        // 动作：重挂全部 + 只有一个可用源可设为主源 + 逐源重挂（含失败那个）
        let actions: Vec<&SourceAction> = entries
            .iter()
            .filter_map(|entry| match entry {
                SourceMenuEntry::Action { action, .. } => Some(action),
                _ => None,
            })
            .collect();
        assert!(
            actions.contains(&&SourceAction::RefreshAll),
            "{actions:?}"
        );
        assert!(
            actions.contains(&&SourceAction::SetPrimary {
                alias: "mysql_src".to_string()
            }),
            "{actions:?}"
        );
        assert!(
            !actions.contains(&&SourceAction::SetPrimary {
                alias: "oracle_prod".to_string()
            }),
            "不可用的源不能设为主源：{actions:?}"
        );
        assert!(
            actions.contains(&&SourceAction::RefreshSource {
                alias: "oracle_prod".to_string()
            }),
            "失败的行也要能重挂（修好之后重试）：{actions:?}"
        );
    }

    #[test]
    fn the_button_label_carries_the_source_count() {
        let snapshot = SourcesSnapshot {
            sources: vec![ready("a", 1, true), ready("b", 2, false)],
            primary: Some("a".to_string()),
            note: None,
        };
        assert_eq!(button_label(Some(&snapshot)), "源清单：2 源 · 主源 a ▾");
        assert_eq!(snapshot.ready_count(), 2);
    }
}
