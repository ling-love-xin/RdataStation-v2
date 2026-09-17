//! 分析用临时表的**生命周期**（洞察 / 表画像这类「取样本 → 算 → 扔掉」的中间产物）。
//!
//! # 为什么单独一个模块
//!
//! 临时表的机制（命名 / 登记 / TTL / 上限 / 按来源清理）都在 [`super::temp_table`] 与
//! [`super::manager`] 里，但「建表时怎么起名、用完什么时候扔掉」过去散在调用点 ——
//! 结果是建出来的表叫 `rs_<uuid>`，**不属于任何来源前缀**：
//! 按来源的清理（`list_by_source` / `drop_by_source` / 惰性 TTL）全都看不见它，
//! 而 `register` 触发的惰性清理也只认 `tmp_i_`。于是进程内的表只增不减（架构 K16）。
//!
//! 本模块把这条路收敛成三个入口，并统一落在 [`ANALYSIS_TABLE_PREFIX`] 这个前缀下：
//!
//! | 入口 | 什么时候用 |
//! | --- | --- |
//! | [`with_analysis_temp_table`] | **默认姿势**：建 → 用 → 无论成败都收掉 |
//! | [`create_analysis_temp_table`] / [`drop_analysis_temp_table`] | 跨函数持有 / 与其他资源交织 |
//! | [`cleanup_analysis_temp_tables`] | 惰性清理：过期的表**真正 DROP** |
//!
//! 与 `mock` 的分工可对照：mock 的表走 `temp_mock_*` / `tmp_m_*`（与
//! [`TempTableSource::Mock`] 的前缀对齐），所以宿主的「项目切换时清掉」对它有效；
//! 本模块做的就是让**洞察**也回到同一套前缀口径。

use duckdb::Connection;
use serde_json::Value;
use shared::error::{CommonError, CoreError};

use super::manager::DuckDBManager;
use super::temp_table::TempTableSource;

/// 分析用临时表的前缀（与 [`TempTableSource::Insight`] 的前缀**必须一致**）。
///
/// 这个前缀不是装饰：TTL / 上限 / 按来源清理都靠它识别。写一个别的名字（如 `rs_`），
/// 那些机制就全成摆设——K16 就是这么来的。
pub const ANALYSIS_TABLE_PREFIX: &str = "tmp_i_";

/// 建一张分析用的临时表，并**登记进临时表管理器**。
///
/// 建表前会先做一次惰性清理（TTL + 上限），避免长会话越积越多。
///
/// 中途失败（插入出错等）会把已经建出来的表收掉再返回错误——不留半成品。
pub fn create_analysis_temp_table(
    conn: &Connection,
    columns: &[String],
    rows: &[Vec<Value>],
    description: &str,
) -> Result<String, CoreError> {
    // 惰性清理放在建表前：让 TTL / 上限在「下一次建表」这个廉价时机生效
    if let Err(e) = cleanup_analysis_temp_tables(conn) {
        // 清理失败不该挡住建表（表还等着用），但要在日志里留痕
        tracing::warn!("[analysis] 惰性清理洞察中间表失败: {e}");
    }

    let table = generate_table_name(description);
    let col_defs: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let dtype = if rows.is_empty() {
                "VARCHAR"
            } else {
                crate::services::duckdb_service::infer_type(rows, i)
            };
            format!("\"{}\" {}", col, dtype)
        })
        .collect();

    conn.execute_batch(&format!(
        "CREATE TABLE {}({})",
        quote_ident(&table),
        col_defs.join(", ")
    ))
    .map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "建分析临时表 {} 失败: {e}",
            table
        )))
    })?;

    if let Err(e) = insert_rows(conn, &table, columns.len(), rows) {
        // 半成品不能留：建成功但灌数据失败时，表已在库里且已占内存
        let _ = drop_analysis_temp_table(conn, &table);
        return Err(e);
    }

    DuckDBManager::register_temp_table(&table);
    warn_if_near_capacity();
    Ok(table)
}

/// 登记数接近上限时给一条**可见信号**（K16 ④）。
///
/// 登记表只增不减是 K16 的病症，而它没有任何外部表现（表在内存库里，界面看不到）。
/// 正常路径下 `create` 会先惰性清理，计数应该远低于上限；一旦持续贴着上限，
/// 说明清理没跟上（或 TTL 被改了）——这条日志是提前发现「内存库在长大」的唯一入口。
fn warn_if_near_capacity() {
    let Some(max) = super::temp_table::TempTableConfig::insight().max_count else {
        return;
    };
    let count = DuckDBManager::temp_table_manager().count_by_prefix(ANALYSIS_TABLE_PREFIX);
    // 80% 开始提醒：留出「还能跑一阵，但该查了」的余量
    if count * 5 >= max * 4 {
        tracing::warn!(
            "[analysis] 洞察中间表登记数 {count}/{max} 接近上限——检查创建与回收是否成对（K16）"
        );
    }
}

/// 删掉一张分析临时表（并同步登记表）。幂等：表不在也返回成功。
///
/// **只接受** [`ANALYSIS_TABLE_PREFIX`] 开头的名字：这个函数会被规则 / 宿主侧的代码
/// 调用，放开前缀就等于给了一把「传什么名字就删什么表」的刀。
pub fn drop_analysis_temp_table(conn: &Connection, table: &str) -> Result<(), CoreError> {
    if !table.starts_with(ANALYSIS_TABLE_PREFIX) {
        return Err(CoreError::common(CommonError::General(format!(
            "拒绝删除非分析临时表：{table}（只允许 {ANALYSIS_TABLE_PREFIX} 开头的表）"
        ))));
    }

    conn.execute_batch(&format!("DROP TABLE IF EXISTS {}", quote_ident(table)))
        .map_err(|e| CoreError::common(CommonError::General(format!("删分析临时表失败: {e}"))))?;
    DuckDBManager::temp_table_manager().unregister(table);
    Ok(())
}

/// 建 → 用 → **无论如何都收掉**（纯中间产物的默认姿势）。
///
/// `body` 收到的是建好的表名。返回值优先给 `body` 的错误：收表的失败只写日志——
/// 主流程的错误比「临时表没删掉」重要得多，不能被它盖掉。
pub fn with_analysis_temp_table<R>(
    conn: &Connection,
    columns: &[String],
    rows: &[Vec<Value>],
    description: &str,
    body: impl FnOnce(&str) -> Result<R, CoreError>,
) -> Result<R, CoreError> {
    let table = create_analysis_temp_table(conn, columns, rows, description)?;
    let outcome = body(&table);
    if let Err(e) = drop_analysis_temp_table(conn, &table) {
        tracing::warn!("[analysis] 收掉临时表 {table} 失败: {e}");
    }
    outcome
}

/// 惰性清理：把**超时与超上限**的洞察中间表真正 DROP 掉，返回被清掉的名字。
///
/// 管理器的 `lazy_cleanup_insight_tables` 只把名字从登记表里摘掉（它拿不到连接，
/// 没法执行 DDL）——所以真正的 DROP 必须在有连接的地方补上，也就是这里。
/// 不补的话，「超上限淘汰」会**把登记丢掉而表还在库里**，下一轮清理再也找不到它。
pub fn cleanup_analysis_temp_tables(conn: &Connection) -> Result<Vec<String>, CoreError> {
    let evicted = DuckDBManager::temp_table_manager().lazy_cleanup_insight_tables();
    let mut dropped = Vec::new();
    for name in evicted {
        // 登记表已经由管理器摘掉了，这里只负责把表删掉
        match conn.execute_batch(&format!("DROP TABLE IF EXISTS {}", quote_ident(&name))) {
            Ok(_) => dropped.push(name),
            Err(e) => tracing::warn!("[analysis] 清理洞察中间表 {name} 失败: {e}"),
        }
    }
    if !dropped.is_empty() {
        tracing::info!("[analysis] 清理 {} 张过期分析临时表", dropped.len());
    }
    Ok(dropped)
}

/// 分析临时表在库里的名字（诊断 / 测试用；**权威来源是库本身**，不是登记表）。
pub fn analysis_temp_tables(conn: &Connection) -> Result<Vec<String>, CoreError> {
    let mut names: Vec<String> = super::temp_table::TempTableManager::list_by_source(
        conn,
        TempTableSource::Insight,
    )?;
    names.sort();
    Ok(names)
}

// ==================== 内部 ====================

/// 表名：`tmp_i_<描述>_<紧凑时间戳>_<短随机>`。
///
/// 末尾那截随机不是好看：管理器的 `generate_name` 只到**秒**，同一秒建两张同描述的表
/// 会撞名（`CREATE TABLE` 直接失败）。分析路径一秒内建多张完全可能（多列分析逐列）。
fn generate_table_name(description: &str) -> String {
    let desc = sanitize_description(description);
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    let uniq = uuid::Uuid::new_v4().simple().to_string();
    format!("{ANALYSIS_TABLE_PREFIX}{desc}_{stamp}_{}", &uniq[..8])
}

/// 描述里只留字母 / 数字 / 下划线（表名要内联进 SQL，虽有引号也不值得冒险）
fn sanitize_description(description: &str) -> String {
    let cleaned: String = description
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.trim_matches('_').is_empty() {
        "t".to_string()
    } else {
        cleaned
    }
}

/// 标识符引号（名字由本模块生成 / 校验过，这里只做最小必要的包裹）
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 灌数据（参数化插入；值的转换与结果集那条路径共用一份实现）
fn insert_rows(
    conn: &Connection,
    table: &str,
    column_count: usize,
    rows: &[Vec<Value>],
) -> Result<(), CoreError> {
    if rows.is_empty() {
        return Ok(());
    }
    let placeholders: Vec<String> = (0..column_count).map(|_| "?".to_string()).collect();
    let insert_sql = format!(
        "INSERT INTO {} VALUES ({})",
        quote_ident(table),
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&insert_sql).map_err(|e| {
        CoreError::common(CommonError::General(format!("准备插入失败: {e}")))
    })?;
    for row in rows {
        let params: Vec<duckdb::types::Value> = row
            .iter()
            .map(crate::services::duckdb_service::json_to_duckdb_value)
            .collect();
        let params_refs: Vec<&dyn duckdb::types::ToSql> =
            params.iter().map(|p| p as &dyn duckdb::types::ToSql).collect();
        stmt.execute(&params_refs[..]).map_err(|e| {
            CoreError::common(CommonError::General(format!("插入行失败: {e}")))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Vec<String>, Vec<Vec<Value>>) {
        (
            vec!["id".into(), "amount".into(), "note".into()],
            vec![
                vec![Value::from(1), Value::from(1.5), Value::from("a")],
                vec![Value::from(2), Value::Null, Value::Null],
            ],
        )
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM information_schema.tables
             WHERE table_catalog = 'memory' AND table_schema = 'main' AND table_name = ?",
            duckdb::params![name],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false)
    }

    /// 名字必须落在洞察前缀下，并且登记进管理器（否则 TTL / 上限 / 按来源清理都看不见它）
    #[test]
    fn create_uses_the_insight_prefix_and_registers() {
        let conn = Connection::open_in_memory().expect("内存连接");
        let (cols, rows) = sample();
        let table = create_analysis_temp_table(&conn, &cols, &rows, "col_sample").expect("建表");

        assert!(
            table.starts_with("tmp_i_col_sample_"),
            "表名必须带洞察前缀与描述（K16 的根因就是前缀不匹配）：{table}"
        );
        assert!(table_exists(&conn, &table));
        assert!(
            analysis_temp_tables(&conn)
                .expect("查库")
                .contains(&table),
            "按来源查库应当能看见它（这正是按前缀清理的依据）"
        );

        drop_analysis_temp_table(&conn, &table).expect("收掉");
        assert!(!table_exists(&conn, &table));
    }

    /// 同一秒建多张同描述的表不能撞名（多列分析会逐列建）：末尾那截随机就是为这个
    #[test]
    fn names_stay_unique_within_the_same_second() {
        let conn = Connection::open_in_memory().expect("内存连接");
        let (cols, rows) = sample();
        let a = create_analysis_temp_table(&conn, &cols, &rows, "dup").expect("第一张");
        let b = create_analysis_temp_table(&conn, &cols, &rows, "dup").expect("第二张");
        assert_ne!(a, b, "同秒同描述不得撞名");
        assert!(table_exists(&conn, &a) && table_exists(&conn, &b));

        drop_analysis_temp_table(&conn, &a).expect("收 a");
        drop_analysis_temp_table(&conn, &b).expect("收 b");
    }

    /// 描述会被清洗成可安全内联的片段（表名要拼进 SQL）
    #[test]
    fn description_is_sanitized() {
        assert_eq!(sanitize_description("col_sample"), "col_sample");
        assert_eq!(sanitize_description("a b-c"), "a_b_c");
        assert_eq!(
            sanitize_description("列画像 样本"),
            "t",
            "全是非 ASCII 时给个兜底，不留空片段"
        );
    }

    /// 默认姿势：用完就收，body 失败也收
    #[test]
    fn with_scope_drops_the_table_even_when_the_body_fails() {
        let conn = Connection::open_in_memory().expect("内存连接");
        let (cols, rows) = sample();

        let seen = with_analysis_temp_table(&conn, &cols, &rows, "col_sample", |table| {
            assert!(table_exists(&conn, table), "body 里表应当在");
            Ok(table.to_string())
        })
        .expect("正常路径");
        assert!(!table_exists(&conn, &seen), "出来就该没了");

        let err = with_analysis_temp_table(&conn, &cols, &rows, "col_sample", |table| {
            Err::<(), CoreError>(CoreError::common(CommonError::General(
                "算到一半炸了".into(),
            )))
            .map(|_| {
                let _ = table;
            })
        })
        .expect_err("body 的错误要原样返回");
        assert!(err.to_string().contains("算到一半炸了"));
        assert!(
            analysis_temp_tables(&conn).expect("查库").is_empty(),
            "body 失败也不能把表留下"
        );
    }

    /// 只允许删自己前缀的表：这个函数会被规则 / 宿主侧调用，放开前缀等于给了一把乱删的刀
    #[test]
    fn drop_refuses_foreign_names() {
        let conn = Connection::open_in_memory().expect("内存连接");
        conn.execute_batch("CREATE TABLE rs_not_ours (a INTEGER)")
            .expect("先造一张别人的表");

        let err = drop_analysis_temp_table(&conn, "rs_not_ours").expect_err("应拒绝");
        assert!(err.to_string().contains("拒绝删除"), "{err}");
        assert!(table_exists(&conn, "rs_not_ours"), "别人的表必须还在");
    }

    /// 清理不得误伤**刚建的 / 正在用**的表：只有超时与超上限的才在淘汰名单里
    #[test]
    fn cleanup_keeps_fresh_tables() {
        let conn = Connection::open_in_memory().expect("内存连接");
        let (cols, rows) = sample();
        let fresh = create_analysis_temp_table(&conn, &cols, &rows, "fresh").expect("建表");

        let dropped = cleanup_analysis_temp_tables(&conn).expect("清理");
        assert!(dropped.is_empty(), "没有过期的就该无事发生：{dropped:?}");
        assert!(table_exists(&conn, &fresh), "刚建的表不能被清掉");

        drop_analysis_temp_table(&conn, &fresh).expect("收尾");
    }
}
