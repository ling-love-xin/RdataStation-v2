//! 洞察服务门面 · 多列与快照（自 service/mod.rs 拆分，纯位移）

use super::*;

impl InsightService {
    // ==================== 多列分析（Phase 3.2 / 3.3） ====================

    /// 「多列」Tab 的数据：**真实列元数据** + `category = multi` 的规则清单。
    ///
    /// 列清单与表探查同源（同一个 `get_temp_table_profile`）：v1 的 `availableColumns`
    /// 恒空是「多列分析从未跑通」的根因，这里不再另存一份列清单。
    pub fn multi_column_view(
        project_root: Option<&Path>,
        temp_table: &str,
        table_name: &str,
    ) -> Result<MultiColumnView, CoreError> {
        let profile = crate::insight_engine::get_temp_table_profile(temp_table)?;
        let rules = Self::list_multi_rules(project_root)?;
        Ok(MultiColumnView::from_profile(&profile, table_name, rules))
    }

    /// `category = multi` 的规则 → 视图模型（不可解析的跳过：宁少不假）
    pub fn list_multi_rules(project_root: Option<&Path>) -> Result<Vec<MultiRuleView>, CoreError> {
        let raw = Self::list_insight_rules(project_root, Some("multi"))?;
        Ok(raw
            .iter()
            .filter_map(MultiRuleView::from_rule_json)
            .collect())
    }

    /// 跑一条多列规则：参数拼装 → 执行（已持连接）→ 结果与门控转视图模型。
    ///
    /// 阻塞（抢 DuckDB 全局锁与并发配额），调用方负责放后台。
    pub fn run_multi_rule(
        project_root: Option<&Path>,
        temp_table: &str,
        rule_id: &str,
        columns: &[String],
    ) -> Result<(MultiResultView, Vec<QualityNote>), CoreError> {
        let parameters = {
            let registry = crate::registry_for(project_root);
            let guard = registry.read().map_err(|e| {
                CoreError::common(CommonError::General(format!("规则注册表锁定失败：{}", e)))
            })?;
            let rule = guard.get(rule_id).ok_or_else(|| {
                CoreError::common(CommonError::General(format!("规则 '{}' 不存在", rule_id)))
            })?;
            rule_params(&rule.query.parameters, temp_table, columns)?
        };

        let duckdb = crate::insight_engine::get_or_create_duckdb()?;
        let conn = duckdb.lock().map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
        })?;
        let result = with_rules(project_root, |registry| {
            crate::insight_engine::execute_insight_rule(registry, rule_id, &conn, &parameters)
        })?;

        let view = MultiResultView::from_execution(&result);
        let notes = crate::model::quality_notes(result.quality.as_ref());
        Ok((view, notes))
    }

    // ==================== 结构洞察（Phase 4） ====================

    /// Schema 健康报告 → 面板视图模型（Phase 4 的门面：补上唯一缺的命令等价物）。
    ///
    /// 分析器本身是异步的（要走源库内省），而服务门面是**同步**口径
    /// （与 `profile_column_view` 一致）：这里用阻塞桥接上，调用方负责放后台。
    ///
    /// `database` 由调用方给出：表清单要按 `table_catalog` 过滤，
    /// 否则同名表会跨库混在一起。
    pub fn schema_report_view(
        conn_id: String,
        database: &str,
        schema: &str,
    ) -> Result<SchemaReportView, CoreError> {
        let report = block_on(crate::schema_analyzer::SchemaAnalyzer::analyze(
            conn_id, database, schema,
        ))?;
        Ok(SchemaReportView::from_report(&report))
    }

    // ==================== 快照历史（Phase 5.1 / 5.2） ====================

    /// 保存当前列的一次快照（正文 + 元数据双写），返回刷新后的历史。
    ///
    /// **重取一次领域画像再存**：面板手里只有视图模型，而快照正文存的是领域结果
    /// （`ColumnInsightFull`）——把视图模型反向拼回去是不可能的，也不应该。
    ///
    /// `entity_source` 记录「这份快照是哪来的」：源目标传来源描述（如
    /// `analytics.orders`），临时表目标只能说临时表名（如实写，不编）。
    pub fn save_column_snapshot(
        project_root: Option<&Path>,
        temp_table: &str,
        column: &str,
        source_label: Option<&str>,
    ) -> Result<HistoryView, CoreError> {
        let root = project_root.ok_or_else(no_project)?;
        let full = Self::get_column_insight_full(Some(root), temp_table, column)?;
        let stores = block_on(crate::store::ProjectInsightStores::open(root))?;
        let entity_source = match source_label {
            Some(label) => format!("{label} · {column}"),
            None => format!("temp_table={temp_table}"),
        };
        block_on(stores.save_column_snapshot(
            &full,
            Some(&entity_source),
            Some(full.stats.total_count as i32),
            None,
        ))?;
        // 读回历史**复用同一个库句柄**：DuckDB 在同一个进程里对同一份文件只允许一个实例，
        // 第二次 open 会报「文件已被占用」——那会表现成「快照写进去了，却提示保存失败」
        // （实测到的就是这个：正文与元数据双双落库，但调用方收到 `open` 错误）。
        read_history(&stores, column)
    }

    /// 读一列的历次快照 + 存储用量。
    pub fn column_history_view(
        project_root: Option<&Path>,
        column: &str,
    ) -> Result<HistoryView, CoreError> {
        let root = project_root.ok_or_else(no_project)?;
        let stores = block_on(crate::store::ProjectInsightStores::open(root))?;
        read_history(&stores, column)
    }

    /// 对比某一版与**最新一版**（Phase 5.2）。
    ///
    /// 方向固定为「选中版本 → 最新版本」：这个 Tab 问的是「和上次比变了什么」，
    /// 两个方向都能选只是多一个状态（还要多一套「谁是基准」的文案）。
    ///
    /// 返回值仍是**整份历史**（列表 + 对比）：对比面板与列表同属一个载荷，
    /// 面板只管「有没有对比」——这样刷新列表时不会留下指向旧「当前」的对比（D43）。
    pub fn compare_column_snapshots(
        project_root: Option<&Path>,
        column: &str,
        baseline_version: &str,
    ) -> Result<HistoryView, CoreError> {
        let root = project_root.ok_or_else(no_project)?;
        let stores = block_on(crate::store::ProjectInsightStores::open(root))?;
        let entries = history_entries(&stores, column)?;

        let baseline = entries
            .iter()
            .find(|entry| entry.version_id == baseline_version)
            .ok_or_else(|| {
                CoreError::common(CommonError::General(format!(
                    "这一版已不在历史里（找不到 {}）",
                    baseline_version.chars().take(8).collect::<String>()
                )))
            })?;
        let latest = entries.first().ok_or_else(|| {
            CoreError::common(CommonError::General("这一列还没有快照".to_string()))
        })?;
        if latest.version_id == baseline.version_id {
            return Err(CoreError::common(CommonError::General(
                "最新一版没有更新的版本可比".to_string(),
            )));
        }

        // 两侧正文都读出来比：比的是**存下来的那份结论**，不是现在重算的
        let diff = crate::model::VersionDiffView::between(
            &baseline.parse_insight()?,
            &baseline.created_at,
            &latest.parse_insight()?,
            &latest.created_at,
        )
        .with_baseline_version(&baseline.version_id);

        Ok(history_view_of(&stores, column, entries).with_diff(diff))
    }

    /// 清理 `days` 天前的快照（Phase 5.3）：正文与元数据**成对删**，返回刷新后的历史。
    ///
    /// `days` 由调用方给（接缝传 `model::SNAPSHOT_RETENTION_DAYS`）：服务不持策略，
    /// 但界面上写的天数与实际切档必须是同一个值（那个常量就是这层约定的落脚点）。
    ///
    /// 两侧条数都带回去：对不上是半写的信号（D16），不能只报一个数就说「清理完成」。
    pub fn cleanup_old_snapshots(
        project_root: Option<&Path>,
        column: &str,
        days: i64,
    ) -> Result<HistoryView, CoreError> {
        let root = project_root.ok_or_else(no_project)?;
        let stores = block_on(crate::store::ProjectInsightStores::open(root))?;
        let (body_removed, meta_removed) = block_on(cleanup_old_insight_snapshots(
            days as i32,
            &stores.storage,
            &stores.meta,
        ))?;
        let outcome = crate::model::CleanupOutcome {
            days,
            body_removed: body_removed.max(0) as usize,
            meta_removed,
        };
        // 读回列表同样**复用同一个库句柄**（D41：同进程对同一项目库不得重叠 open）
        Ok(read_history(&stores, column)?.with_cleanup(outcome))
    }
}
