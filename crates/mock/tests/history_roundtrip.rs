//! 生成历史的真库端到端（M7 · D4/D5）。
//!
//! `persistence_roundtrip.rs` 验的是 store 的读写守恒；这里验的是**它的上一层**：
//! `history::run` / `list` / `detail` 把「一次生成运行」变成历史行、再从历史行还原成草稿
//! 的整条链路——面板侧只提交一件 `HistoryAction`，其余全在 mock crate 内完成。
//!
//! 每个用例一个临时项目目录（`ProjectDatabaseManager::open` 会跑迁移，互不共享）。

use std::path::PathBuf;

use rds_mock::history::{
    self, HistoryAction, RunOutcome, RunRecord, draft_of_detail, draft_of_template,
};
use rds_mock::mock_view::{MockColumnSpec, MockDraft, MockRunOptions};
use rds_mock::models::{ColumnDataType, ColumnDef, GeneratorConfig, Locale};
use rds_mock::persistence::MockGenerationTask;

/// 临时项目根（真项目库在 `{根}/.RSmeta/project.db`）。
fn temp_project(tag: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "rds_mock_history_{tag}_{}_{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("临时项目目录");
    root
}

fn draft(table: &str, seed: Option<u32>) -> MockDraft {
    MockDraft {
        table_name: table.to_string(),
        columns: vec![
            MockColumnSpec {
                id: 1,
                def: ColumnDef {
                    name: "id".to_string(),
                    data_type: ColumnDataType::Integer,
                    generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                    nullable_ratio: 0.0,
                    unique: false,
                    dependency: None,
                },
                confidence: "high".to_string(),
                sample_value: String::new(),
            },
            MockColumnSpec {
                id: 2,
                def: ColumnDef {
                    name: "amount".to_string(),
                    data_type: ColumnDataType::Decimal {
                        precision: 12,
                        scale: 2,
                    },
                    generator: GeneratorConfig::RandomDecimal {
                        min: 1.0,
                        max: 9.0,
                        scale: 2,
                    },
                    nullable_ratio: 0.1,
                    unique: false,
                    dependency: None,
                },
                confidence: "low".to_string(),
                sample_value: String::new(),
            },
        ],
        // `MockRunOptions` 是 `#[non_exhaustive]`：只有 `new` 一个构造入口（跨 crate）
        options: MockRunOptions::new(250, seed, Locale::En),
    }
}

fn succeeded() -> RunRecord {
    RunRecord {
        save_format: None,
        outcome: RunOutcome::Succeeded {
            rows: 250,
            elapsed_ms: Some(31),
        },
    }
}

fn ids(tasks: &[MockGenerationTask]) -> Vec<&str> {
    tasks.iter().map(|task| task.id.as_str()).collect()
}

#[tokio::test]
async fn a_run_is_recorded_listed_and_replayed() {
    let root = temp_project("replay");
    let recorded = draft("orders", Some(11));

    let tasks = history::run(
        &root,
        HistoryAction::Record {
            draft: recorded,
            run: succeeded(),
        },
        history::HISTORY_LIMIT,
    )
    .await
    .expect("记录一次运行")
    .tasks;

    assert_eq!(tasks.len(), 1, "记录后列表应包含这一条");
    let task = &tasks[0];
    assert_eq!(task.table_name, "orders");
    assert_eq!(task.row_count, 250);
    assert_eq!(task.seed, Some(11));
    assert_eq!(task.locale, "EN", "语言按 serde 名入库");
    assert_eq!(task.status, "success");
    assert_eq!(task.generated_rows, Some(250));
    assert_eq!(task.generation_time_ms, Some(31));

    // 列表读回同一份（面板刷新走这条）
    let listed = history::list(&root, history::HISTORY_LIMIT)
        .await
        .expect("读列表")
        .tasks;
    assert_eq!(ids(&listed), vec![task.id.as_str()]);

    // 重放：草稿从历史还原（生成器与参数经目录名 + 参数 JSON 一条路往返）
    let detail = history::detail(&root, &task.id).await.expect("读详情");
    let replayed = draft_of_detail(&detail);
    assert_eq!(replayed.table_name, "orders");
    assert_eq!(replayed.options.rows, 250);
    assert_eq!(replayed.options.seed, Some(11));
    assert_eq!(replayed.options.locale, Locale::En);
    assert_eq!(
        replayed
            .columns
            .iter()
            .map(|column| column.def.name.as_str())
            .collect::<Vec<_>>(),
        vec!["id", "amount"]
    );
    assert_eq!(replayed.columns[1].def.nullable_ratio, 0.1);
    assert_eq!(replayed.columns[1].confidence, "low");

    // 删除：列表与详情同时消失
    let after_delete = history::run(
        &root,
        HistoryAction::DeleteTask(task.id.clone()),
        history::HISTORY_LIMIT,
    )
    .await
    .expect("删除")
    .tasks;
    assert!(after_delete.is_empty());
    assert!(
        history::detail(&root, &task.id).await.is_err(),
        "删掉的任务不该还能读出详情"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn history_is_newest_first_and_failures_keep_their_reason() {
    let root = temp_project("order");

    for (table, run) in [
        ("first", succeeded()),
        (
            "second",
            RunRecord {
                save_format: Some(history::SAVE_FORMAT_TABLE.to_string()),
                outcome: RunOutcome::Failed {
                    reason: "分析库已存在表 second".to_string(),
                },
            },
        ),
    ] {
        history::run(
            &root,
            HistoryAction::Record {
                draft: draft(table, None),
                run,
            },
            history::HISTORY_LIMIT,
        )
        .await
        .expect("记录");
    }

    let tasks = history::list(&root, history::HISTORY_LIMIT)
        .await
        .expect("读列表")
        .tasks;
    assert_eq!(tasks.len(), 2);
    assert_eq!(
        tasks[0].table_name, "second",
        "最近的在最前（列表按 created_at 倒序）"
    );
    assert_eq!(tasks[0].status, "failed");
    assert_eq!(
        tasks[0].error_message.as_deref(),
        Some("分析库已存在表 second"),
        "失败原因随记录保存"
    );
    assert_eq!(tasks[0].save_format.as_deref(), Some("table"));
    assert_eq!(tasks[1].status, "success");

    // limit 截尾（面板一次只取最近若干条）
    let only_one = history::list(&root, 1).await.expect("读列表").tasks;
    assert_eq!(only_one.len(), 1);
    assert_eq!(only_one[0].table_name, "second");

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn a_template_is_saved_listed_replayed_and_deleted() {
    let root = temp_project("template");
    let saved = draft("orders", Some(11));

    let templates = history::run(
        &root,
        HistoryAction::SaveTemplate {
            name: "  电商主数据  ".to_string(),
            draft: saved,
        },
        history::HISTORY_LIMIT,
    )
    .await
    .expect("保存模板")
    .templates;

    assert_eq!(templates.len(), 1, "保存后列表里应有一条");
    let template = &templates[0];
    assert_eq!(template.name, "电商主数据", "名字取 trim 后的值");
    assert_eq!(template.row_count, 250);
    assert_eq!(template.seed, Some(11));
    assert_eq!(template.locale, "EN");
    assert_eq!(template.description.as_deref(), Some("2 列"));

    // 应用：拿详细配置还原出一份草稿（表名由调用方决定，模板里不存）
    let (read_back, columns) = history::template_detail(&root, &template.id)
        .await
        .expect("读模板详情");
    let replayed = draft_of_template(&read_back, &columns);
    assert_eq!(replayed.options.rows, 250);
    assert_eq!(replayed.options.seed, Some(11));
    assert_eq!(replayed.options.locale, Locale::En);
    assert_eq!(
        replayed
            .columns
            .iter()
            .map(|column| column.def.name.as_str())
            .collect::<Vec<_>>(),
        vec!["id", "amount"]
    );
    assert_eq!(replayed.columns[1].def.nullable_ratio, 0.1);

    // 删：列表与详情同时消失
    let left = history::run(
        &root,
        HistoryAction::DeleteTemplate(template.id.clone()),
        history::HISTORY_LIMIT,
    )
    .await
    .expect("删除模板")
    .templates;
    assert!(left.is_empty());
    assert!(
        history::template_detail(&root, &template.id).await.is_err(),
        "删掉的模板不该还能读出详情"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn a_project_root_that_is_a_file_reports_a_readable_error() {
    // 注意：**不存在的目录不算错**——项目库的打开语义是「没有就建」（新项目落地就这么走），
    // 真正的错误是路径不可能是目录（指向一个文件）。
    let parent = temp_project("broken");
    let file = parent.join("not_a_project");
    std::fs::write(&file, "x").expect("写一个文件占位");

    let error = history::list(&file, history::HISTORY_LIMIT)
        .await
        .expect_err("项目根是文件时应报错");
    assert!(error.contains("打开项目库失败"), "{error}");

    std::fs::remove_dir_all(&parent).ok();
}
