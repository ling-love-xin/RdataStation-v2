//! 联邦源清单端口的工作台实现（T1.6）：把引擎的会话快照翻译成界面读得懂的行
//!
//! ## 职责边界
//!
//! 编辑器问一件事：“这个连接上的联邦源现在是什么样？”（`editor::sources::SourcesPort`）。
//! 真值在引擎的联邦会话里（`federation::session::snapshot_for`）；这里只做**形状翻译**：
//! 挂载状态、表数量、主源是哪个、回退说明——**纯内存读**，渲染路径可调（不做 I/O）。
//!
//! ## 动作不在这里
//!
//! 重挂 / 换主源会做 I/O（`DETACH` + `ATTACH`），它们走执行器的旁路线程
//! （`workbench::services::editor_exec` 的 `refresh_sources` / `set_federated_primary`），
//! 与「重新挂载源库」同一条路——端口只管“看”。

use editor::sources::{SourceRow, SourceState, SourcesPort, SourcesSnapshot};
use editor::shared::EditorShared;

use engine::duckdb::federation::registry::{MountState, SessionSnapshot};
use engine::duckdb::federation::session as fed_session;

/// 工作台实现：联邦会话快照（读引擎的内存快照，不做 I/O）
struct WorkbenchSources;

impl SourcesPort for WorkbenchSources {
    fn snapshot(&self, conn_id: &str) -> Option<SourcesSnapshot> {
        fed_session::snapshot_for(conn_id).map(translate)
    }
}

/// 引擎快照 → 界面快照（**纯函数**：可逐条断言）
fn translate(snapshot: SessionSnapshot) -> SourcesSnapshot {
    SourcesSnapshot {
        sources: snapshot
            .sources
            .iter()
            .map(|mounted| SourceRow {
                alias: mounted.alias().to_string(),
                kind_label: mounted.source.kind.label().to_string(),
                state: match &mounted.state {
                    MountState::Ready { tables } => SourceState::Ready { tables: *tables },
                    // 原话照搬（引擎侧已经把口令抹掉了，见 `accel::scrub_credentials`）
                    MountState::Failed(reason) => SourceState::Failed(reason.clone()),
                },
                primary: snapshot.primary.as_deref() == Some(mounted.alias()),
            })
            .collect(),
        primary: snapshot.primary.clone(),
        note: snapshot.primary_note.clone(),
    }
}

/// 把源清单端口接到编辑器共享状态上（**启动装配调用一次**）
pub fn attach(shared: &EditorShared) {
    shared.attach_sources(std::rc::Rc::new(WorkbenchSources));
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::translate;
    use engine::duckdb::accel::AccelKind;
    use engine::duckdb::federation::registry::{
        FederatedSource, MountState, MountedSource, SessionSnapshot,
    };

    fn mounted(alias: &str, state: MountState) -> MountedSource {
        MountedSource {
            source: FederatedSource {
                conn_id: format!("G_{alias}"),
                alias: alias.to_string(),
                kind: AccelKind::MySql,
                connection_string: "mysql://root:pw@h:3306/db".to_string(),
            },
            state,
        }
    }

    /// 引擎快照 → 界面行：主源标上、失败行带原话、表数量照搬
    #[test]
    fn the_engine_snapshot_becomes_rows_the_ui_can_draw() {
        let snapshot = SessionSnapshot {
            primary: Some("mysql_src".to_string()),
            sources: vec![
                mounted("mysql_src", MountState::Ready { tables: 42 }),
                mounted(
                    "oracle_prod",
                    MountState::Failed("源 oracle_prod：ORA-12541 无监听程序".to_string()),
                ),
            ],
            primary_note: Some("主源 oracle_prod 不可用，已改用 mysql_src".to_string()),
        };
        let ui = translate(snapshot);

        assert_eq!(ui.sources.len(), 2, "失败的行不消失：{:?}", ui.sources);
        assert!(ui.sources[0].primary, "主源要标上");
        assert!(!ui.sources[1].primary);
        assert_eq!(ui.sources[0].kind_label, "MySQL");
        assert_eq!(
            ui.sources[0].state,
            super::SourceState::Ready { tables: 42 }
        );
        assert!(
            ui.sources[1].state.text().contains("ORA-12541"),
            "失败原因原话照搬：{:?}",
            ui.sources[1].state
        );
        assert_eq!(
            ui.note.as_deref(),
            Some("主源 oracle_prod 不可用，已改用 mysql_src"),
            "回退说明不能丢"
        );
        assert_eq!(ui.ready_count(), 1);
    }
}
