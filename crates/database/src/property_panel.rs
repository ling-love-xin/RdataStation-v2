//! rds-database — 对象属性面板取数（M4 数据生产者）。
//!
//! 领域模型 / 数据容器 / 描述模型 / 属性源契约在 `rds-property`
//! （见 `docs/architecture/property/property-module-spec.md` §16）。
//! 本模块是 **M4 场景的通用骨架**：
//! - 驱动无关取数：列 / 索引 / 约束 / DDL / 数据（`metadata_service` 内省）；
//! - 属性源分派：`property_providers::global_registry().provider(driver, kind)`；
//!   驱动特异部分（属性行 / 洞察 / 类别 / 编辑 SQL）在 provider，**新增驱动不改本文件**；
//! - 未注册 provider 的对象类型回退通用展示（fallback）。
//! 视图形态为编辑区右侧停靠面板（`workbench/panels/editor.rs`）。

use crate::metadata_service::MetadataService;
use crate::model::{NavSource, PropertyRef};
use crate::property_providers::global_registry;
use crate::table_data::{self, TableDataRequest};
use engine::driver::traits::SchemaObjectKind;
use property::{
    ObjectOverview, ObjectProperties, PageId, PropertyKind, PropertyRow, PropertySection,
    PropertyTable, PropertyTargetRef, ProviderInput, ProviderStats,
    TableDataPage,
};
use shared::error::CoreError;

/// 只读文本行（DBeaver 只读属性；id 取显示名）。
fn row(label: &str, value: impl Into<String>) -> PropertyRow {
    PropertyRow::read_only(label, value)
}

fn qualify(ref_: &PropertyRef) -> String {
    match ref_.kind {
        PropertyKind::Column => {
            let parent = ref_.parent.clone().unwrap_or_default();
            if parent.is_empty() {
                ref_.name.clone()
            } else {
                format!("{parent}.{}", ref_.name)
            }
        }
        _ => crate::sql_gen::qualified_name(
            ref_.catalog.as_deref(),
            ref_.schema.as_deref(),
            &ref_.name,
        ),
    }
}

fn source_label(source: NavSource) -> String {
    match source {
        NavSource::Project => "项目 (P)".to_string(),
        NavSource::Global => "全局 (G)".to_string(),
        NavSource::Shared => "共享 (GP)".to_string(),
    }
}

/// `PropertyRef` → 属性源目标引用（`property` crate 的轻量结构，不依赖本 crate）。
fn to_target(ref_: &PropertyRef) -> PropertyTargetRef {
    PropertyTargetRef {
        kind: ref_.kind,
        name: ref_.name.clone(),
        catalog: ref_.catalog.clone(),
        schema: ref_.schema.clone(),
        parent: ref_.parent.clone(),
        conn_id: ref_.conn_id.clone(),
        source: source_label(ref_.source),
    }
}

/// 加载对象属性（实时内省；连接 / Catalog / Schema 无需查询）。
///
/// 流程：通用取数（列/索引/约束/DDL/数据）→ 属性源分派（provider 组装属性行/洞察/类别）→
/// 类别内容合并（categories 声明顺序 + 骨架取数内容）。
///
/// `data_req` 为 `Some` 时对表 / 视图附带「数据」页签的分页抓取
/// （`property_panel` 只编排：取数在 [`crate::table_data`]，与结果集只共享网格渲染层）。
pub async fn load_properties(
    metadata: &MetadataService,
    ref_: &PropertyRef,
    conn_label: &str,
    driver: &str,
    type_id: Option<&str>,
    db_type: Option<&str>,
    data_req: Option<&TableDataRequest>,
) -> Result<ObjectProperties, CoreError> {
    let source = source_label(ref_.source);
    let target = to_target(ref_);

    let object_type = match ref_.kind {
        PropertyKind::Connection => "连接",
        PropertyKind::Catalog => "Catalog",
        PropertyKind::Schema => "Schema",
        PropertyKind::Table => "BASE TABLE",
        PropertyKind::View => "VIEW",
        PropertyKind::Column => "COLUMN",
        PropertyKind::Routine => "ROUTINE",
        PropertyKind::Sequence => "SEQUENCE",
        PropertyKind::Trigger => "TRIGGER",
    }
    .to_string();

    let title = match ref_.kind {
        PropertyKind::Connection => conn_label.to_string(),
        _ => ref_.name.clone(),
    };

    let catalog = ref_.catalog.clone().unwrap_or_default();
    let schema = ref_.schema.clone().unwrap_or_default();

    // ── ① 通用取数（驱动无关，失败降级不影响主面板） ──────────────────────
    let mut columns: Vec<engine::driver::traits::ColumnDetail> = Vec::new();
    let mut indexes: Vec<engine::driver::traits::IndexDetail> = Vec::new();
    let mut constraints: Vec<engine::driver::traits::ConstraintDetail> = Vec::new();
    let mut column_detail: Option<engine::driver::traits::ColumnDetail> = None;
    let mut routine_source: Option<String> = None;
    let mut ddl_text: Option<String> = None;
    let mut data: Option<TableDataPage> = None;

    match ref_.kind {
        PropertyKind::Table | PropertyKind::View => {
            columns = metadata
                .list_columns(&ref_.conn_id, &catalog, &schema, &ref_.name)
                .await?;
            if ref_.kind == PropertyKind::Table {
                indexes = metadata
                    .list_indexes(&ref_.conn_id, &catalog, &schema, &ref_.name)
                    .await
                    .unwrap_or_default();
                constraints = metadata
                    .list_constraints(&ref_.conn_id, &catalog, &schema, &ref_.name)
                    .await
                    .unwrap_or_default();
            }
            ddl_text = metadata
                .get_table_ddl(&ref_.conn_id, &catalog, &schema, &ref_.name)
                .await
                .ok()
                .flatten();
        }
        PropertyKind::Column => {
            let parent = ref_.parent.clone().unwrap_or_default();
            let all = metadata
                .list_columns(&ref_.conn_id, &catalog, &schema, &parent)
                .await?;
            column_detail = all.into_iter().find(|c| c.name == ref_.name);
        }
        PropertyKind::Routine => {
            // 例程种类未知：先按过程取，未命中再按函数取（`PropertyRef` 不带种类）。
            routine_source = metadata
                .get_routine_source(
                    &ref_.conn_id,
                    &catalog,
                    &schema,
                    &ref_.name,
                    SchemaObjectKind::Procedure,
                )
                .await
                .ok()
                .flatten();
            if routine_source.is_none() {
                routine_source = metadata
                    .get_routine_source(
                        &ref_.conn_id,
                        &catalog,
                        &schema,
                        &ref_.name,
                        SchemaObjectKind::Function,
                    )
                    .await
                    .ok()
                    .flatten();
            }
        }
        _ => {}
    }
    if let Some(req) = data_req {
        if matches!(ref_.kind, PropertyKind::Table | PropertyKind::View) {
            data = Some(
                table_data::load_table_data(
                    metadata,
                    &ref_.conn_id,
                    &catalog,
                    &schema,
                    &ref_.name,
                    driver,
                    req,
                )
                .await?,
            );
        }
    }

    // ── ② 属性源分派（新增数据库 = 注册 provider，本文件不改） ──────────────
    // 分派键 = **数据库类型 id**（`drivers.type_id`，如 `postgresql`）；`driver`
    // 只是驱动 id / 显示名（fallback 的「驱动」行用）。type_id 缺失时回退 driver
    // （旧调用方 / 测试直接传类型名）。
    let stats = ProviderStats {
        column_count: columns.len(),
        index_count: indexes.len(),
        constraint_count: constraints.len(),
    };
    let provider =
        global_registry().provider(type_id.unwrap_or(driver), ref_.kind);

    // 调用方统计（洞察指标）：连接元信息（在线状态 / 服务端版本 / URL / 描述 /
    // 认证·环境·网络引用） + provider 声明的懒加载查询（行估计 / 表大小等）。
    // 连接对象 provider 从 `counts` 组装常规 + 引用行（引用显示来源 ID 且只读，
    // 2026-10-24 连接对象属性组装 P0-3）；`version` 供 duckdb/mysql 连接行读。
    let mut counts: Vec<(&str, String)> = Vec::new();
    if let Some(info) = metadata
        .manager()
        .get_connection_info(&ref_.conn_id)
        .await
    {
        counts.push(("connected", "true".to_string()));
        if let Some(v) = info.server_version {
            counts.push(("server_version", v.clone()));
            counts.push(("version", v));
        }
        counts.push(("url", info.url));
        if let Some(v) = info.description {
            if !v.is_empty() {
                counts.push(("description", v));
            }
        }
        if let Some(v) = info.auth_method.as_deref() {
            if !v.is_empty() {
                counts.push(("auth_method", v.to_string()));
            }
        }
        if let Some(v) = info.auth_config_id.as_deref() {
            if !v.is_empty() {
                counts.push(("auth_ref", v.to_string()));
            }
        }
        if let Some(v) = info.environment_id.as_deref() {
            if !v.is_empty() {
                counts.push(("env_ref", v.to_string()));
            }
        }
        if let Some(v) = info.network_config_id.as_deref() {
            if !v.is_empty() {
                counts.push(("network_ref", v.to_string()));
            }
        }
        // P0-3 深化（2026-10-25）：分节显示名称 / 类型 / 策略覆盖，而非裸引用 ID——
        // 经全局管理器单例解析（未初始化 / 查无档案时降级为 ID 行，不失败）。
        if let Some(global) = engine::migration::global_init::get_global_db_manager() {
            if let Some(id) = info.auth_config_id.as_deref() {
                if let Ok(Some(a)) = global.get_auth_config(id).await {
                    counts.push(("auth_name", a.name.clone().unwrap_or_else(|| id.to_string())));
                    if !a.auth_type.is_empty() {
                        counts.push(("auth_type", a.auth_type.clone()));
                    }
                }
            }
            if let Some(id) = info.network_config_id.as_deref() {
                if let Ok(Some(n)) = global.get_network_config(id).await {
                    counts.push(("network_name", n.name.clone().unwrap_or_else(|| id.to_string())));
                    if !n.network_type.is_empty() {
                        counts.push(("network_type", n.network_type.clone()));
                    }
                }
            }
            if let Some(id) = info.environment_id.as_deref() {
                if let Ok(Some(e)) = global.get_environment(id).await {
                    counts.push(("env_name", e.name.clone()));
                    if let Some(c) = e.color.as_deref() {
                        if !c.is_empty() {
                            counts.push(("env_color", c.to_string()));
                        }
                    }
                }
                if let Ok(policies) = global.list_environment_policies(id).await {
                    let enabled: Vec<String> = policies
                        .iter()
                        .filter(|p| p.enabled)
                        .map(|p| p.policy_type.clone())
                        .collect();
                    if !enabled.is_empty() {
                        counts.push(("env_policies", enabled.join(", ")));
                    }
                }
            }
        }
    }
    // Select 候选（骨架执行 provider 的补充查询时按行收集首列全量）。
    let mut candidates: Vec<(&str, Vec<String>)> = Vec::new();
    if let Some(p) = provider {
        let pre_input = ProviderInput {
            target: &target,
            conn_label,
            type_id,
            db_type,
            stats,
            counts: &counts,
            candidates: &candidates,
            detail: None,
        };
        // 补充查询结果按行数分流：单行 → 洞察 counts；多行 → Select 候选（首列全量）。
        for q in p.extra_queries(&pre_input) {
            if let Ok(res) = metadata
                .execute_query(&ref_.conn_id, ref_.catalog.as_deref().unwrap_or("main"), &q.sql)
                .await
            {
                if res.rows.is_empty() {
                    continue;
                }
                let values: Vec<String> = res
                    .rows
                    .iter()
                    .filter_map(|r| r.first().map(|v| v.to_string()))
                    .collect();
                if values.len() == 1 {
                    counts.push((q.key, values[0].clone()));
                } else if !values.is_empty() {
                    candidates.push((q.key, values));
                }
            }
        }
    }

    let (properties, overview, categories) = match provider {
        Some(p) => {
            let input = ProviderInput {
                target: &target,
                conn_label,
                type_id,
                db_type,
                stats,
                counts: &counts,
                candidates: &candidates,
                detail: column_detail
                    .as_ref()
                    .map(|c| c as &dyn std::any::Any),
            };
            (
                p.property_defs(&input),
                p.overview(&input),
                p.categories(&input),
            )
        }
        // 未注册 provider：通用展示（fallback，保留旧行为）。
        None => (
            fallback_properties(ref_, conn_label, driver, db_type, &source, &columns, &indexes, &constraints, &routine_source, &ddl_text),
            fallback_overview(ref_, &title, &object_type, &source, &stats),
            fallback_categories(),
        ),
    };

    // ── ③ 类别内容合并：categories 声明顺序 + 骨架取数内容 ────────────────
    let qualified = qualify(ref_);
    let caps = super::property_providers::caps_for(db_type);
    let mut sections: Vec<PropertySection> = Vec::new();
    for cat in categories {
        let table = sections_of(
            ref_.kind,
            cat.id,
            &qualified,
            &columns,
            &indexes,
            &constraints,
            &routine_source,
            &ddl_text,
            &caps,
        )
        .map(|s| s.table)
        .unwrap_or(cat.table);
        sections.push(PropertySection {
            id: cat.id,
            label: cat.label,
            table,
        });
    }

    Ok(ObjectProperties {
        title,
        object_type,
        source_label: source,
        overview,
        properties,
        sections,
        data,
    })
}

/// 骨架分区内容（列 / 索引 / 约束 / DDL / 源码——按类别 id 提供）。
#[allow(clippy::too_many_arguments)]
fn sections_of(
    kind: PropertyKind,
    page: PageId,
    qualified: &str,
    columns: &[engine::driver::traits::ColumnDetail],
    indexes: &[engine::driver::traits::IndexDetail],
    constraints: &[engine::driver::traits::ConstraintDetail],
    routine_source: &Option<String>,
    ddl_text: &Option<String>,
    caps: &super::property_providers::DriverCapabilities,
) -> Option<PropertySection> {
    match page {
        PageId::Columns => Some(PropertySection {
            id: PageId::Columns,
            label: format!("列 ({})", columns.len()),
            table: columns_table(columns, caps),
        }),
        PageId::Indexes => Some(PropertySection {
            id: PageId::Indexes,
            label: format!("索引 ({})", indexes.len()),
            table: indexes_table(indexes),
        }),
        PageId::Constraints => Some(PropertySection {
            id: PageId::Constraints,
            label: format!("约束 ({})", constraints.len()),
            table: constraints_table(constraints),
        }),
        PageId::Ddl => {
            if let Some(text) = ddl_text {
                Some(PropertySection {
                    id: PageId::Ddl,
                    label: "DDL（源版）".to_string(),
                    table: text_table("DDL", text),
                })
            } else if kind == PropertyKind::Table {
                // 取不到源（PostgreSQL 无 SHOW CREATE 等价物）→ 目录信息合成。
                let composed = crate::sql_gen::create_table_ddl(
                    qualified,
                    columns,
                    constraints,
                    indexes,
                );
                Some(PropertySection {
                    id: PageId::Ddl,
                    label: "DDL（合成）".to_string(),
                    table: text_table("DDL", &composed),
                })
            } else {
                None
            }
        }
        PageId::Source => routine_source.as_ref().map(|text| PropertySection {
            id: PageId::Source,
            label: "源码".to_string(),
            table: text_table("源码", text),
        }),
        _ => None,
    }
}

/// 未注册 provider 时的通用属性行（fallback，保留原行为）。
#[allow(clippy::too_many_arguments)]
fn fallback_properties(
    ref_: &PropertyRef,
    conn_label: &str,
    driver: &str,
    db_type: Option<&str>,
    source: &str,
    columns: &[engine::driver::traits::ColumnDetail],
    indexes: &[engine::driver::traits::IndexDetail],
    constraints: &[engine::driver::traits::ConstraintDetail],
    routine_source: &Option<String>,
    ddl_text: &Option<String>,
) -> Vec<PropertyRow> {
    let mut properties = Vec::new();
    match ref_.kind {
        PropertyKind::Connection => {
            properties.push(row("名称", conn_label));
            if let Some(t) = db_type {
                if !t.is_empty() {
                    properties.push(row("数据库类型", t));
                }
            }
            properties.push(row("归属域", source));
            properties.push(row("驱动", driver));
        }
        PropertyKind::Catalog => {
            properties.push(row("名称", ref_.name.clone()));
            properties.push(row("类型", "Catalog"));
            properties.push(row("归属域", source));
            properties.push(row("限定名", qualify(ref_)));
        }
        PropertyKind::Schema => {
            properties.push(row("名称", ref_.name.clone()));
            properties.push(row("类型", "Schema"));
            properties.push(row("归属域", source));
            properties.push(row("限定名", qualify(ref_)));
        }
        PropertyKind::Table | PropertyKind::View => {
            properties.push(row("限定名", qualify(ref_)));
            properties.push(row("类型", if ref_.kind == PropertyKind::Table { "表" } else { "视图" }));
            properties.push(row("列数", columns.len().to_string()));
            properties.push(row("归属域", source));
        }
        PropertyKind::Column => {
            properties.push(row("名称", ref_.name.clone()));
            properties.push(row("所属表", qualify(ref_)));
            properties.push(row("提示", "该驱动的列级属性未单独接入，当前为通用展示"));
        }
        PropertyKind::Sequence | PropertyKind::Trigger => {
            properties.push(row("名称", ref_.name.clone()));
            properties.push(row("限定名", qualify(ref_)));
            properties.push(row("类型", "序列/触发器"));
            properties.push(row("归属域", source));
        }
        PropertyKind::Routine => {
            properties.push(row("名称", ref_.name.clone()));
            properties.push(row("限定名", qualify(ref_)));
            properties.push(row("类型", "例程"));
            properties.push(row("归属域", source));
        }
    }
    let _ = (indexes, constraints, routine_source, ddl_text);
    properties
}

/// 未注册 provider 时的通用洞察（fallback）。
fn fallback_overview(
    _ref_: &PropertyRef,
    title: &str,
    object_type: &str,
    source: &str,
    stats: &ProviderStats,
) -> ObjectOverview {
    let mut cards = Vec::new();
    if stats.column_count > 0 {
        cards.push(property::OverviewCard { label: "列".into(), value: stats.column_count.to_string(), unit: String::new(), delta: None });
    }
    ObjectOverview {
        title: title.to_string(),
        subtitle: object_type.to_string(),
        status: property::OverviewStatus::Connected,
        status_text: "已加载".to_string(),
        cards,
        kv: vec![("作用域".to_string(), source.to_string())],
        note: String::new(),
        meta: vec![("作用域".to_string(), source.to_string())],
    }
}

/// 未注册 provider 时的通用类别（fallback：仅 General；未实现类别隐藏式不产出）。
fn fallback_categories() -> Vec<PropertySection> {
    vec![PropertySection {
        id: PageId::General,
        label: "通用".to_string(),
        table: PropertyTable { headers: Vec::new(), rows: Vec::new() },
    }]
}

/// 应用属性编辑（DBeaver `setPropertyValue` 对齐，2026-10-09）。
///
/// 编辑 SQL 由驱动 provider 生成（`ProviderProvider::apply_edit_sql`，仅 PostgreSQL
/// 列级编辑支持；其余驱动 / 对象类型明确报「暂不支持」）。返回实际执行的 SQL（渲染方展示）。
///
/// `property_id` 与属性行 id 一致（`Column.name` / `Column.comment` /
/// `Column.default_value` / `Column.nullable`）。
pub async fn apply_property_edit(
    metadata: &MetadataService,
    ref_: &PropertyRef,
    driver: &str,
    type_id: Option<&str>,
    property_id: &str,
    value: &str,
) -> Result<String, CoreError> {
    let catalog = ref_.catalog.clone().unwrap_or_default();
    let schema = ref_.schema.clone().unwrap_or_default();
    let target = to_target(ref_);

    // 列级编辑需要列明细（detail 传 provider）。
    let column_detail = if ref_.kind == PropertyKind::Column {
        let parent = ref_.parent.clone().unwrap_or_default();
        metadata
            .list_columns(&ref_.conn_id, &catalog, &schema, &parent)
            .await?
            .into_iter()
            .find(|c| c.name == ref_.name)
    } else {
        None
    };

    let provider = global_registry().provider(type_id.unwrap_or(driver), ref_.kind);
    let sql = match provider {
        Some(p) => {
            let input = ProviderInput {
                target: &target,
                conn_label: "",
                type_id,
                db_type: None,
                stats: ProviderStats::default(),
                counts: &[],
                candidates: &[],
                detail: column_detail
                    .as_ref()
                    .map(|c| c as &dyn std::any::Any),
            };
            p.apply_edit_sql(&input, property_id, value).map_err(|msg| {
                CoreError::database(shared::error::DatabaseError::Driver {
                    db_type: driver.to_string(),
                    operation: "apply_property_edit".to_string(),
                    source: msg,
                })
            })?
        }
        None => {
            // fallback：仅 PG 列级编辑（provider 未注册的驱动同样报不支持）。
            crate::sql_gen::alter_column_sql(
                driver,
                (!schema.is_empty()).then_some(schema.as_str()),
                ref_.parent.clone().unwrap_or_default().as_str(),
                &ref_.name,
                property_id,
                value,
            )?
        }
    };
    metadata.execute_query(&ref_.conn_id, &catalog, &sql).await?;
    Ok(sql)
}

/// 多行文本段落（源码 / DDL）：每行一条记录（单列），保留换行。
fn text_table(header: &str, text: &str) -> PropertyTable {
    let headers = vec![header.to_string()];
    let rows = text
        .lines()
        .map(|line| vec![line.to_string()])
        .collect::<Vec<_>>();
    // 空文本（或全空白）时给出占位，避免分区标题下无内容。
    let rows = if rows
        .iter()
        .all(|r| r.first().map(|s| s.trim().is_empty()).unwrap_or(true))
    {
        vec![vec!["（空）".to_string()]]
    } else {
        rows
    };
    PropertyTable { headers, rows }
}

fn columns_table(
    columns: &[engine::driver::traits::ColumnDetail],
    caps: &super::property_providers::DriverCapabilities,
) -> PropertyTable {
    // 「引用」一列同时承载两件事：拿得到目标就写 `表.列`，只拿得到「是外键」（复合外键 ——
    // 配对要按列序做，见 `utils::PG_TABLE_DETAIL_SQL` 的说明）就写 FK。
    // 2026-10-24 列字段增强（竞品分析必做 #5）：PG / MySQL 追加「标识 / 排序规则」列
    // （`identity_generation` / `collation` 真值已入 engine 列序契约 0..=12）；
    // SQLite / DuckDB 无这两个概念 → 按能力位裁剪，列结构不随驱动漂移。
    let mut headers = vec!["#", "名称", "类型"];
    if caps.identity_column {
        headers.push("标识");
    }
    if caps.collation_column {
        headers.push("排序规则");
    }
    headers.extend(["非空", "主键", "引用", "默认", "注释"]);
    let headers: Vec<String> = headers.into_iter().map(|s| s.to_string()).collect();
    let rows = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut cells = vec![
                // 驱动给了真序号就用它（内省结果被筛选 / 重排后数组下标会错位），没给才退回下标
                if c.ordinal > 0 {
                    c.ordinal.to_string()
                } else {
                    (i + 1).to_string()
                },
                c.name.clone(),
                c.data_type.clone(),
            ];
            if caps.identity_column {
                cells.push(c.identity_generation.clone().unwrap_or_default());
            }
            if caps.collation_column {
                cells.push(c.collation.clone().unwrap_or_default());
            }
            cells.extend(vec![
                if c.nullable { "" } else { "✓" }.to_string(),
                if c.is_primary_key { "PK" } else { "" }.to_string(),
                match (&c.references, c.is_foreign_key) {
                    (Some(r), _) => format!("{}.{}", r.table, r.column),
                    (None, true) => "FK".to_string(),
                    (None, false) => String::new(),
                },
                c.default_value.clone().unwrap_or_default(),
                c.comment.clone().unwrap_or_default(),
            ]);
            cells
        })
        .collect();
    PropertyTable { headers, rows }
}

fn indexes_table(indexes: &[engine::driver::traits::IndexDetail]) -> PropertyTable {
    let headers = ["名称", "类型", "唯一", "主键", "包含列", "注释"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rows = indexes
        .iter()
        .map(|idx| {
            vec![
                idx.name.clone(),
                idx.index_type.clone().unwrap_or_default(),
                if idx.is_unique { "是" } else { "否" }.to_string(),
                if idx.is_primary { "是" } else { "否" }.to_string(),
                idx.column_names.join(", "),
                idx.comment.clone().unwrap_or_default(),
            ]
        })
        .collect();
    PropertyTable { headers, rows }
}

fn constraints_table(constraints: &[engine::driver::traits::ConstraintDetail]) -> PropertyTable {
    let headers = ["名称", "类型", "列", "引用表", "引用列", "更新", "删除"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rows = constraints
        .iter()
        .map(|c| {
            vec![
                c.name.clone(),
                c.constraint_type.clone(),
                c.column_names.join(", "),
                c.referenced_table.clone().unwrap_or_default(),
                c.referenced_columns.join(", "),
                c.update_rule.clone().unwrap_or_default(),
                c.delete_rule.clone().unwrap_or_default(),
            ]
        })
        .collect();
    PropertyTable { headers, rows }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 转换：PropertyRef → PropertyTargetRef（kind / 父对象 / 来源文案）。
    #[test]
    fn to_target_maps_ref_fields() {
        let r = PropertyRef {
            conn_id: "c1".to_string(),
            kind: PropertyKind::Column,
            name: "id".to_string(),
            catalog: Some("db".to_string()),
            schema: Some("public".to_string()),
            parent: Some("t".to_string()),
            source: NavSource::Project,
        };
        let t = super::to_target(&r);
        assert_eq!(t.kind, PropertyKind::Column);
        assert_eq!(t.parent.as_deref(), Some("t"));
        assert_eq!(t.source, "项目 (P)");
    }

    /// fallback 连接行（provider 未注册的驱动保底展示）。
    #[test]
    fn fallback_connection_rows_show_base_info() {
        let r = PropertyRef {
            conn_id: "c1".to_string(),
            kind: PropertyKind::Connection,
            name: String::new(),
            catalog: None,
            schema: None,
            parent: None,
            source: NavSource::Global,
        };
        let rows = super::fallback_properties(
            &r, "C1", "oracle", Some("Oracle"), "全局 (G)",
            &[], &[], &[], &None, &None,
        );
        let labels: Vec<&str> = rows.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"名称"));
        assert!(labels.contains(&"驱动"));
    }

    /// DDL 合成验证：PG 表无源版时由骨架按目录信息合成（源版优先）。
    #[test]
    fn sections_of_ddl_composes_when_no_source() {
        let s = super::sections_of(
            PropertyKind::Table,
            PageId::Ddl,
            "public.t",
            &[], &[], &[], &None, &None,
            &crate::property_providers::DriverCapabilities::default(),
        );
        assert!(s.is_some(), "PG 表无源版时合成 DDL");
        assert_eq!(s.unwrap().label, "DDL（合成）");
    }

    /// 列字段增强（2026-10-24）：列表按能力位裁剪——pg / mysql 追加「标识 / 排序规则」列，
    /// sqlite / duckdb 不显示；标识值取 `identity_generation`（BY DEFAULT / AUTO_INCREMENT 等）。
    #[test]
    fn columns_table_clips_identity_and_collation_by_caps() {
        use engine::driver::traits::ColumnDetail;
        let col = ColumnDetail {
            name: "id".to_string(),
            data_type: "bigint".to_string(),
            nullable: false,
            is_primary_key: true,
            is_foreign_key: false,
            references: None,
            ordinal: 1,
            default_value: None,
            comment: None,
            is_identity: true,
            identity_generation: Some("BY DEFAULT".to_string()),
            collation: Some("default".to_string()),
            extra: Default::default(),
        };
        let pg = super::columns_table(&[col.clone()], &crate::property_providers::DriverCapabilities::pg());
        assert_eq!(pg.headers.len(), 10, "pg 10 列（含 标识/排序规则）: {:#?}", pg.headers);
        assert!(pg.headers.contains(&"标识".to_string()));
        assert!(pg.headers.contains(&"排序规则".to_string()));
        let pg_row = &pg.rows[0];
        assert_eq!(pg_row[3], "BY DEFAULT", "标识列展示生成方式");
        assert_eq!(pg_row[4], "default", "排序规则列展示 collation");

        let sq = super::columns_table(&[col], &crate::property_providers::DriverCapabilities::sqlite());
        assert_eq!(sq.headers.len(), 8, "sqlite 8 列（无 标识/排序规则）: {:#?}", sq.headers);
        assert!(!sq.headers.contains(&"标识".to_string()));
        assert_eq!(sq.rows[0].len(), 8);
    }
}
