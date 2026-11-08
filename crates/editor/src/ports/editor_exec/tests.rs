//! 本模块的单元用例。

use super::*;
use super::convert::*;
use super::federation::*;

#[cfg(test)]
mod analysis_tests {
    // 安全模式：**不通配导入**
    use super::{EngineQueryRunner, NoopFederatedSource};
    use crate::analysis::AnalysisRequest;
    use crate::execution::QueryRunner as _;
    use ::shared::{FactFrame, StoragePref};
    use std::sync::Arc;

    /// 单列 Int64、两行的事实档（类型来自 Arrow schema，不猜型）。
    fn fact_frame() -> Arc<FactFrame> {
        use arrow::array::{ArrayRef, Int64Array, RecordBatch};
        use arrow::datatypes::{DataType, Field, Schema};

        let array: ArrayRef = Arc::new(Int64Array::from(vec![1i64, 2]));
        let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)]));
        let batch = RecordBatch::try_new(schema, vec![array]).expect("批能建出来");
        Arc::new(FactFrame::new(vec!["id".to_string()], vec![batch]).expect("事实档"))
    }

    /// 桥接的行真能落到内存 DuckDB 上跑出结果（**不需要任何服务器**）
    ///
    /// 这条把三件事一起钉住：`{table}` 占位符被替换、行被逐行写进临时表、
    /// `NULL` 字面量当真 NULL（否则 `count(id)` 会把那一行也算上）。
    #[test]
    fn bridged_rows_run_on_the_in_memory_duckdb() {
        let Some(runner) = EngineQueryRunner::new(Arc::new(NoopFederatedSource)) else {
            eprintln!("⏭️ 建不出执行器 runtime，跳过本地分析探针");
            return;
        };
        let request = AnalysisRequest {
            sql: "SELECT count(*) AS \"总\", count(\"id\") AS \"非空\" FROM {table}".to_string(),
            columns: vec!["id".to_string()],
            rows: vec![vec!["1".to_string()], vec!["NULL".to_string()]],
            dropped_rows: 0,
            fact: None,
            pref: StoragePref::Auto,
            source_set: 0,
        };
        let data = runner.analyze(&request).expect("本地分析该跑通");
        assert_eq!(data.columns, vec!["总".to_string(), "非空".to_string()]);
        assert_eq!(
            data.rows,
            vec![vec!["2".to_string(), "1".to_string()]],
            "两行都进表；NULL 是真 NULL（非空计数只算 1）"
        );
        assert!(!data.has_more, "分析一次算完，没有下一段");
        let notice = data.notice.clone().unwrap_or_default();
        assert!(notice.contains("基于已抓取的 2 行"), "{notice}");
        // 无事实档 = 显示矩阵猜型降级档：notice 必须显式标注口径，不静默混进保真档
        assert!(notice.contains("文本猜型降级档"), "{notice}");
    }

    /// 【B4】内存档事实档走 batches 直灌（保真档）：表名信号回传、目标内存档不释放
    /// batches、原槽位下标随回执透传，notice 不带降级档标注。
    #[test]
    fn in_memory_fact_ingests_batches_and_reports_table_handle() {
        let Some(runner) = EngineQueryRunner::new(Arc::new(NoopFederatedSource)) else {
            eprintln!("⏭️ 建不出执行器 runtime，跳过本地分析探针");
            return;
        };
        let request = AnalysisRequest {
            sql: "SELECT count(*) AS \"n\" FROM {table}".to_string(),
            columns: vec!["id".to_string()],
            // 显示矩阵仍随请求带上（契约在），但事实档在场时不参与建表
            rows: vec![vec!["1".to_string()], vec!["2".to_string()]],
            dropped_rows: 0,
            fact: Some(fact_frame()),
            // 强制内存档：分流目标 = InMemory → 不释放 batches（release=false）
            pref: StoragePref::Memory,
            source_set: 7,
        };
        let data = runner.analyze(&request).expect("事实档直灌分析该跑通");
        assert_eq!(data.rows, vec![vec!["2".to_string()]], "两行事实档聚合出 2");
        assert!(
            data.source_fact_table.is_some(),
            "新建常驻表名要回传给 host 挂回原槽位"
        );
        assert!(!data.source_fact_release, "内存档目标不释放 batches");
        assert_eq!(data.source_fact_set, 7, "原槽位下标随回执透传");
        let notice = data.notice.clone().unwrap_or_default();
        assert!(
            !notice.contains("文本猜型降级档"),
            "保真档不挂降级标注：{notice}"
        );
    }

    /// 【B4】强制表档偏好：分流目标 = OnDuckDb → 回执置释放信号，host 据此提升表档。
    #[test]
    fn table_pref_signals_release_back_to_the_host() {
        let Some(runner) = EngineQueryRunner::new(Arc::new(NoopFederatedSource)) else {
            return;
        };
        let request = AnalysisRequest {
            sql: "SELECT count(*) AS \"n\" FROM {table}".to_string(),
            columns: vec!["id".to_string()],
            rows: vec![vec!["1".to_string()], vec!["2".to_string()]],
            dropped_rows: 0,
            fact: Some(fact_frame()),
            pref: StoragePref::Table,
            source_set: 0,
        };
        let data = runner.analyze(&request).expect("表档直灌分析该跑通");
        assert!(
            data.source_fact_table.is_some(),
            "常驻表名回传；表档复用 / 新建都要给 host 一个句柄"
        );
        assert!(
            data.source_fact_release,
            "强制表档目标要释放 Rust 堆 batches"
        );
    }

    /// 分析 SQL 写错：如实报错（不静默出空网格）
    #[test]
    fn a_broken_analysis_sql_reports_the_reason() {
        let Some(runner) = EngineQueryRunner::new(Arc::new(NoopFederatedSource)) else {
            return;
        };
        let request = AnalysisRequest {
            sql: "SELECT nope FROM {table}".to_string(),
            columns: vec!["id".to_string()],
            rows: vec![vec!["1".to_string()]],
            dropped_rows: 0,
            fact: None,
            pref: StoragePref::Auto,
            source_set: 0,
        };
        let error = runner.analyze(&request).expect_err("该报错");
        assert!(!error.is_empty(), "错误要说清原因：{error}");
    }
}

#[cfg(test)]
mod export_tests {
    // 安全模式：**不通配导入**
    use super::{EngineQueryRunner, NoopFederatedSource};
    use crate::execution::{DuckDbExportRequest, QueryRunner as _};
    use crate::export::ExportFormat;
    use crate::model::DocumentId;
    use ::shared::FactFrame;
    use std::sync::Arc;

    /// 单列 Int64、两行的事实档（类型来自 Arrow schema，不猜型）。
    fn fact_frame() -> FactFrame {
        use arrow::array::{ArrayRef, Int64Array, RecordBatch};
        use arrow::datatypes::{DataType, Field, Schema};

        let array: ArrayRef = Arc::new(Int64Array::from(vec![1i64, 2]));
        let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)]));
        let batch = RecordBatch::try_new(schema, vec![array]).expect("批能建出来");
        FactFrame::new(vec!["id".to_string()], vec![batch]).expect("事实档")
    }

    fn request(format: ExportFormat, path: std::path::PathBuf) -> DuckDbExportRequest {
        DuckDbExportRequest {
            document: DocumentId::new("doc"),
            set: 1,
            format,
            path,
            columns: vec!["id".to_string(), "tag".to_string()],
            // 展示文本那一层：`NULL` 是字面量（与网格一致）
            rows: vec![
                vec!["1".to_string(), "alpha".to_string()],
                vec!["2".to_string(), "NULL".to_string()],
            ],
            filtered: false,
            // 测试走显示矩阵猜型降级档：无事实档快照
            fact: None,
            degraded: true,
        }
    }

    /// 【B7 切片二】Parquet 导出：真写文件，且引擎能读回来（`NULL` 要是真 NULL）
    ///
    /// **不需要任何服务器**（Parquet 是 DuckDB 内核自带的），所以这条可以离线跑。
    #[test]
    fn parquet_export_writes_a_file_the_engine_reads_back() {
        let Some(runner) = EngineQueryRunner::new(Arc::new(NoopFederatedSource)) else {
            eprintln!("⏭️ 建不出执行器 runtime，跳过导出探针");
            return;
        };
        let path =
            std::env::temp_dir().join(format!("rds_editor_export_{}.parquet", std::process::id()));
        std::fs::remove_file(&path).ok();

        let rows = runner
            .export_via_duckdb(&request(ExportFormat::Parquet, path.clone()))
            .expect("Parquet 导出该跑通");
        assert_eq!(rows, 2);
        assert!(
            std::fs::metadata(&path)
                .map(|m| m.len() > 0)
                .unwrap_or(false),
            "Parquet 文件要真写出来"
        );

        // 读回来：数字是数字、`NULL` 字面量在盘上是真 NULL
        let duckdb = engine::services::duckdb_service::DuckDbService::get_or_create_duckdb()
            .expect("内存库");
        let mut conn = duckdb.lock().expect("锁");
        let file = path.to_string_lossy().replace('\\', "/");
        let (cols, got) = engine::services::duckdb_service::DuckDbService::query_duckdb(
            &mut conn,
            &format!("SELECT * FROM read_parquet('{file}') ORDER BY id"),
        )
        .expect("读回 Parquet");
        assert_eq!(cols, vec!["id".to_string(), "tag".to_string()]);
        assert_eq!(got.len(), 2);
        assert_eq!(got[1][1], serde_json::Value::Null, "NULL 要真是 NULL");

        std::fs::remove_file(&path).ok();
    }

    /// 文本三档不该走这条路（它们是编辑侧编码；叫错要**如实回绝**而不是写个空文件）
    #[test]
    fn a_text_format_is_refused_by_the_duckdb_port() {
        let Some(runner) = EngineQueryRunner::new(Arc::new(NoopFederatedSource)) else {
            return;
        };
        let path =
            std::env::temp_dir().join(format!("rds_editor_export_{}.csv", std::process::id()));
        let error = runner
            .export_via_duckdb(&request(ExportFormat::Csv, path.clone()))
            .expect_err("CSV 不该走 DuckDB");
        assert!(error.contains("不走 DuckDB"), "{error}");
        assert!(!path.exists(), "回绝时不该留下文件");
    }

    /// 【D1】内存档事实帧走 batches 直灌导出：数字列保留 BIGINT，不挂降级口径。
    #[test]
    fn batches_fact_export_writes_parquet_with_native_type() {
        let Some(runner) = EngineQueryRunner::new(Arc::new(NoopFederatedSource)) else {
            eprintln!("⏭️ 建不出执行器 runtime，跳过导出探针");
            return;
        };
        let path =
            std::env::temp_dir().join(format!("rds_export_batches_{}.parquet", std::process::id()));
        std::fs::remove_file(&path).ok();

        let request = DuckDbExportRequest {
            document: DocumentId::new("doc"),
            set: 1,
            format: ExportFormat::Parquet,
            path: path.clone(),
            columns: vec!["id".to_string()],
            rows: vec![vec!["1".to_string()], vec!["2".to_string()]],
            filtered: false,
            fact: Some(Arc::new(fact_frame())),
            // 保真档：不标降级
            degraded: false,
        };
        let rows = runner
            .export_via_duckdb(&request)
            .expect("batches 直灌导出该跑通");
        assert_eq!(rows, 2);

        let duckdb = engine::services::duckdb_service::DuckDbService::get_or_create_duckdb()
            .expect("内存库");
        let mut conn = duckdb.lock().expect("锁");
        let file = path.to_string_lossy().replace('\\', "/");
        let (cols, types) = engine::services::duckdb_service::DuckDbService::query_duckdb(
            &mut conn,
            &format!("SELECT typeof(id) AS t FROM read_parquet('{file}') LIMIT 1"),
        )
        .expect("读回类型");
        assert_eq!(cols, vec!["t".to_string()]);
        assert_eq!(
            types[0][0].as_str(),
            Some("BIGINT"),
            "数字列保留数值类型，不被收窄成文本"
        );

        std::fs::remove_file(&path).ok();
    }

    /// 【D1】表档事实帧复用常驻表导出：不新建表、不 DROP，行数按帧缓存报出。
    #[test]
    fn existing_table_fact_export_reuses_the_resident_table() {
        let Some(runner) = EngineQueryRunner::new(Arc::new(NoopFederatedSource)) else {
            eprintln!("⏭️ 建不出执行器 runtime，跳过导出探针");
            return;
        };
        use engine::duckdb::{TempTableSource, drop_temp_table};

        let mut frame = fact_frame();
        let batches = frame.batches().to_vec();
        // 先经分析设施把 batches 灌进常驻表，拿到真实表名（表档复用的前提）
        let analysis =
            crate::ports::result_service::ResultService::execute_duckdb_analysis_from_batches(
                None,
                "SELECT * FROM {table}",
                &batches,
            )
            .expect("常驻表应当建出来");
        let table = analysis.temp_table.clone();

        // 表档帧：挂表名、释放 batches（与 B2 回挂口径一致）
        frame.set_temp_table(table.clone());
        frame.release_batches();
        let fact = Arc::new(frame);

        let path =
            std::env::temp_dir().join(format!("rds_export_table_{}.parquet", std::process::id()));
        std::fs::remove_file(&path).ok();
        let request = DuckDbExportRequest {
            document: DocumentId::new("doc"),
            set: 1,
            format: ExportFormat::Parquet,
            path: path.clone(),
            columns: vec!["id".to_string()],
            rows: vec![vec!["1".to_string()], vec!["2".to_string()]],
            filtered: false,
            fact: Some(fact),
            degraded: false,
        };
        let rows = runner
            .export_via_duckdb(&request)
            .expect("常驻表复用导出该跑通");
        assert_eq!(rows, 2, "行数按事实帧缓存报出");

        // 常驻表仍在：导出路径不回收表（归事实档生命周期管）
        let duckdb = engine::services::duckdb_service::DuckDbService::get_or_create_duckdb()
            .expect("内存库");
        let mut conn = duckdb.lock().expect("锁");
        let (_, count_rows) = engine::services::duckdb_service::DuckDbService::query_duckdb(
            &mut conn,
            &format!("SELECT count(*) AS n FROM \"{table}\""),
        )
        .expect("复用表应当还能查");
        assert_eq!(count_rows[0][0].as_i64(), Some(2), "表没被导出路径 DROP");

        drop_temp_table(&conn, TempTableSource::Query, &table).expect("测试收尾清场");
        std::fs::remove_file(&path).ok();
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{FedSourceRecord, cell_text, column_names, plan_from_records};
    use engine::duckdb::accel::AccelKind;
    use shared::models::{QueryResult, Value};

    /// 一条源记录（宿主组装好的形状：URL 已带凭据）
    fn record(conn_id: &str, name: &str, db_type: &str, url: &str) -> FedSourceRecord {
        FedSourceRecord {
            conn_id: conn_id.to_string(),
            name: name.to_string(),
            db_type: db_type.to_string(),
            url: url.to_string(),
        }
    }

    #[test]
    fn driver_columns_win_over_the_schema() {
        let mut result = QueryResult::empty();
        result.columns = vec!["n".to_string(), "m".to_string()];
        assert_eq!(column_names(&result), ["n".to_string(), "m".to_string()]);
    }

    #[test]
    fn a_result_without_columns_or_batches_has_no_names() {
        let result = QueryResult::empty();
        assert!(column_names(&result).is_empty());
    }

    #[test]
    fn cells_show_the_value_not_a_debug_dump() {
        assert_eq!(cell_text(&Value::Null), "NULL");
        assert_eq!(cell_text(&Value::Int(42)), "42");
        assert_eq!(cell_text(&Value::Float(1.5)), "1.5");
        assert_eq!(cell_text(&Value::Bool(true)), "true");
        assert_eq!(cell_text(&Value::Text("中文".to_string())), "中文");
    }

    /// 组装源清单：别名稳定、凭据随连接串带上、主源是本文档的连接
    #[test]
    fn the_federated_plan_names_sources_and_points_at_the_primary() {
        // 故意把顺序打乱：组装结果不该跟着读表的顺序漂
        let records = vec![
            record("G_2", "仓库", "postgres", "postgres://u:real@h:5432/w"),
            record(
                "G_1",
                "订单库",
                "mysql_native",
                "mysql://root:real@h:3306/shop",
            ),
        ];
        let plan = plan_from_records(&records, &[], "G_1").expect("组装");

        assert_eq!(plan.owner, "G_1");
        assert_eq!(plan.sources.len(), 2);
        // 中文连接名整串变成 `db`，第二个重名就排到 `db_2`（顺序按连接 id）
        assert_eq!(plan.sources[0].alias, "db", "G_1 先（按 id 排序）");
        assert_eq!(plan.sources[1].alias, "db_2");
        assert_eq!(plan.primary.as_deref(), Some("db"), "主源 = 本文档的连接");
        assert!(
            plan.notes.is_empty(),
            "没出问题就别制造说明：{:?}",
            plan.notes
        );
        // 凭据随连接串进 `ATTACH`（DuckDB 扫描器不认 Secret，见 accel::AccelSource::new）
        assert_eq!(
            plan.sources[0].connection_string,
            "mysql://root:real@h:3306/shop"
        );
        assert_eq!(
            plan.sources[1].connection_string,
            "postgres://u:real@h:5432/w"
        );
        assert_eq!(plan.sources[0].kind, AccelKind::MySql);
        assert_eq!(plan.sources[1].kind, AccelKind::PostgreSql);
    }

    /// 同一个连接在两张表里都出现（记录 + 运行态）：只算一次
    #[test]
    fn a_source_that_shows_up_twice_counts_once() {
        let records = vec![
            record(
                "G_1",
                "订单库",
                "mysql_native",
                "mysql://root:real@h:3306/shop",
            ),
            record(
                "G_1",
                "订单库",
                "mysql_native",
                "mysql://root:real@h:3306/shop",
            ),
            record("G_2", "仓库", "postgres", "postgres://u:real@h:5432/w"),
        ];
        let plan = plan_from_records(&records, &[], "G_1").expect("组装");
        assert_eq!(plan.sources.len(), 2, "重复的 conn_id 不该占两个源");
    }

    /// 驱动不支持的源 / 组装时攒下的实话：都留在 notes 里，不静默
    #[test]
    fn the_plan_says_what_it_left_out() {
        // 本文档的连接没开开关（不在记录里）：源清单里只有别人，说明里点名
        let records = vec![
            record("G_2", "仓库", "postgres", "postgres://h:5432/w"),
            record("G_3", "分析库", "duckdb", "D:/data/a.duckdb"),
        ];
        let plan = plan_from_records(&records, &[], "G_1").expect("组装");
        assert_eq!(plan.primary, None, "主源不是它，只能由引擎选第一个可用源");
        assert!(
            plan.notes.iter().any(|note| note.contains("本文档的连接")),
            "要说清本文档的连接为什么没参与：{:?}",
            plan.notes
        );

        // 认不出的驱动（还没接入的库，如 ClickHouse）：不因为一个连接不可用就整份失败
        let records = vec![
            record("G_1", "订单库", "mysql_native", "mysql://h:3306/a"),
            record("G_2", "仓库", "postgres", "postgres://h:5432/w"),
            record("G_3", "归档库", "clickhouse", "clickhouse://h:8123/db"),
        ];
        let plan = plan_from_records(&records, &[], "G_1").expect("组装");
        assert_eq!(plan.sources.len(), 2, "驱动不支持的连接不占位");
        assert!(
            plan.notes.iter().any(|note| note.contains("归档库")),
            "要说清哪个连接没参与：{:?}",
            plan.notes
        );

        // L2（Oracle）组装不出来时（连接串缺凭据 / 服务名）也不阻断：点名它，其余源照用
        let records = vec![
            record("G_1", "订单库", "mysql_native", "mysql://h:3306/a"),
            record("G_2", "仓库", "postgres", "postgres://h:5432/w"),
            record("G_3", "老库", "oracle", "oracle://h:1521/XEPDB1"),
        ];
        let plan = plan_from_records(&records, &[], "G_1").expect("组装");
        assert_eq!(plan.sources.len(), 2, "L2 组装不出来的不占位");
        assert!(
            plan.notes
                .iter()
                .any(|note| note.contains("老库") && note.contains("凭据")),
            "要说清它差什么（这是 L2 最常见的一种失败）：{:?}",
            plan.notes
        );

        // L2 组装得出来就**真是参与**（不是只说一句）；凭据进会话级 Secret，类归联邦源
        let records = vec![
            record("G_1", "订单库", "mysql_native", "mysql://u:p@h:3306/a"),
            record("G_2", "老库", "oracle", "oracle://u:p@h:1521/XEPDB1"),
        ];
        let plan = plan_from_records(&records, &[], "G_1").expect("组装");
        assert_eq!(plan.sources.len(), 2, "L2 该占位：{:?}", plan.notes);
        let oracle = plan
            .sources
            .iter()
            .find(|source| source.kind.needs_secret())
            .expect("Oracle 源该在清单里");
        assert!(oracle.secret.is_some(), "L2 的凭据应该是 Secret");
        assert!(
            !oracle.attach_sql().contains("READ_ONLY"),
            "L2 不带只读选项"
        );

        // 组装时已经攒下的实话（读连接库失败 / 连接串组不出来）要带到结果区
        let carried = vec!["连接 甲 的连接串没能组装出来：解密失败".to_string()];
        let records = vec![
            record("G_1", "订单库", "mysql_native", "mysql://h:3306/a"),
            record("G_2", "仓库", "postgres", "postgres://h:5432/w"),
        ];
        let plan = plan_from_records(&records, &carried, "G_1").expect("组装");
        assert!(
            plan.notes.iter().any(|note| note.contains("解密失败")),
            "攒下的说明不该丢：{:?}",
            plan.notes
        );
    }

    /// 不够两个源 / 一个都没有：入口就把话说清楚（与门控同一口径）
    #[test]
    fn the_plan_refuses_before_the_session_is_built() {
        let one = vec![record("G_1", "订单库", "mysql_native", "mysql://h:3306/a")];
        let error = plan_from_records(&one, &[], "G_1").expect_err("一个源该拒");
        assert!(error.contains("至少需要两个源"), "{error}");

        let error = plan_from_records(&[], &[], "G_1").expect_err("没有源该拒");
        assert!(error.contains("本地加速"), "{error}");
    }
}
