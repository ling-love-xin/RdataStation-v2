//! 联邦档：源清单组装、会话取用、执行与历史/说明文案。

use super::*;
use super::convert::*;

impl EngineQueryRunner {
    // ===== 联邦档（B13 第一期后半） =====

    /// 组装本次联邦的源清单（**宿主组装**：引擎不读连接库、也不解密口令）
    ///
    /// 判据只有一个：连接开了 **「DuckDB 本地加速（联邦查询直连源库）」**（`use_duckdb_fed`）
    /// ——它就是「允许分析引擎直连本连接」这个意图，同时也是**没有原生驱动的库**
    /// （Oracle 这类：应用连不上它，但 DuckDB 扫描器能）唯一的参与入口。
    ///
    /// 两个来源，一套判据：
    ///
    /// 1. **连接记录**（`DataSourceService::list()`，全局表）：**不要求已连接**，连接串现组
    ///    （`build_connection_url` 解密口令）——这是驱动类库能进来的那条路；
    /// 2. **运行态连接**（连接管理器里已建连的，含项目作用域 `P_`）：用重连用的
    ///    `url_override`（带凭据），按同样的开关过滤。
    ///
    /// 驱动能不能挂由 [`accel::AccelKind`] 回答（L2 的 Oracle 也算：它只做联邦源、不做本地加速），
    /// 认不出就如实记一句；**别名**由连接名生成（`sanitize_alias` + 重名再排），
    /// 按连接 id 排序保证稳定；**主源** = 本文档绑定的那条连接（它不在清单里时由引擎选
    /// 第一个可用源，并留下回退说明）。
    pub(crate) fn federated_plan(&self, connection: Option<&str>) -> Result<FederatedPlan, String> {
        let manager = engine::connection_manager::get_connection_manager();
        let owner = self
            .resolve_conn_id(connection)
            .ok_or_else(|| "联邦查询需要一个连接：先绑定一个，或在导航里选中它".to_string())?;

        let mut records: Vec<FedSourceRecord> = Vec::new();
        let mut notes: Vec<String> = Vec::new();

        // ① 连接记录（不要求已连接）：宿主组装连接串（解密口令是宿主的事，编辑器不碰）
        for item in self.fed.records() {
            records.push(FedSourceRecord {
                conn_id: item.conn_id,
                name: item.name,
                db_type: item.db_type,
                url: item.url,
            });
        }

        // ② 运行态连接（含项目作用域；记录里没有的补上）
        let infos = self.runtime.block_on(manager.get_all_connection_info());
        for info in infos.iter().filter(|info| info.use_duckdb_fed) {
            if records.iter().any(|record| record.conn_id == info.id) {
                continue;
            }
            let Some(url) = self.runtime_connection_string(&info.id) else {
                notes.push(format!("连接 {} 拿不到运行时连接串，未参与", info.name));
                continue;
            };
            records.push(FedSourceRecord {
                conn_id: info.id.clone(),
                name: info.name.clone(),
                db_type: info.db_type.clone(),
                url,
            });
        }

        plan_from_records(&records, &notes, &owner)
    }

    /// 联邦档执行：在联邦会话上跑（源清单为空/全挂不上都会提前给可读原因）
    /// `lineage` 随历史落库（【B5 余项】；`run()` 把队列注入的执行血缘递过来）。
    pub(crate) fn run_on_federation(
        &self,
        connection: Option<&str>,
        sql: &str,
        lineage: &str,
    ) -> Result<QueryData, String> {
        let (plan, session) = self.federated_session(connection)?;
        // 与另两档同口径：查询只取**第一段**（套窗口），写语句原样交给引擎
        let (to_run, segmented) = match window_sql(sql, crate::execution::SEGMENT_ROWS, 0) {
            Some(wrapped) => (wrapped, true),
            None => (sql.to_string(), false),
        };
        let started = std::time::Instant::now();
        let outcome = session.run(&to_run);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_federation_history(&plan, &session, sql, elapsed_ms, &outcome, Some(lineage));
        crate::history::bump();
        let result = match outcome {
            Ok(ok) => ok,
            Err(error) => {
                let explained = explain_federation_error(&session, error.to_string());
                tracing::error!(
                    target = "rds_workbench::exec",
                    channel = "federated",
                    connection = ?connection,
                    owner = %plan.owner,
                    elapsed_ms,
                    error = %explained,
                    "联邦查询执行失败"
                );
                return Err(explained);
            }
        };
        let mut data = to_data(&result, elapsed_ms, false);
        data.has_more = segmented
            && !data.columns.is_empty()
            && data.rows.len() == crate::execution::SEGMENT_ROWS;
        data.notice = federation_notice(&plan, &session);
        Ok(data)
    }

    /// 联邦档的「取下一段」（同一套窗口包装，见 `sql_service::window_sql`）
    pub(crate) fn fetch_next_on_federation(
        &self,
        connection: Option<&str>,
        sql: &str,
        offset: usize,
        limit: usize,
    ) -> Result<QueryData, String> {
        let (plan, session) = self.federated_session(connection)?;
        let wrapped =
            window_sql(sql, limit, offset).ok_or_else(|| "这份结果没有可再取的部分".to_string())?;
        let started = std::time::Instant::now();
        let outcome = session.run(&wrapped);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_federation_history(
            &plan,
            &session,
            sql,
            elapsed_ms,
            &outcome,
            Some(crate::execution::LINEAGE_SEGMENT),
        );
        crate::history::bump();
        let result =
            outcome.map_err(|error| explain_federation_error(&session, error.to_string()))?;
        let mut data = to_data(&result, elapsed_ms, false);
        data.has_more = data.rows.len() == limit;
        Ok(data)
    }

    /// 联邦档的下发（筛选 / 排序）：改写之后照跑（`notice` 由调用方给）
    pub(crate) fn run_rewritten_on_federation(
        &self,
        connection: Option<&str>,
        rewritten: &engine::sql::Rewrite,
        notice: impl FnOnce(&str) -> String,
        lineage: &str,
    ) -> Result<QueryData, String> {
        let (plan, session) = self.federated_session(connection)?;
        let started = std::time::Instant::now();
        let outcome = session.run(&rewritten.sql);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_federation_history(
            &plan,
            &session,
            &rewritten.sql,
            elapsed_ms,
            &outcome,
            Some(lineage),
        );
        crate::history::bump();
        let result =
            outcome.map_err(|error| explain_federation_error(&session, error.to_string()))?;
        let mut data = to_data(&result, elapsed_ms, false);
        data.notice = rewritten.dropped_limit.as_deref().map(notice);
        Ok(data)
    }

    /// 组装源清单 + 取（必要时建立）会话（两件事总是一起做，收口在这里）
    pub(crate) fn federated_session(
        &self,
        connection: Option<&str>,
    ) -> Result<(FederatedPlan, Arc<fed_session::FederatedSession>), String> {
        let plan = self.federated_plan(connection)?;
        let session =
            fed_session::ensure_session(&plan.owner, &plan.sources, plan.primary.as_deref())?;
        Ok((plan, session))
    }
}

/// 从源记录组装清单（**纯函数**：不碰连接库，便于逐条断言）
///
/// 口径见 [`EngineQueryRunner::federated_plan`]；`notes` 是取记录时已经攒下的实话
/// （读连接库失败 / 组装连接串失败之类），会一并带到结果区那行小字里。
pub(crate) fn plan_from_records(
    records: &[FedSourceRecord],
    notes: &[String],
    owner: &str,
) -> Result<FederatedPlan, String> {
    // 顺序不稳定（两张表合起来）：按连接 id 排序 → 同一份清单每次组装出同样的别名
    let mut ordered: Vec<&FedSourceRecord> = records.iter().collect();
    ordered.sort_by(|a, b| a.conn_id.cmp(&b.conn_id));

    let mut notes: Vec<String> = notes.to_vec();
    let mut taken: Vec<String> = Vec::new();
    let mut sources: Vec<FederatedSource> = Vec::new();
    let mut primary: Option<String> = None;
    let mut owner_seen = false;

    // 同一个 conn_id 只算一次（记录与运行态都出现时）
    let mut seen: Vec<&str> = Vec::new();
    for record in ordered {
        if record.conn_id == owner {
            owner_seen = true;
        }
        if seen.contains(&record.conn_id.as_str()) {
            continue;
        }
        seen.push(&record.conn_id);
        if let Err(reason) = accel::AccelKind::from_db_type(&record.db_type) {
            notes.push(format!("连接 {} 没参与：{reason}", record.name));
            continue;
        }
        let alias = fed_registry::unique_alias(&fed_registry::sanitize_alias(&record.name), &taken);
        taken.push(alias.clone());
        // 凭据随连接串进 `ATTACH`（DuckDB 的扫描器不认 Secret，见 `accel::AccelSource::new`）；
        // L2 是拆成会话级 Secret（`registry::oracle_secret_from_url`）。
        //
        // 组装不出来的**不阻断整份清单**（与上面“认不出的驱动”同一口径）：一条连接的串缺件
        // （凭据 / 服务名）不该把其余源一起拖下水；结果区那行小字会点名它是怎么回事。
        let source =
            match FederatedSource::new(&record.conn_id, &alias, &record.db_type, &record.url) {
                Ok(source) => source,
                Err(reason) => {
                    notes.push(format!("连接 {} 没参与：{reason}", record.name));
                    continue;
                }
            };
        if record.conn_id == owner {
            primary = Some(alias);
        }
        sources.push(source);
    }

    if sources.is_empty() {
        return Err(
            "还没有可用作联邦源的连接：在连接对话框 → 高级里开启「DuckDB 本地加速（联邦查询直连源库）」"
                .to_string(),
        );
    }
    // 联邦与本地加速的区别就是“跨源”：只有一个源时两者是同一件事（门控也是这么挡的）
    if sources.len() < 2 {
        return Err(format!(
            "联邦查询至少需要两个源（现在只有 {}）：把另一个连接也开启「DuckDB 本地加速」",
            sources[0].alias
        ));
    }
    if !owner_seen {
        notes.push("本文档的连接未开启「DuckDB 本地加速」，没有作为联邦源参与".to_string());
    } else if primary.is_none() {
        notes.push(format!(
            "本文档的连接（{owner}）未能作为联邦源参与（见上一条说明）"
        ));
    }

    Ok(FederatedPlan {
        owner: owner.to_string(),
        sources,
        primary,
        notes,
    })
}

/// 联邦档的执行也进同一条历史流水（与另两档一致）
///
/// `db_type` 取**主源**的库类型（历史面板据此拼出 `MYSQL·联邦`）、`sources` 取本次
/// **真正挂上**的源别名：用户重看这条历史时要能对上是哪几个源。
pub(crate) fn record_federation_history(
    plan: &FederatedPlan,
    session: &fed_session::FederatedSession,
    sql: &str,
    elapsed_ms: u64,
    outcome: &Result<QueryResult, shared::error::CoreError>,
    lineage: Option<&str>,
) {
    let snapshot = session.snapshot();
    let ready: Vec<String> = snapshot
        .sources
        .iter()
        .filter(|entry| entry.is_ready())
        .map(|entry| entry.alias().to_string())
        .collect();
    let sources = (!ready.is_empty()).then(|| ready.join(", "));
    let db_type = snapshot
        .primary
        .as_deref()
        .and_then(|primary| plan.sources.iter().find(|source| source.alias == primary))
        .map(|source| source.kind.label().to_string());

    let entry = match outcome {
        Ok(result) => SqlHistoryEntry {
            conn_id: Some(plan.owner.clone()),
            db_type,
            elapsed_ms,
            success: true,
            error_message: None,
            rows_returned: Some(result.total_rows() as u64),
            rows_affected: None,
            channel: Some(ExecChannel::Federated.code().to_string()),
            sources,
            lineage: lineage.map(str::to_string),
        },
        Err(error) => SqlHistoryEntry {
            conn_id: Some(plan.owner.clone()),
            db_type,
            elapsed_ms,
            success: false,
            error_message: Some(error.to_string()),
            rows_returned: None,
            rows_affected: None,
            channel: Some(ExecChannel::Federated.code().to_string()),
            sources,
            lineage: lineage.map(str::to_string),
        },
    };
    if let Err(error) = history_store::save_sql_history(sql, &entry) {
        tracing::error!(error = %error, "联邦执行未记入历史");
    }
}

/// 结果区那行小字（联邦档专属）：挂了哪些源 + 写法提示 + 没挂上的原因
///
/// “写法提示”是**临时的**：源清单浮层（T1.6）上线后它收进浮层底部，这里只留状态。
pub(crate) fn federation_notice(
    plan: &FederatedPlan,
    session: &fed_session::FederatedSession,
) -> Option<String> {
    let snapshot = session.snapshot();
    let ready: Vec<String> = snapshot
        .sources
        .iter()
        .filter(|entry| entry.is_ready())
        .map(|entry| entry.alias().to_string())
        .collect();
    // L2（凭据走 Secret 的那类，如 Oracle）的限定名是**两段**：`别名.表`
    // （真机台账 §2.1：表挂在 `main` schema 上，写三段会报 schema 不存在）
    let l2: Vec<String> = snapshot
        .sources
        .iter()
        .filter(|entry| entry.is_ready() && entry.source.kind.needs_secret())
        .map(|entry| entry.alias().to_string())
        .collect();

    let mut parts: Vec<String> = Vec::new();
    if !ready.is_empty() {
        parts.push(format!("联邦源 {}", ready.join("、")));
    }
    parts.push(match l2.is_empty() {
        true => "跨源请写 别名.schema.表".to_string(),
        false => format!(
            "跨源请写 别名.schema.表（{} 写两段：别名.表，不写 schema）",
            l2.join("、")
        ),
    });
    parts.extend(plan.notes.iter().cloned());
    for entry in &snapshot.sources {
        if let MountState::Failed(reason) = &entry.state {
            parts.push(format!("源 {} 未挂上：{reason}", entry.alias()));
        }
    }
    if let Some(note) = &snapshot.primary_note {
        parts.push(note.clone());
    }
    Some(parts.join(" · "))
}

/// 联邦档的失败原因：点得到源就点名源（D6：错误归属）
///
/// 典型场景：用户写了 `mysql_src.…` 而这个源根本没挂上，DuckDB 只会说
/// “Catalog \"mysql_src\" does not exist”；把挂载失败的原因缀在后面，用户才知道去哪儿修。
///
/// 只认两种提法（引号里的 catalog 名 / `别名.` 限定引用）：纯子串匹配会在 `odbc` 里
/// 匹到别名 `db`，把一句无关的话缀在错误后面——错误归属宁可少说也不能说错。
pub(crate) fn explain_federation_error(session: &fed_session::FederatedSession, error: String) -> String {
    let snapshot = session.snapshot();
    for entry in &snapshot.sources {
        if let MountState::Failed(reason) = &entry.state {
            let quoted = format!("\"{}\"", entry.alias());
            let qualified = format!("{}.", entry.alias());
            if error.contains(&quoted) || error.contains(&qualified) {
                return format!("{error}（源 {} 未挂上：{reason}）", entry.alias());
            }
        }
    }
    error
}
