//! ER 图的**取数边界**（`er` 模块里唯一有 I/O 的一层）。
//!
//! 只调用宿主已落地的元数据服务（[`MetadataService`]：`list_tables` / `list_columns_batch` /
//! `list_constraints`），**不直连库、不碰凭据**（对齐 ER 边界硬约束）。本函数只应在
//! **后台任务线程**被调用（`crate::nav_jobs::Job::LoadErDiagram`）——UI 线程绝不直接 await
//! 元数据查询（会冻界面，与 M4 导航同一纪律）。
//!
//! 取数策略：一次 `list_tables` 拿清单（表 + 视图，含 `kind`），一次 `list_columns_batch`
//! 拿全部列，再**逐表**取约束（没有 schema 级约束批量接口）。逐表约束是 N 次往返，
//! 因此以 [`MAX_TABLES`] 收口，超限**如实报错**而不是静默画半张图。

use std::collections::HashMap;

use engine::driver::traits::ColumnDetail;

use crate::er::model::{build_diagram, ErDiagram, TableInput};
use crate::metadata_service::MetadataService;

/// 单张 ER 图最多纳入的表 / 视图数量。
///
/// 为什么要上限：每张表要单独取一次约束（无 schema 级批量接口），无上限时一个数百表的
/// schema 会把后台串行队列占住很久，且首次绘制也读不过来。超限时给一条可执行的提示
/// （收窄到单个 schema），而不是画一张读不了也画不完的图。
pub const MAX_TABLES: usize = 120;

/// 取一个 schema 的表 / 视图 / 外键，组装成只读 ER 图。
///
/// 返回的 `Err` 一律是**可直接显示**的中文文案（后台任务把它原样回填给面板）。
pub async fn load_diagram(
    conn_id: &str,
    catalog: &str,
    schema: &str,
) -> Result<ErDiagram, String> {
    let svc = MetadataService::new(engine::get_connection_manager().clone());

    let tables = svc
        .list_tables(conn_id, catalog, schema)
        .await
        .map_err(|e| e.to_string())?;
    if tables.is_empty() {
        return Ok(ErDiagram::default());
    }
    if tables.len() > MAX_TABLES {
        return Err(format!(
            "该 schema 有 {} 张表 / 视图，超过 ER 视图上限 {}；请收窄范围后重试",
            tables.len(),
            MAX_TABLES
        ));
    }

    let names: Vec<String> = tables.iter().map(|t| t.name.clone()).collect();
    let batches = svc
        .list_columns_batch(conn_id, catalog, schema, &names)
        .await
        .map_err(|e| e.to_string())?;
    // batch 按「传入表序」分组，但这里按**表名**建映射，不依赖顺序假设。
    let mut columns_by_name: HashMap<String, Vec<ColumnDetail>> = HashMap::new();
    for (name, cols) in batches {
        columns_by_name.insert(name, cols);
    }

    let mut inputs: Vec<TableInput> = Vec::with_capacity(tables.len());
    for t in &tables {
        // 约束取不到就**如实失败**（面板显示原因）：悄悄丢约束会画出一张「没有外键」的图，
        // 比报错更误导。列级引用（`ColumnDetail::references`）只是补充，不替代约束。
        let constraints = svc
            .list_constraints(conn_id, catalog, schema, &t.name)
            .await
            .map_err(|e| format!("取表 {} 的约束失败：{e}", t.name))?;
        inputs.push(TableInput {
            schema: schema.to_string(),
            name: t.name.clone(),
            kind: t.kind.clone(),
            columns: columns_by_name.remove(&t.name).unwrap_or_default(),
            constraints,
        });
    }

    Ok(build_diagram(&inputs))
}
