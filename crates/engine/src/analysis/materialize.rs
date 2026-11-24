//! 分析内核 · 物化（形态 I / II 的唯一实现）。
//!
//! 两种形态在装配上完全一样：**在会话里建一张表，内容来自一条 SELECT**。
//! 差别只在名字由谁给：
//!
//! - **形态 I（洞察取样）**：名字走仓库既有的临时表规范
//!   （`temp_table::generate_unique_name(TempTableSource::Insight, 描述)` → `tmp_i_…`），
//!   不自己拼前缀——TTL / 上限 / 按来源清理全靠那个前缀识别；
//! - **形态 II（结果集二次分析）**：名字由调用方给（用户看得见的那张表），
//!   这里只做标识符校验。
//!
//! # 已知边界（不装作没有）
//!
//! 会话把 `Connection` 藏在私有 `Mutex` 里，只对外给 `run`。所以这一层**只建表**，
//! 不做 [`temp_table`] 的 TTL 登记（登记需要 `&Connection`）。后果：表随会话存活，
//! 会话在进程级缓存里长期存在时不会自己消失。清理责任的落点：
//!
//! - 会话销毁（`drop_session` / 进程退出）——自动；
//! - 宿主显式 [`drop_materialized`]——洞察重跑、结果集被丢弃时该调它。
//!
//! 要把它彻底收进 TTL 体系，出路是给两种会话各加一个 `with_connection(closure)`
//! 只读借用入口，让内核能调 `temp_table::create_analysis_temp_table_as`。那会动会话
//! 的公开面，留给「刀 5 接洞察」时按实际需要做。
//!
//! 2026-11-15 进展：那个「按实际需要」的落点找到了，但换了个做法——不加借用入口，而是
//! 在**需要时把结果搬进单例**（[`move_sample_to_process_db`]：会话侧 `COPY … TO parquet`
//! → 单例侧 `CREATE TABLE AS read_parquet`，建完即登记）。原边界仍在：会话内的表仍不登记；
//! 搬过来的那张是登记表，跟着 TTL 走。搬运按需触发（洞察要在单例里看样本时才搬），
//! 不做「每次都搬」——那会把加速档的省内存意义抹掉。

use shared::error::{CommonError, CoreError};

use crate::duckdb::temp_table::{generate_unique_name, TempTableSource};

use super::kernel::SessionHandle;

/// 物化落在 **内存 catalog** 上。
///
/// 会话的默认 catalog 指向只读挂载的源，未限定名建表会被 DuckDB 拒（实测：
/// `Cannot execute statement of type "CREATE" on database "rds_src" which is attached
/// in read-only mode!`）。所以建表一律写 `memory.<name>`，**返回值也带这个前缀**——
/// 调用方拿裸名去查会报「表不存在」，限定名的责任在物化这一侧。
pub const MATERIALIZE_CATALOG: &str = "memory";

/// 形态 I 的表名：走仓库临时表规范（`tmp_i_<描述>_<时间戳>_<随机>`）。
///
/// `description` 用「源的可读名」（表名 / 文件名），只保留字母数字下划线。
pub fn sample_table_name(description: &str) -> String {
    generate_unique_name(TempTableSource::Insight, description)
}

/// 表名合法性：只允许 `[A-Za-z_][A-Za-z0-9_]*`。
///
/// 这条校验不是洁癖：表名会拼进 `CREATE TABLE {name} AS …`，允许引号 / 空格 / 分号
/// 就等于把注入面开给了「用户给表起名」这个入口。
fn validate_identifier(name: &str) -> Result<(), CoreError> {
    if name.is_empty() {
        return Err(invalid("表名不能为空".to_string()));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap_or_default();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(invalid(format!(
            "表名 {name} 不合法：首字符只能是字母或下划线"
        )));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(invalid(format!(
            "表名 {name} 不合法：只允许字母、数字、下划线（不要引号 / 空格 / 分号）"
        )));
    }
    Ok(())
}

/// 在会话里建（或替换）一张表，内容来自 `select_sql`；返回 `memory.<name>`。
///
/// 用 `CREATE OR REPLACE` 而不是 `CREATE`：同一张表重复物化是常态（用户来回点），
/// 撞名报错只会把「重复」包装成「失败」。
pub fn materialize_in_session(
    handle: &SessionHandle,
    name: &str,
    select_sql: &str,
) -> Result<String, CoreError> {
    if !handle.has_session() {
        return Err(invalid(
            "源库直连档不能物化：没有 DuckDB 会话，取样应走驱动拉数".to_string(),
        ));
    }
    validate_identifier(name)?;
    let select = select_sql.trim();
    if select.is_empty() {
        return Err(invalid("物化需要一条 SELECT：空 SQL 建不出表".to_string()));
    }
    // 尾分号会让 `CREATE … AS <sql>;` 变成两条语句，DuckDB 直接拒（多语句 API 才行）
    let select = select.trim_end_matches(';').trim_end();
    handle.run(&format!(
        "CREATE OR REPLACE TABLE {MATERIALIZE_CATALOG}.{name} AS {select}"
    ))?;
    Ok(format!("{MATERIALIZE_CATALOG}.{name}"))
}

/// 形态 I 的便捷入口：按描述生成规范表名再物化。
///
/// **按执行面分派**，因为「样本落在哪」决定了能不能登记：
///
/// | 执行面 | 落地 | 登记 |
/// | --- | --- | --- |
/// | `ProcessDb`（洞察 / 导出） | 进程单例的 `tmp_i_` 表 | ✅ `create_analysis_temp_table_as`（TTL / 上限现成） |
/// | `Accelerated` / `Federated` | 会话内的 `memory.tmp_i_` 表 | ❌ 见模块头的已知边界 |
///
/// 结果集二次分析这类「表本来就该留在我这条会话里」的消费方才直接用
/// [`materialize_in_session`]，不要经过这里。
pub fn materialize_sample(
    handle: &SessionHandle,
    description: &str,
    select_sql: &str,
) -> Result<String, CoreError> {
    let select = select_sql.trim().trim_end_matches(';').trim_end();
    if select.is_empty() {
        return Err(invalid("物化需要一条 SELECT：空 SQL 建不出表".to_string()));
    }
    match handle {
        // 进程单例：走仓库的分析临时表入口，建表即登记（洞察取样就是这个形态）
        SessionHandle::ProcessDb(conn) => {
            let guard = conn
                .lock()
                .map_err(|e| invalid(format!("DuckDB 单例锁中毒: {e}")))?;
            crate::duckdb::temp_table::create_analysis_temp_table_as(&guard, select, description)
        }
        _ => {
            let name = sample_table_name(description);
            materialize_in_session(handle, &name, select)
        }
    }
}

/// 把**会话内**的样本表搬进进程单例，并登记成规范的 `tmp_i_` 表。
///
/// 为什么需要这条：加速 / 联邦档的形态 I 只能落在会话自己的 `memory.tmp_i_`（见模块头），
/// 而洞察的下游（EDA / 图表 / 导出）读的是**进程单例**那台库。两张表在不同实例里，
/// 名字再像也互相看不见——所以「在加速档取样」这件事，要么下游跟着换连接，
/// 要么像这里一样把结果搬过来。
///
/// 搬运走 SQL 而不是 Rust 值装箱：会话只对外给 `run`（连接藏在私有 `Mutex`），
/// 所以是「会话侧 `COPY … TO parquet` → 单例侧 `CREATE TABLE AS read_parquet`」，
/// 类型（DECIMAL / 日期）原样过一遍。Parquet 中转文件是临时文件，用完即删。
///
/// 边界：**只搬一张表，不搬「会话」**。会话里的其它中间表仍留在会话侧；
/// 要清它，仍用 [`drop_materialized`] 对原会话调。
pub fn move_sample_to_process_db(
    handle: &SessionHandle,
    session_table: &str,
    description: &str,
) -> Result<String, CoreError> {
    if let SessionHandle::ProcessDb(_) = handle {
        return Err(invalid(
            "这张表已经在进程单例上：无需搬运（直接按裸名使用即可）".to_string(),
        ));
    }
    if !handle.has_session() {
        return Err(invalid(
            "源库直连档没有 DuckDB 会话：无从搬运（取样应走驱动拉数）".to_string(),
        ));
    }
    let name = session_table
        .strip_prefix(&format!("{MATERIALIZE_CATALOG}."))
        .unwrap_or(session_table);
    validate_identifier(name)?;

    // 中转文件：唯一名 + 放在系统临时目录；DuckDB 的字符串字面量里反斜杠不转义，
    // 所以路径统一成正斜杠（Windows 上也认），单引号按 SQL 口径翻倍。
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let parquet = std::env::temp_dir().join(format!("rds_sample_{}.parquet", &unique[..12]));
    let literal = parquet.to_string_lossy().replace('\\', "/").replace('\'', "''");

    handle
        .run(&format!(
            "COPY (SELECT * FROM {MATERIALIZE_CATALOG}.{name}) TO '{literal}' (FORMAT PARQUET)"
        ))
        .map_err(|e| invalid(format!("导出会话样本到中转文件失败: {e}")))?;

    let landed = {
        let singleton = crate::services::duckdb_service::DuckDbService::get_or_create_duckdb()
            .map_err(|e| invalid(format!("取内存分析库失败: {e}")))?;
        let guard = singleton
            .lock()
            .map_err(|e| invalid(format!("DuckDB 单例锁中毒: {e}")))?;
        crate::duckdb::temp_table::create_analysis_temp_table_as(
            &guard,
            &format!("SELECT * FROM read_parquet('{literal}')"),
            description,
        )
    };
    // 中转文件无论成败都不留：失败时留着只会攒垃圾（排查靠错误信息，不靠这个文件）
    let _ = std::fs::remove_file(&parquet);
    landed
}

/// 载入：把一条 `SELECT` 的结果落成进程单例里的 `tmp_l_*` 表（建表即登记）。
///
/// 与 [`materialize_sample`] 的区别是**来源档与寿命分档**：那份是洞察取样
/// （`tmp_i_`、TTL 管着），这份是数据对象面板「载入内存…」的落点（`tmp_l_`、
/// 无 TTL、随项目切换 / 进程退出清掉）。铁律第 12/34/35 条。
pub fn load_select_into_process_db(
    select_sql: &str,
    description: &str,
) -> Result<String, CoreError> {
    let select = select_sql.trim().trim_end_matches(';').trim();
    if select.is_empty() {
        return Err(invalid("载入需要一条 SELECT：空 SQL 建不出表".to_string()));
    }
    let singleton = crate::services::duckdb_service::DuckDbService::get_or_create_duckdb()
        .map_err(|e| invalid(format!("取内存分析库失败: {e}")))?;
    let guard = singleton
        .lock()
        .map_err(|e| invalid(format!("DuckDB 单例锁中毒: {e}")))?;
    crate::duckdb::temp_table::create_analysis_temp_table_as_for(
        &guard,
        select,
        description,
        crate::duckdb::temp_table::TempTableSource::Load,
    )
}

/// 载入（跨实例）：源表在**会话**里（联邦 / 加速档），把它搬进进程单例的 `tmp_l_*`。
///
/// 与 [`move_sample_to_process_db`] 同一条搬运路线（会话侧 `COPY … TO parquet` →
/// 单例侧 `CREATE TABLE AS read_parquet`），区别只有两点：
/// - 源表由**限定名**给（`别名.schema.表`），不是会话里的 `memory.xxx`；
/// - 落到 `tmp_l_`（载入档），不是 `tmp_i_`（洞察取样）。
///
/// `run` 是「在这条会话上执行一句 SQL」的闭包：会话对象（`AccelSession` /
/// `FederatedSession`）各自藏着自己的 `Connection`，引擎不在这里替它们选类型。
pub fn load_qualified_from_session(
    qualified: &str,
    description: &str,
    run: impl Fn(&str) -> Result<(), CoreError>,
) -> Result<String, CoreError> {
    let qualified = qualified.trim();
    if qualified.is_empty() {
        return Err(invalid("载入需要限定名（别名.schema.表）".to_string()));
    }
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let parquet = std::env::temp_dir().join(format!("rds_load_{}.parquet", &unique[..12]));
    let literal = parquet.to_string_lossy().replace('\\', "/").replace('\'', "''");
    // 中转文件在会话侧写、单例侧读：路径字面量同上一条，统一正斜杠 + 引号翻倍
    let copy = format!("COPY (SELECT * FROM {qualified}) TO '{literal}' (FORMAT PARQUET)");
    if let Err(error) = run(&copy) {
        let _ = std::fs::remove_file(&parquet);
        return Err(invalid(format!("把源表导出到中转文件失败: {error}")));
    }
    let landed = load_select_into_process_db(
        &format!("SELECT * FROM read_parquet('{literal}')"),
        description,
    );
    // 中转文件无论成败都不留（同 `move_sample_to_process_db` 的口径）
    let _ = std::fs::remove_file(&parquet);
    landed
}

/// 归档默认表名：`<源>_<表>`（铁律第 35 条）。
///
/// 用持久名的清洗尺子（保留中文，只换掉 DuckDB 建表会拒的字符），**不加** `tmp_` / `res_`
/// 任何前缀：归档出来的表是项目分析库里的正式表，名字本身就是资产名，加前缀等于让用户
/// 在资产列表里认一个自己没起过的名字。撞名由调用方在对话框里改名，这里不负责去重。
pub fn archive_table_name(source: &str, table: &str) -> String {
    let clean = crate::duckdb::temp_table::sanitize_persistent_description;
    // 空的一侧**不参与拼接**：清洗尺子对空串会兜底成 `tmp`，直接拼就会变成 `tmp_orders`
    // 这种既不像源名、也不像表名的东西。
    if source.trim().is_empty() {
        return clean(table);
    }
    if table.trim().is_empty() {
        return clean(source);
    }
    let source = clean(source);
    let table = clean(table);
    format!("{source}_{table}")
}

/// 归档（跨实例）：把一条 `SELECT` 的结果写进**项目分析库**里的一张持久表，返回行数。
///
/// 与 [`load_qualified_from_session`] 同一条搬运路线（源侧 `COPY … TO parquet` →
/// 目标库 `CREATE TABLE AS read_parquet`），区别只在**落点与寿命**：那份是进程单例的
/// `tmp_l_`（会话级、不登记），这份是 `.RSmeta/analytics.duckdb` 里的永久表（随后由调用方
/// 登记成表型资产，两步不可分，铁律第 11/35 条）。
///
/// 目标连接由调用方给：项目分析库的写连接归 `ProjectDatabaseManager` 按次开关，
/// 引擎不替它决定生命周期。已存在同名表 → `CREATE TABLE` 直接失败（不覆盖别人的持久表）。
pub fn archive_select_into(
    select_sql: &str,
    dest_table: &str,
    run: impl Fn(&str) -> Result<(), CoreError>,
    dest: &duckdb::Connection,
) -> Result<u64, CoreError> {
    let select = select_sql.trim().trim_end_matches(';').trim();
    if select.is_empty() {
        return Err(invalid("归档需要一条 SELECT：空 SQL 建不出表".to_string()));
    }
    let dest_table = dest_table.trim();
    if dest_table.is_empty() {
        return Err(invalid("归档需要目标表名".to_string()));
    }
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let parquet = std::env::temp_dir().join(format!("rds_archive_{}.parquet", &unique[..12]));
    let literal = parquet.to_string_lossy().replace('\\', "/").replace('\'', "''");

    let copy = format!("COPY ({select}) TO '{literal}' (FORMAT PARQUET)");
    if let Err(error) = run(&copy) {
        let _ = std::fs::remove_file(&parquet);
        return Err(invalid(format!("把源表导出到中转文件失败: {error}")));
    }

    let target = crate::duckdb::temp_table::quote_ident(dest_table);
    let written = (|| -> Result<u64, CoreError> {
        dest.execute_batch(&format!(
            "CREATE TABLE {target} AS SELECT * FROM read_parquet('{literal}')"
        ))
        .map_err(|e| {
            let text = e.to_string();
            if text.contains("already exists") || text.contains("已存在") {
                invalid(format!(
                    "项目分析库已存在表 {dest_table}：换一个名字再归档（不覆盖别人的持久表）"
                ))
            } else {
                invalid(format!("在项目分析库建表 {dest_table} 失败: {text}"))
            }
        })?;
        let count = dest
            .query_row(&format!("SELECT count(*) FROM {target}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|e| invalid(format!("统计归档表行数失败: {e}")))?;
        Ok(count.max(0) as u64)
    })();

    // 中转文件无论成败都不留（同 `move_sample_to_process_db` 的口径）
    let _ = std::fs::remove_file(&parquet);
    written
}

/// 丢掉一张物化表（幂等）。`qualified` 接受 `memory.x` 或裸名 `x`。
///
/// 前缀守卫与 `temp_table::drop_temp_table` 同口径：只允许删 `tmp_i_` / `tmp_q_` / `tmp_l_`
/// （洞察取样 / 查询结果 / 载入），放开就等于给了一把「传什么名字删什么表」的刀。
pub fn drop_materialized(handle: &SessionHandle, qualified: &str) -> Result<(), CoreError> {
    let name = qualified
        .strip_prefix(&format!("{MATERIALIZE_CATALOG}."))
        .unwrap_or(qualified);
    let allowed = ["tmp_i_", "tmp_q_", "tmp_l_"];
    if !allowed.iter().any(|prefix| name.starts_with(prefix)) {
        return Err(invalid(format!(
            "拒绝删除非临时表：{name}（只允许 {} 前缀）",
            allowed.join(" / ")
        )));
    }
    // 源库档没有会话：前缀校验照做（守卫不能被「没有会话」这条捷径跳过），之后才是空操作
    if !handle.has_session() {
        return Ok(());
    }
    // 进程单例：走登记版删除，否则登记表会与库里实际张数对不上（K16 ④ 的计数信号就失真）
    if let SessionHandle::ProcessDb(conn) = handle {
        let guard = conn
            .lock()
            .map_err(|e| invalid(format!("DuckDB 单例锁中毒: {e}")))?;
        return match name.starts_with("tmp_l_") {
            true => crate::duckdb::temp_table::drop_loaded_temp_table(&guard, name),
            false => crate::duckdb::temp_table::drop_analysis_temp_table(&guard, name),
        };
    }
    handle.run(&format!(
        "DROP TABLE IF EXISTS {MATERIALIZE_CATALOG}.{name}"
    ))?;
    Ok(())
}

fn invalid(message: String) -> CoreError {
    CoreError::common(CommonError::General(message))
}

#[cfg(test)]
mod tests {
    use super::{drop_materialized, materialize_in_session, materialize_sample, sample_table_name};
    use crate::analysis::kernel::SessionHandle;

    #[test]
    fn sample_name_follows_the_repo_temp_table_convention() {
        let name = sample_table_name("orders");
        assert!(name.starts_with("tmp_i_"), "实际 {name}");
        assert!(name.contains("orders"), "描述要进名字，便于人在库里认出来");
        // 每次生成都必须不同：同一秒内建两张也不会撞名（仓库口径：时间戳 + 随机尾巴）
        assert_ne!(sample_table_name("orders"), sample_table_name("orders"));
    }

    #[test]
    fn sample_name_sanitizes_description() {
        let name = sample_table_name("a b;drop table x");
        assert!(name.starts_with("tmp_i_"));
        assert!(!name.contains(' '), "空格不应进表名：{name}");
        assert!(!name.contains(';'), "分号不应进表名：{name}");
    }

    #[test]
    fn source_channel_cannot_materialize() {
        let err = materialize_in_session(&SessionHandle::None, "t", "select 1").unwrap_err();
        assert!(format!("{err}").contains("没有 DuckDB 会话"));
    }

    #[test]
    fn identifier_validation_rejects_bad_names() {
        assert!(super::validate_identifier("orders_2024").is_ok());
        assert!(super::validate_identifier("_tmp").is_ok());
        assert!(super::validate_identifier("").is_err());
        assert!(super::validate_identifier("1abc").is_err());
        assert!(super::validate_identifier("a b").is_err());
        assert!(super::validate_identifier("a;b").is_err());
    }

    #[test]
    fn sample_on_the_process_singleton_is_registered() {
        // 形态 I 的真形态：进程单例上的 tmp_i_ 表，建完即登记（TTL 才认得出它）
        let plan = crate::analysis::mount::plan_for(
            &crate::analysis::channel::AnalysisChannel::ProcessDuckDb,
            &[],
            None,
            "orders",
        )
        .unwrap();
        let handle = crate::analysis::kernel::open(&plan, &Default::default()).unwrap();
        let table = materialize_sample(&handle, "source_sample", "select 1 as a").expect("物化");
        assert!(table.starts_with("tmp_i_"), "实际 {table}");
        let result = handle.run(&format!("select count(*) from {table}")).expect("查");
        assert_eq!(result.rows.len(), 1);
        // 删除要走登记版：删完再查应报「表不存在」
        drop_materialized(&handle, &table).expect("删除");
        assert!(handle.run(&format!("select count(*) from {table}")).is_err());
    }

    #[test]
    fn materialize_sample_requires_a_session() {
        assert!(materialize_sample(&SessionHandle::None, "orders", "select 1").is_err());
    }

    #[test]
    fn loading_lands_a_tmp_l_table_on_the_process_singleton() {
        // 铁律第 35 条：载入 = `tmp_l_`，落进程单例、不登记资产
        let landing = super::load_select_into_process_db("select 1 as a", "orders_shop")
            .expect("载入应当成功");
        assert!(landing.starts_with("tmp_l_"), "载入档命名：{landing}");
        assert!(
            crate::analysis::result_set::list_loaded_tables()
                .expect("列载入表")
                .contains(&landing),
            "载入表要能被表段列出来：{landing}"
        );
        let handle = crate::analysis::result_set::open_for_result_set("载入测试").expect("取单例");
        drop_materialized(&handle, &landing).expect("删除");
        assert!(!crate::analysis::result_set::list_loaded_tables()
            .expect("列载入表")
            .contains(&landing));
    }

    #[test]
    fn loading_rejects_an_empty_select() {
        assert!(super::load_select_into_process_db("   ", "x").is_err());
        assert!(super::load_qualified_from_session("", "x", |_| Ok(())).is_err());
    }

    #[test]
    fn drop_requires_a_temp_prefix() {
        // 前缀守卫：不能拿它删用户表
        let err = drop_materialized(&SessionHandle::None, "memory.orders").unwrap_err();
        assert!(format!("{err}").contains("拒绝删除非临时表"));
        // 源库档没有会话 → 幂等成功（没什么可删）
        assert!(drop_materialized(&SessionHandle::None, "memory.tmp_i_x").is_ok());
    }

    /// 归档默认名 = `<源>_<表>`，中文保留、不带 `tmp_` / `res_` 前缀（铁律第 35 条）。
    #[test]
    fn archive_name_is_source_then_table() {
        assert_eq!(super::archive_table_name("shop", "orders"), "shop_orders");
        let zh = super::archive_table_name("订单 库", "明细/表");
        assert!(zh.contains('订') && zh.contains('明'), "中文要留：{zh}");
        assert!(!zh.contains('/') && !zh.contains(' '), "非法字符要清：{zh}");
        // 源名空时只剩表名，不造出「_表」这种以分隔符开头的名字
        assert_eq!(super::archive_table_name("", "orders"), "orders");
    }

    /// 归档落的是**目标库**里的持久表；同名再来必须失败（不覆盖）。
    #[test]
    fn archiving_lands_a_table_in_the_target_db_and_refuses_to_overwrite() {
        let unique = uuid::Uuid::new_v4().simple().to_string();
        let dir = std::env::temp_dir().join(format!("rds_archive_{}", &unique[..8]));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let target = dir.join("analytics.duckdb");

        let source = duckdb::Connection::open_in_memory().expect("源连接");
        source
            .execute_batch("CREATE TABLE shop_orders AS SELECT 1 AS a UNION ALL SELECT 2")
            .expect("建源表");
        let dest = duckdb::Connection::open(&target).expect("目标库");

        let run = |sql: &str| {
            source.execute_batch(sql).map_err(|error| {
                shared::error::CoreError::common(shared::error::CommonError::General(
                    error.to_string(),
                ))
            })
        };
        let rows = super::archive_select_into(
            "SELECT * FROM main.shop_orders",
            "shop_orders",
            run,
            &dest,
        )
        .expect("归档");
        assert_eq!(rows, 2, "行数要如实回报");

        let again =
            super::archive_select_into("SELECT * FROM main.shop_orders", "shop_orders", run, &dest)
                .unwrap_err();
        assert!(
            format!("{again}").contains("已存在"),
            "同名归档必须报「已存在」而不是覆盖：{again}"
        );

        drop(dest);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn archiving_rejects_an_empty_select_or_table() {
        let dest = duckdb::Connection::open_in_memory().expect("目标库");
        assert!(super::archive_select_into("  ", "x", |_| Ok(()), &dest).is_err());
        assert!(super::archive_select_into("select 1", "  ", |_| Ok(()), &dest).is_err());
    }
}
