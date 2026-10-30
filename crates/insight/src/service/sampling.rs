//! 洞察服务门面 · 源取样（自 service/mod.rs 拆分，纯位移）

use super::*;

impl InsightService {
    // ==================== 源取样（统一入口契约）====================

    /// 源目标列画像：**取样（或全量）→ 分析临时表 → 画像**，返回（样本表名, 视图）。
    ///
    /// 「凡能喂给 DuckDB 的数据都能洞察」的落点：导航树 / 分析存档 / 草稿箱 /
    /// 编辑器结果集都只需给出「连接 + 只读查询 + 标签」，其余在这里完成。
    ///
    /// 返回样本表名是必需的：面板保存快照 / 开多列 / 下钻要接着用**同一份样本**。
    /// 阻塞（要跑源库查询 + DuckDB 分析），调用方负责放后台。
    pub fn profile_source_column(
        project_root: Option<&Path>,
        source: &crate::model::SampleSource,
        column_name: &str,
        full: bool,
    ) -> Result<(String, ColumnProfileView), CoreError> {
        block_on(persistence::profile_source_column(
            project_root,
            source,
            column_name,
            full,
        ))
    }

    /// 源目标表探查：取样（或全量）→ 内省，返回（样本表名, 视图）。
    pub fn profile_source_table(
        source: &crate::model::SampleSource,
        table_name: &str,
        full: bool,
    ) -> Result<(String, TableProfileView), CoreError> {
        block_on(persistence::profile_source_table(source, table_name, full))
    }

    // ==================== 表探查（Phase 3.1） ====================

    /// 表探查（Tab「表」）：临时表内省 → 视图模型。
    ///
    /// **不跑规则**：表探查只看元数据与行数；逐列规则统计是「评估全表」的事
    /// （那一步很贵，必须由用户显式发起，进度也要可见）。
    ///
    /// `project_root` 目前不被使用（表探查不依赖规则集），保留形参是为了与
    /// 列画像门面保持一致的调用口径——将来表级取样规则要分项目时不用改签名。
    pub fn profile_table_view(
        project_root: Option<&Path>,
        temp_table: &str,
        table_name: &str,
    ) -> Result<TableProfileView, CoreError> {
        let _ = project_root;
        let profile = crate::insight_engine::get_temp_table_profile(temp_table)?;
        let mut view = TableProfileView::from_profile(&profile, table_name);
        // EDA 列统计摘要（缺失率 / 唯一率）：查询失败不阻塞画像——列清单仍可看，
        // 只是概览卡 / 缺失矩阵少两列数字（保留 None，不编假值）。
        let column_names: Vec<String> = profile
            .columns
            .iter()
            .map(|c| c.column_name.clone())
            .collect();
        if let Ok(stats) =
            crate::insight_engine::compute_table_column_summary(temp_table, &column_names)
        {
            view.fill_column_stats(&stats);
        }
        // 整行重复统计（概览五卡「重复行」）：失败不阻塞画像，卡位显示「—」
        if let Ok(dup) = crate::insight_engine::compute_dup_rows(temp_table) {
            view.dup_rows = Some(dup);
        }
        // EDA 样本（前 5 行）：同样失败不阻塞画像（样本区给空态即可）
        if let Ok(sample) = crate::insight_engine::get_temp_table_sample(temp_table) {
            view.sample = Some(sample);
        }
        Ok(view)
    }

    // ==================== 跨表关系排查（T19） ====================

    /// 两张洞察临时表的同名列值重叠报告（右栏集合卡「关系」按钮的取数）。
    ///
    /// 阻塞（抢 DuckDB 全局锁），调用方负责放后台线程（`jobs` 事件路径）。
    pub fn overlap_report(
        table_a: &str,
        table_b: &str,
    ) -> Result<crate::model::OverlapView, CoreError> {
        let duckdb = crate::insight_engine::get_or_create_duckdb()?;
        let conn = duckdb.lock().map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB lock error: {e}")))
        })?;
        let rows = crate::insight_engine::compute_column_overlap(&conn, table_a, table_b)?;
        Ok(crate::model::OverlapView {
            table_a: table_a.to_string(),
            table_b: table_b.to_string(),
            rows,
        })
    }

    // ==================== 归档快照（T20） ====================

    /// 把一张洞察临时表的画像快照导出为 JSON 文件（归档到资产库的本体）。
    ///
    /// 快照 = 表探查元数据（列名 / 类型 / 可空 / 主键 / 行数）+ 生成时间戳：
    /// 归档后它作为资产库 `File` 档的本体，版本链由资产库按内容指纹维护——
    /// 同一张表两次归档生成两份不同内容（时间戳在指纹里）即两个版本。
    ///
    /// # 返回
    /// `(文件绝对路径, 展示名)`；文件落在 `{项目}/.RSmeta/insight-archive/`，
    /// 该目录在项目目录内（随项目备份），不入 resources/（那是资产库本体区，
    /// 由资产库登记时复制，这里只负责「产生可归档的本体」）。
    ///
    /// 阻塞（抢 DuckDB 锁），调用方放后台线程。
    pub fn export_table_snapshot(
        project_root: &Path,
        temp_table: &str,
        table_name: &str,
    ) -> Result<(PathBuf, String), CoreError> {
        use chrono::Local;

        let profile = crate::insight_engine::get_temp_table_profile(temp_table)?;
        let json = serde_json::json!({
            "kind": "insight_table_snapshot",
            "table": table_name,
            "generated_at": Local::now().to_rfc3339(),
            "row_count": profile.row_count,
            "column_count": profile.columns.len(),
            "columns": profile.columns.iter().map(|c| {
                serde_json::json!({
                    "name": c.column_name,
                    "type": c.data_type,
                    "nullable": c.is_nullable,
                    "primary_key": c.is_primary_key,
                })
            }).collect::<Vec<_>>(),
        });

        let dir = project_root.join(".RSmeta").join("insight-archive");
        std::fs::create_dir_all(&dir).map_err(|e| {
            CoreError::common(CommonError::General(format!("创建洞察归档目录失败: {e}")))
        })?;
        let stamp = Local::now().format("%Y%m%d%H%M%S");
        let file_name = format!("{table_name}_{stamp}.json");
        let path = dir.join(&file_name);
        let bytes = serde_json::to_vec_pretty(&json)
            .map_err(|e| CoreError::common(CommonError::General(format!("快照序列化失败: {e}"))))?;
        std::fs::write(&path, bytes)
            .map_err(|e| CoreError::common(CommonError::General(format!("快照写入失败: {e}"))))?;
        Ok((path, table_name.to_string()))
    }
}
