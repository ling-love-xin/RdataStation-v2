//! `MockGenerationStore` 的真库往返（迁移 009 的四张表）。
//!
//! `src/persistence.rs` 的单测只验序列化形状（结构体 ↔ JSON），读写的另一半——SQL 列名、
//! 可空列、布尔位、JSON 参数、排序键、级联删除——没有任何东西在验。store 目前全项目零调用
//! （M7 的「生成历史 / 模板落库」待接线），所以这里用**真 SQLite**（走 `ProjectDatabaseManager`
//! 的迁移链，顺带验证 009 确实挂上了）把它钉住：接线时才发现读写对不上就太晚了。
//!
//! 每个用例一个临时项目目录，互相不共享状态。

use std::path::PathBuf;

use engine::persistence::project_db::ProjectDatabaseManager;
use rds_mock::persistence::{
    MockGenerationColumn, MockGenerationStore, MockGenerationTask, MockTemplateColumn,
    MockUserTemplate,
};

/// 建临时项目目录并打开真项目库（`.RSmeta/project.db`，迁移 009 已应用）。
async fn new_store(tag: &str) -> (MockGenerationStore, PathBuf) {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "rds_mock_store_{tag}_{}_{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("临时项目目录");
    let manager = ProjectDatabaseManager::open(&root, 2)
        .await
        .expect("打开项目库（含迁移 009）");
    (MockGenerationStore::new(manager.sqlite_pool()), root)
}

fn task(id: &str, created_at: &str) -> MockGenerationTask {
    MockGenerationTask {
        id: id.to_string(),
        table_name: "orders".to_string(),
        table_alias: Some("订单".to_string()),
        row_count: 500,
        seed: Some(42),
        locale: "ZH_CN".to_string(),
        scene_id: Some("ecommerce".to_string()),
        save_format: Some("csv".to_string()),
        status: "success".to_string(),
        error_message: None,
        generated_rows: Some(500),
        generation_time_ms: Some(1234),
        created_at: Some(created_at.to_string()),
        updated_at: Some(created_at.to_string()),
    }
}

fn column(id: &str, task_id: &str, name: &str, sort_order: i32) -> MockGenerationColumn {
    MockGenerationColumn {
        id: id.to_string(),
        task_id: task_id.to_string(),
        column_name: name.to_string(),
        column_type: "DECIMAL(12,2)".to_string(),
        generator: "decimal".to_string(),
        generator_params: Some(r#"{"min":1,"max":9}"#.to_string()),
        null_ratio: 0.25,
        is_unique: false,
        is_primary_key: sort_order == 0,
        is_foreign_key: false,
        ref_table: None,
        ref_column: None,
        comment: None,
        confidence: Some("high".to_string()),
        sort_order,
    }
}

fn template_column(id: &str, template_id: &str, name: &str, sort_order: i32) -> MockTemplateColumn {
    MockTemplateColumn {
        id: id.to_string(),
        template_id: template_id.to_string(),
        column_name: name.to_string(),
        column_type: "INTEGER".to_string(),
        generator: "auto_increment".to_string(),
        generator_params: None,
        null_ratio: 0.0,
        is_unique: true,
        is_primary_key: true,
        is_foreign_key: false,
        ref_table: None,
        ref_column: None,
        comment: Some("主键".to_string()),
        confidence: None,
        sort_order,
    }
}

fn template(id: &str, name: &str) -> MockUserTemplate {
    MockUserTemplate {
        id: id.to_string(),
        name: name.to_string(),
        description: Some("电商主数据".to_string()),
        row_count: 1000,
        seed: None,
        locale: "ZH_CN".to_string(),
        created_at: Some("2026-09-16T10:00:00Z".to_string()),
        updated_at: Some("2026-09-16T10:00:00Z".to_string()),
    }
}

fn names_of(columns: &[MockGenerationColumn]) -> Vec<&str> {
    columns.iter().map(|c| c.column_name.as_str()).collect()
}

fn ids_of(tasks: &[MockGenerationTask]) -> Vec<&str> {
    tasks.iter().map(|t| t.id.as_str()).collect()
}

#[tokio::test]
async fn task_round_trips_with_columns_in_sort_order() {
    let (store, root) = new_store("task").await;

    // 故意乱序插入：读回来必须按 `sort_order`（面板按列序显示，不按插入顺序）。
    let mut id_column = column("c0", "t1", "id", 0);
    // 自动编号不需要参数：这条同时验「没传的列回来是 None 而不是空串」。
    id_column.generator_params = None;
    store
        .save_task(
            &task("t1", "2026-09-16T10:00:00Z"),
            &[
                column("c2", "t1", "amount", 2),
                id_column,
                column("c1", "t1", "name", 1),
            ],
        )
        .await
        .expect("保存任务");

    let detail = store.get_detail("t1").await.expect("读回任务");
    assert_eq!(detail.task.table_name, "orders");
    assert_eq!(detail.task.table_alias.as_deref(), Some("订单"));
    assert_eq!(detail.task.row_count, 500);
    assert_eq!(detail.task.seed, Some(42));
    assert_eq!(detail.task.locale, "ZH_CN");
    assert_eq!(detail.task.scene_id.as_deref(), Some("ecommerce"));
    assert_eq!(detail.task.save_format.as_deref(), Some("csv"));
    assert_eq!(detail.task.status, "success");
    assert_eq!(detail.task.error_message, None);
    assert_eq!(detail.task.generated_rows, Some(500));
    assert_eq!(detail.task.generation_time_ms, Some(1234));
    assert_eq!(
        detail.task.created_at.as_deref(),
        Some("2026-09-16T10:00:00Z")
    );

    assert_eq!(names_of(&detail.columns), vec!["id", "name", "amount"]);
    let id_col = &detail.columns[0];
    assert!(id_col.is_primary_key, "布尔位按整数位存，读回仍为真");
    assert!(!id_col.is_unique);
    assert_eq!(
        id_col.generator_params, None,
        "没传的参数回来是 None 而不是空串"
    );
    assert_eq!(id_col.comment, None);
    let amount_col = &detail.columns[2];
    assert_eq!(amount_col.generator, "decimal");
    assert_eq!(
        amount_col.generator_params.as_deref(),
        Some(r#"{"min":1,"max":9}"#),
        "生成器参数是整串 JSON，store 不解析它"
    );
    assert_eq!(amount_col.null_ratio, 0.25);
    assert_eq!(amount_col.confidence.as_deref(), Some("high"));

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn missing_timestamps_stay_null() {
    let (store, root) = new_store("nulls").await;

    // 全部可空列都不传：读回来必须还是 None——若写成空串，就与「确实记了一个空值」
    // 在读取侧分不开（历史列表要按时间排，空串只是恰好排在最后）。
    let bare = MockGenerationTask {
        id: "t1".to_string(),
        table_name: "logs".to_string(),
        table_alias: None,
        row_count: 10,
        seed: None,
        locale: "ZH_CN".to_string(),
        scene_id: None,
        save_format: None,
        status: "failed".to_string(),
        error_message: Some("列类型无法映射".to_string()),
        generated_rows: None,
        generation_time_ms: None,
        created_at: None,
        updated_at: None,
    };
    store.save_task(&bare, &[]).await.expect("保存任务");

    let detail = store.get_detail("t1").await.expect("读回任务");
    assert_eq!(detail.task.table_alias, None);
    assert_eq!(detail.task.seed, None);
    assert_eq!(detail.task.scene_id, None);
    assert_eq!(detail.task.save_format, None);
    assert_eq!(detail.task.generated_rows, None);
    assert_eq!(detail.task.generation_time_ms, None);
    assert_eq!(detail.task.created_at, None);
    assert_eq!(detail.task.updated_at, None);
    assert_eq!(
        detail.task.error_message.as_deref(),
        Some("列类型无法映射"),
        "失败态的原因要能原样读回（历史列表显示它）"
    );
    assert!(detail.columns.is_empty(), "没有列的任务也能单独存");

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn history_is_newest_first_and_limited() {
    let (store, root) = new_store("history").await;

    for (index, id) in ["t_a", "t_b", "t_c"].iter().enumerate() {
        let at = format!("2026-09-16T1{index}:00:00Z");
        store
            .save_task(&task(id, &at), &[column(&format!("{id}_c0"), id, "id", 0)])
            .await
            .expect("保存任务");
    }

    let all = store.get_history(10).await.expect("读历史");
    assert_eq!(ids_of(&all), vec!["t_c", "t_b", "t_a"], "最近的在最前");

    let two = store.get_history(2).await.expect("读历史");
    assert_eq!(ids_of(&two), vec!["t_c", "t_b"], "limit 截的是尾部");

    // 列按 task_id 归属：别家的列不能串进来。
    let detail = store.get_detail("t_a").await.expect("读回任务");
    assert_eq!(names_of(&detail.columns), vec!["id"]);

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn deleting_a_task_takes_its_columns_with_it() {
    let (store, root) = new_store("delete_task").await;

    store
        .save_task(
            &task("t1", "2026-09-16T10:00:00Z"),
            &[column("c0", "t1", "id", 0), column("c1", "t1", "name", 1)],
        )
        .await
        .expect("保存任务");

    store.delete_task("t1").await.expect("删除任务");
    assert!(
        store.get_detail("t1").await.is_err(),
        "删掉的任务不该还能读出来"
    );
    assert!(store.get_history(10).await.expect("读历史").is_empty());

    // 子行必须随任务一起没了（建表语句写的是 ON DELETE CASCADE，池开了 foreign_keys）：
    // 复用同样的列 id 再存一次，若还留着就会撞主键。
    store
        .save_task(
            &task("t2", "2026-09-16T11:00:00Z"),
            &[column("c0", "t2", "id", 0), column("c1", "t2", "name", 1)],
        )
        .await
        .expect("同样的列 id 应可复用（说明旧子行已被级联删除）");

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn templates_round_trip_and_delete_their_columns() {
    let (store, root) = new_store("template").await;

    store
        .save_template(
            &template("tpl1", "电商主数据"),
            &[
                template_column("tc1", "tpl1", "price", 1),
                template_column("tc0", "tpl1", "id", 0),
            ],
        )
        .await
        .expect("保存模板");

    let list = store.get_templates().await.expect("读模板列表");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "电商主数据");
    assert_eq!(list[0].description.as_deref(), Some("电商主数据"));
    assert_eq!(list[0].seed, None);

    let (read_back, columns) = store.get_template_detail("tpl1").await.expect("读模板详情");
    assert_eq!(read_back.row_count, 1000);
    assert_eq!(
        columns
            .iter()
            .map(|c| c.column_name.as_str())
            .collect::<Vec<_>>(),
        vec!["id", "price"],
        "模板列同样按 sort_order 读回"
    );
    assert_eq!(columns[0].comment.as_deref(), Some("主键"));

    store.delete_template("tpl1").await.expect("删除模板");
    assert!(store.get_templates().await.expect("读模板列表").is_empty());
    assert!(
        store.get_template_detail("tpl1").await.is_err(),
        "删掉的模板不该还能读出来"
    );

    // 子行也要跟着走：同样的列 id 挂到新模板上不该撞主键。
    store
        .save_template(
            &template("tpl2", "另一套"),
            &[template_column("tc0", "tpl2", "id", 0)],
        )
        .await
        .expect("同样的列 id 应可复用");

    std::fs::remove_dir_all(&root).ok();
}
