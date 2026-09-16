//! 生成历史与用户模板（M7 · D4/D5/C4）：领域动作 + 后台执行入口。
//!
//! # 为什么要一个快照
//!
//! 面板同时要「最近几次运行」与「保存过的模板」——两者在同一个库里、同一次打开连接就能读完，
//! 所以一次后台读返回 [`HistorySnapshot`]，不做两次往返（也就不会出现「一半新一半旧」）。
//!
//! # 为什么在 mock crate 内
//!
//! 历史落在 `{项目}/.RSmeta/project.db`（迁移 009，读写见 [`crate::persistence`]），而
//! 「项目根在哪」只有宿主知道。宿主因此只回答这一个问题（`MockHost::project_root`），
//! **取数、组装、写入全部在本 crate 内完成**——与 `insight::jobs` 同一形态，不把存储细节
//! 摊到工作台里（耦合治理计划 §5 的目标形态：特性 crate 内的 jobs + 面板 drain）。
//!
//! # 线程边界
//!
//! 打开项目库（文件 I/O）与 SQLite 读写都在**后台线程**上跑：调用方只提交一件
//! [`HistoryAction`]，拿回的是所有权数据（`Vec<MockGenerationTask>` / `MockGenerationDetail`）。
//! 面板没有同步入口，渲染期也不碰这里。
//!
//! 为什么要自己进一次 tokio 运行时（[`drive`]）：项目库的打开与读写走 `tokio::fs` /
//! `tokio::sync`（`ProjectDatabaseManager`），而调用我们的线程是 **GPUI 后台执行器**的
//! 线程池——它没有 tokio reactor，直接 `.await` 会在“没有 reactor”处 panic。与工作台
//! 后台任务（`services::{resource_jobs,scratchpad_jobs}`）在工作线程里进运行时的口径一致；
//! 差别只是那边线程是自己起的，这里借的是 GPUI 的后台线程（不阻塞 UI）。
//!
//! # 一次动作 = 一次读
//!
//! [`run`] 在动作之后**顺带重读列表**并返回：面板不必「删除成功后再补一次读」，
//! 也就不会出现「删掉了但列表还是旧的」这类中间态。

use std::path::Path;

use chrono::{DateTime, Utc};
use engine::persistence::project_db::ProjectDatabaseManager;
use serde_json::Value;
use uuid::Uuid;

use crate::generator_catalog;
use crate::mock_view::{
    MockColumnSpec, MockDraft, MockJobDone, MockJobKind, default_generator_for,
};
use crate::models::{ColumnDef, DependencyType, GeneratorConfig, Locale};
use crate::persistence::{
    MockGenerationColumn, MockGenerationDetail, MockGenerationStore, MockGenerationTask,
    MockTemplateColumn, MockUserTemplate,
};
use crate::schema_map::parse_data_type;

/// 打开项目库时的连接池大小（与 `data_source_service` / `workspace_loader` 同口径）。
const SQLITE_POOL_SIZE: usize = 4;

/// 面板一次读多少条（历史按时间倒序，够回看最近几次）。
pub const HISTORY_LIMIT: u32 = 20;

/// 追加类运行的落点标记（写进 `save_format`，历史行据此显示「→ 表」）。
pub const SAVE_FORMAT_TABLE: &str = "table";

/// 面板要后台做的一件事（读由 [`list`] / [`run`] 的返回值承担，不单列一条动作）。
#[derive(Debug, Clone)]
pub enum HistoryAction {
    /// 记一次生成运行（成功与失败都记；取消与出口类不记，见 [`RunRecord::of`]）。
    Record { draft: MockDraft, run: RunRecord },
    /// 删一条历史（连带它的列行）。
    DeleteTask(String),
    /// 把当前草稿存成用户模板。
    SaveTemplate { name: String, draft: MockDraft },
    /// 删一个用户模板（连带它的列行）。
    DeleteTemplate(String),
}

/// 一次读到的历史与模板。
///
/// 两者在同一个库里、同一次打开连接就能读完，所以合成一次读——面板不会出现
/// 「历史是新的、模板还是旧的」这种一半新一半旧的中间态。
#[derive(Debug, Clone, Default)]
pub struct HistorySnapshot {
    /// 最近若干次运行（时间倒序）
    pub tasks: Vec<MockGenerationTask>,
    /// 保存过的用户模板（时间倒序）
    pub templates: Vec<MockUserTemplate>,
}

/// 一次生成运行的结局。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// 成功：本次生成行数；追加类的写入耗时在任务里拿不到，故为 `None`。
    Succeeded { rows: i32, elapsed_ms: Option<i32> },
    /// 失败：给用户看的原因（原样入库，历史行会显示它）。
    Failed { reason: String },
}

/// 一次生成运行的记录请求（面板在任务收尾时组装）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    /// 落点：`Some("table")` = 追加进分析库既有表；`None` = 只产内存临时表。
    pub save_format: Option<String>,
    pub outcome: RunOutcome,
}

impl RunRecord {
    /// 从任务种类与结局组装；**没有产出的运行返回 `None`**：
    ///
    /// - 出口类（落库 / 导出 / 草稿箱）不产新配置，记了只是重复上一条；
    /// - 用户主动取消的运行没有数据，记下来只会把真记录挤掉。
    pub fn of(
        kind: &MockJobKind,
        result: &Result<MockJobDone, String>,
        draft: &MockDraft,
    ) -> Option<Self> {
        let save_format = match kind {
            MockJobKind::Generate => None,
            MockJobKind::AppendTo(_) => Some(SAVE_FORMAT_TABLE.to_string()),
            _ => return None,
        };
        let outcome = match result {
            Ok(MockJobDone::Generated(info)) => RunOutcome::Succeeded {
                rows: info.row_count as i32,
                elapsed_ms: Some(info.elapsed_ms as i32),
            },
            // 追加：行数就是本次生成量；耗时落在「写入」阶段，任务不回传
            Ok(MockJobDone::Appended { .. }) => RunOutcome::Succeeded {
                rows: kind.rows_total(draft) as i32,
                elapsed_ms: None,
            },
            Err(reason) if reason.contains("取消") => return None,
            Err(reason) => RunOutcome::Failed {
                reason: reason.clone(),
            },
            // 出口类完成：上面已在种类上排除，这里只是把「不记录」写实
            Ok(_) => return None,
        };
        Some(Self {
            save_format,
            outcome,
        })
    }
}

/// 执行一次动作，并返回**动作之后**的历史与模板。
pub async fn run(
    project_root: &Path,
    action: HistoryAction,
    limit: u32,
) -> Result<HistorySnapshot, String> {
    let store = open_store(project_root).await?;
    match action {
        HistoryAction::Record { draft, run } => {
            let (task, columns) = task_of_run(&draft, &run, Utc::now());
            store
                .save_task(&task, &columns)
                .await
                .map_err(|e| format!("写入生成历史失败：{e}"))?;
        }
        HistoryAction::DeleteTask(id) => {
            store
                .delete_task(&id)
                .await
                .map_err(|e| format!("删除生成历史失败：{e}"))?;
        }
        HistoryAction::SaveTemplate { name, draft } => {
            let (template, columns) = template_of_draft(&name, &draft, Utc::now());
            store
                .save_template(&template, &columns)
                .await
                .map_err(|e| format!("保存模板失败：{e}"))?;
        }
        HistoryAction::DeleteTemplate(id) => {
            store
                .delete_template(&id)
                .await
                .map_err(|e| format!("删除模板失败：{e}"))?;
        }
    }
    read_snapshot(&store, limit).await
}

/// 只读快照（打开面板 / 切换项目时用）。
pub async fn list(project_root: &Path, limit: u32) -> Result<HistorySnapshot, String> {
    let store = open_store(project_root).await?;
    read_snapshot(&store, limit).await
}

/// 取一条历史任务的完整配置（重放用）。
pub async fn detail(project_root: &Path, task_id: &str) -> Result<MockGenerationDetail, String> {
    let store = open_store(project_root).await?;
    store
        .get_detail(task_id)
        .await
        .map_err(|e| format!("读取生成历史详情失败：{e}"))
}

/// 取一个模板的完整配置（应用模板用）。
pub async fn template_detail(
    project_root: &Path,
    template_id: &str,
) -> Result<(MockUserTemplate, Vec<MockTemplateColumn>), String> {
    let store = open_store(project_root).await?;
    store
        .get_template_detail(template_id)
        .await
        .map_err(|e| format!("读取模板详情失败：{e}"))
}

async fn open_store(project_root: &Path) -> Result<MockGenerationStore, String> {
    let manager = ProjectDatabaseManager::open(project_root, SQLITE_POOL_SIZE)
        .await
        .map_err(|e| format!("打开项目库失败：{e}"))?;
    Ok(MockGenerationStore::new(manager.sqlite_pool()))
}

/// 在后台线程上把一件异步动作用跑完（自备 tokio 运行时，理由见模块文档）。
///
/// 调用方是**已落到后台的执行器**（面板用 `background_executor().spawn` 包住这里），
/// 因此 `block_on` 阻塞的是后台线程，不是 UI 线程。
pub fn drive<T>(future: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("创建后台运行时失败：{e}"))?;
    runtime.block_on(future)
}

async fn read_snapshot(store: &MockGenerationStore, limit: u32) -> Result<HistorySnapshot, String> {
    let tasks = store
        .get_history(limit)
        .await
        .map_err(|e| format!("读取生成历史失败：{e}"))?;
    let templates = store
        .get_templates()
        .await
        .map_err(|e| format!("读取模板列表失败：{e}"))?;
    Ok(HistorySnapshot { tasks, templates })
}

// ==================== 映射（纯函数，单测锁住） ====================

/// 草稿 + 运行结局 → 历史任务与列。
///
/// 列序即 `sort_order`（读回时按它排）；生成器存**目录名 + 参数 JSON**，与
/// [`config_from_parts`] 严格对称——重放因此不需要第二套映射表。
pub fn task_of_run(
    draft: &MockDraft,
    run: &RunRecord,
    now: DateTime<Utc>,
) -> (MockGenerationTask, Vec<MockGenerationColumn>) {
    let task_id = Uuid::new_v4().to_string();
    let stamp = now.to_rfc3339();
    let (status, error_message, generated_rows, elapsed_ms) = match &run.outcome {
        RunOutcome::Succeeded { rows, elapsed_ms } => ("success", None, Some(*rows), *elapsed_ms),
        RunOutcome::Failed { reason } => ("failed", Some(reason.clone()), None, None),
    };

    let task = MockGenerationTask {
        id: task_id.clone(),
        table_name: draft.table_name.trim().to_string(),
        table_alias: None,
        row_count: draft.options.rows as i32,
        // seed 是 u32：按位存进 i32 列，重放侧按位读回（`u32::MAX` 也不会丢）
        seed: draft.options.seed.map(|seed| seed as i32),
        locale: locale_token(&draft.options.locale),
        scene_id: None,
        save_format: run.save_format.clone(),
        status: status.to_string(),
        error_message,
        generated_rows,
        generation_time_ms: elapsed_ms,
        created_at: Some(stamp.clone()),
        updated_at: Some(stamp),
    };

    let columns = draft
        .columns
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            ColumnFields::of(spec).into_task(
                Uuid::new_v4().to_string(),
                task_id.clone(),
                index as i32,
            )
        })
        .collect();

    (task, columns)
}

/// 草稿 + 模板名 → 用户模板与模板列。
///
/// 模板存的是「怎么造数据」（行数 / 种子 / 语言 / 列），**不存目标表名**——表名是
/// 「造到哪张表」，属于一次运行的输入，不属于可复用的配置，见 [`draft_of_template`]。
pub fn template_of_draft(
    name: &str,
    draft: &MockDraft,
    now: DateTime<Utc>,
) -> (MockUserTemplate, Vec<MockTemplateColumn>) {
    let template_id = Uuid::new_v4().to_string();
    let stamp = now.to_rfc3339();
    let template = MockUserTemplate {
        id: template_id.clone(),
        name: name.trim().to_string(),
        // 描述当列表里的概览用（保存对话框只问名字，不额外要求用户写说明）
        description: Some(format!("{} 列", draft.columns.len())),
        row_count: draft.options.rows as i32,
        // seed 是 u32：按位存进 i32 列，应用模板时按位读回
        seed: draft.options.seed.map(|seed| seed as i32),
        locale: locale_token(&draft.options.locale),
        created_at: Some(stamp.clone()),
        updated_at: Some(stamp),
    };

    let columns = draft
        .columns
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            ColumnFields::of(spec).into_template(
                Uuid::new_v4().to_string(),
                template_id.clone(),
                index as i32,
            )
        })
        .collect();

    (template, columns)
}

/// 历史条目 + 列 → 草稿（重放）。
///
/// 列 id 从 1 起编：面板拿到后按自己的 `next_id` 重新编号（id 只用于面板内部定位）。
/// **列依赖不随重放恢复**：依赖是结构导入的产物（`import_columns` 目前恒为 `None`），
/// 要它的人重新导入一次即可；硬塞一个空 `source_columns` 的依赖反而会在生成期出事。
pub fn draft_of_detail(detail: &MockGenerationDetail) -> MockDraft {
    let mut draft = draft_of_parts(
        detail.task.row_count,
        detail.task.seed,
        &detail.task.locale,
        detail.columns.iter().map(|column| spec_of_stored(column)),
    );
    draft.table_name = detail.task.table_name.clone();
    draft
}

/// 模板 + 列 → 草稿（应用模板）。
///
/// **不改目标表名**：模板不存表名，这里留 `MockDraft::default()` 的名字，由调用方按当前
/// 输入覆盖（「把这份配置摆回来」不该顺手换掉用户正在写的表名）。
pub fn draft_of_template(template: &MockUserTemplate, columns: &[MockTemplateColumn]) -> MockDraft {
    draft_of_parts(
        template.row_count,
        template.seed,
        &template.locale,
        columns.iter().map(|column| spec_of_stored(column)),
    )
}

/// 运行参数 + 列 → 草稿（重放与应用模板共用的骨架）。
fn draft_of_parts(
    rows: i32,
    seed: Option<i32>,
    locale: &str,
    columns: impl Iterator<Item = MockColumnSpec>,
) -> MockDraft {
    let mut draft = MockDraft::default();
    // 行数下限 1：历史行里可能出现 0（旧记录 / 手工改库），引擎对 0 行会直接拒绝
    draft.options.rows = rows.max(1) as u32;
    draft.options.seed = seed.map(|seed| seed as u32);
    draft.options.locale = locale_of(locale).unwrap_or(Locale::ZhCn);
    draft.columns = columns
        .enumerate()
        .map(|(index, mut spec)| {
            spec.id = index as u64 + 1;
            spec
        })
        .collect();
    draft
}

/// 存下来的列 → 草稿列（两张「列」表共用的读取面）。
fn spec_of_stored(column: &impl StoredColumn) -> MockColumnSpec {
    let data_type = parse_data_type(column.column_type());
    let generator = config_from_parts(column.generator(), column.generator_params())
        .unwrap_or_else(|| default_generator_for(&data_type));
    MockColumnSpec {
        // 由 `draft_of_parts` 统一编号
        id: 0,
        def: ColumnDef {
            name: column.column_name().to_string(),
            data_type,
            generator,
            nullable_ratio: column.null_ratio(),
            unique: column.is_unique(),
            // 依赖不随重放 / 应用模板恢复（见 `draft_of_detail`）
            dependency: None,
        },
        // 置信度是「这一列是怎么来的」的标注：记录里有就用它，没有就当手工列
        confidence: column
            .confidence()
            .map(str::to_string)
            .unwrap_or_else(|| "manual".to_string()),
        sample_value: String::new(),
    }
}

// ==================== 两张「列」表的公共面向 ====================

/// 存下来的列（`mock_generation_columns` / `mock_template_columns` 字段完全一致，
/// 只有父 id 的列名不同）。
///
/// 抽出来的理由：读两张表、写两张表要四份 11 字段的映射，任何一处写错都不会有人发现
/// ——公共部分收成一份，差异只剩「父 id 叫什么」。
trait StoredColumn {
    fn column_name(&self) -> &str;
    fn column_type(&self) -> &str;
    fn generator(&self) -> &str;
    fn generator_params(&self) -> Option<&str>;
    fn null_ratio(&self) -> f64;
    fn is_unique(&self) -> bool;
    fn confidence(&self) -> Option<&str>;
}

impl StoredColumn for MockGenerationColumn {
    fn column_name(&self) -> &str {
        &self.column_name
    }
    fn column_type(&self) -> &str {
        &self.column_type
    }
    fn generator(&self) -> &str {
        &self.generator
    }
    fn generator_params(&self) -> Option<&str> {
        self.generator_params.as_deref()
    }
    fn null_ratio(&self) -> f64 {
        self.null_ratio
    }
    fn is_unique(&self) -> bool {
        self.is_unique
    }
    fn confidence(&self) -> Option<&str> {
        self.confidence.as_deref()
    }
}

impl StoredColumn for MockTemplateColumn {
    fn column_name(&self) -> &str {
        &self.column_name
    }
    fn column_type(&self) -> &str {
        &self.column_type
    }
    fn generator(&self) -> &str {
        &self.generator
    }
    fn generator_params(&self) -> Option<&str> {
        self.generator_params.as_deref()
    }
    fn null_ratio(&self) -> f64 {
        self.null_ratio
    }
    fn is_unique(&self) -> bool {
        self.is_unique
    }
    fn confidence(&self) -> Option<&str> {
        self.confidence.as_deref()
    }
}

/// 草稿列 → 两张「列」表的公共字段（写路径，与 [`StoredColumn`] 对偶）。
struct ColumnFields {
    column_name: String,
    column_type: String,
    generator: String,
    generator_params: Option<String>,
    null_ratio: f64,
    is_unique: bool,
    is_foreign_key: bool,
    ref_table: Option<String>,
    ref_column: Option<String>,
    confidence: Option<String>,
}

impl ColumnFields {
    fn of(spec: &MockColumnSpec) -> Self {
        let (generator, generator_params) =
            generator_parts(&spec.def.generator).unwrap_or_else(|| (String::from("unknown"), None));
        let dependency = spec.def.dependency.as_ref();
        Self {
            column_name: spec.def.name.clone(),
            // 存**建表类型**（面板类型列显示的就是它），读回靠 `parse_data_type`
            column_type: spec.def.data_type.to_duckdb_type(),
            generator,
            generator_params,
            null_ratio: spec.def.nullable_ratio,
            is_unique: spec.def.unique,
            is_foreign_key: dependency
                .is_some_and(|d| matches!(d.dep_type, DependencyType::ForeignKey)),
            ref_table: dependency.and_then(|d| d.ref_table.clone()),
            ref_column: dependency.and_then(|d| d.ref_column.clone()),
            confidence: Some(spec.confidence.clone()),
        }
    }

    fn into_task(self, id: String, task_id: String, sort_order: i32) -> MockGenerationColumn {
        MockGenerationColumn {
            id,
            task_id,
            column_name: self.column_name,
            column_type: self.column_type,
            generator: self.generator,
            generator_params: self.generator_params,
            null_ratio: self.null_ratio,
            is_unique: self.is_unique,
            // 主键不是草稿里的概念（v2 没有主键配置），一律不标
            is_primary_key: false,
            is_foreign_key: self.is_foreign_key,
            ref_table: self.ref_table,
            ref_column: self.ref_column,
            comment: None,
            confidence: self.confidence,
            sort_order,
        }
    }

    fn into_template(self, id: String, template_id: String, sort_order: i32) -> MockTemplateColumn {
        MockTemplateColumn {
            id,
            template_id,
            column_name: self.column_name,
            column_type: self.column_type,
            generator: self.generator,
            generator_params: self.generator_params,
            null_ratio: self.null_ratio,
            is_unique: self.is_unique,
            is_primary_key: false,
            is_foreign_key: self.is_foreign_key,
            ref_table: self.ref_table,
            ref_column: self.ref_column,
            comment: None,
            confidence: self.confidence,
            sort_order,
        }
    }
}

/// 生成器配置 →（目录名，参数 JSON）。
///
/// 参数取自变体载荷：**没有参数的变体（`SafeEmail` 这类单元变体）返回 `None`**，
/// 空对象的载荷也不落成 `{}`。
pub fn generator_parts(config: &GeneratorConfig) -> Option<(String, Option<String>)> {
    let spec = generator_catalog::spec_of(config);
    let value = serde_json::to_value(config).ok()?;
    let params = match value {
        // 单元变体：载荷是变体名本身
        Value::String(_) => None,
        Value::Object(outer) => match outer.values().next() {
            Some(Value::Object(fields)) if fields.is_empty() => None,
            Some(payload) => Some(payload.to_string()),
            None => None,
        },
        _ => None,
    };
    Some((spec.name.to_string(), params))
}

/// （目录名，参数 JSON）→ 生成器配置；目录里没有这个名字、或参数不是对象时返回 `None`
/// （由调用方退到按类型给的默认生成器）。
///
/// 认不出的参数键**跳过**（不整列作废）：参数集是脚本按变体派生的，旧记录里多出的键
/// 只说明这个变体改过字段——守住能守的部分比拒绝整列有用。
pub fn config_from_parts(name: &str, params: Option<&str>) -> Option<GeneratorConfig> {
    let mut config = generator_catalog::default_of(name)?;
    let Some(raw) = params else {
        return Some(config);
    };
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(raw) else {
        return Some(config);
    };
    for (key, value) in map {
        if let Some(next) = crate::mock_view::patch_param_value(&config, &key, value) {
            config = next;
        }
    }
    Some(config)
}

/// 语言入库值（与 `Locale` 的 serde 名一致：`ZH_CN` / `EN` …），重放用 [`locale_of`] 读回。
fn locale_token(locale: &Locale) -> String {
    serde_json::to_value(locale)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "ZH_CN".to_string())
}

/// 入库值 → 语言；认不出的（旧版本新增的语言）回默认，不拦重放。
fn locale_of(token: &str) -> Option<Locale> {
    serde_json::from_value(Value::String(token.to_string())).ok()
}

/// 时间标签：`2026-09-16T15:02:03+00:00` → `09-16 15:02`（认不出就原样返回）。
///
/// 历史行只有 280px 宽，完整时间戳会把表名挤没。
pub fn time_label(raw: &str) -> String {
    match DateTime::parse_from_rfc3339(raw) {
        Ok(stamp) => stamp.format("%m-%d %H:%M").to_string(),
        Err(_) => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    // 注意：不通配导入（`use gpui_kit::*` 会把 gpui 的 `test` 宏带入作用域）。
    use chrono::{DateTime, Utc};
    use serde_json::json;

    use super::{
        RunOutcome, RunRecord, SAVE_FORMAT_TABLE, config_from_parts, draft_of_detail,
        generator_parts, task_of_run, time_label,
    };
    use crate::mock_view::{
        MockColumnSpec, MockDraft, MockGenInfo, MockJobDone, MockJobKind, MockPreview,
        MockRunOptions,
    };
    use crate::models::{ColumnDataType, ColumnDef, GeneratorConfig, Locale};
    use crate::persistence::MockGenerationDetail;

    /// 三列草稿：无参生成器 / 带参生成器 / 复杂参数（集合），并带上空值率与唯一性。
    fn draft() -> MockDraft {
        let column = |id: u64,
                      name: &str,
                      data_type: ColumnDataType,
                      generator: GeneratorConfig,
                      nullable_ratio: f64,
                      unique: bool| MockColumnSpec {
            id,
            def: ColumnDef {
                name: name.to_string(),
                data_type,
                generator,
                nullable_ratio,
                unique,
                dependency: None,
            },
            confidence: "high".to_string(),
            sample_value: String::new(),
        };
        MockDraft {
            table_name: "orders".to_string(),
            columns: vec![
                column(
                    1,
                    "id",
                    ColumnDataType::Integer,
                    GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                    0.0,
                    false,
                ),
                column(
                    2,
                    "email",
                    ColumnDataType::Text,
                    GeneratorConfig::SafeEmail,
                    0.0,
                    true,
                ),
                column(
                    3,
                    "amount",
                    ColumnDataType::Decimal {
                        precision: 12,
                        scale: 2,
                    },
                    GeneratorConfig::RandomDecimal {
                        min: 1.0,
                        max: 9.0,
                        scale: 2,
                    },
                    0.0,
                    false,
                ),
                column(
                    4,
                    "status",
                    ColumnDataType::Text,
                    GeneratorConfig::Weighted {
                        choices: vec![("已发货".to_string(), 3.0), ("待发货".to_string(), 1.0)],
                    },
                    0.2,
                    false,
                ),
            ],
            options: MockRunOptions {
                rows: 500,
                seed: Some(7),
                locale: Locale::ZhCn,
            },
        }
    }

    fn succeeded() -> RunRecord {
        RunRecord {
            save_format: None,
            outcome: RunOutcome::Succeeded {
                rows: 500,
                elapsed_ms: Some(42),
            },
        }
    }

    fn gen_info() -> MockGenInfo {
        MockGenInfo {
            temp_table_name: "temp_mock_orders".to_string(),
            row_count: 500,
            elapsed_ms: 42,
            preview: MockPreview {
                columns: vec!["id".to_string()],
                rows: vec![vec!["1".to_string()]],
            },
        }
    }

    /// 同一个配置的两份值是否等价（`GeneratorConfig` 没有 `PartialEq`，用序列化形状比）。
    fn same_generator(left: &GeneratorConfig, right: &GeneratorConfig) -> bool {
        serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
    }

    #[test]
    fn a_run_is_recorded_with_its_columns() {
        let now = DateTime::parse_from_rfc3339("2026-09-16T15:02:03Z")
            .expect("时间戳可解析")
            .with_timezone(&Utc);
        let (task, columns) = task_of_run(&draft(), &succeeded(), now);

        assert_eq!(task.table_name, "orders");
        assert_eq!(task.row_count, 500);
        assert_eq!(task.seed, Some(7));
        assert_eq!(
            task.locale, "ZH_CN",
            "语言按 serde 名入库，与迁移 009 的默认值同形"
        );
        assert_eq!(task.status, "success");
        assert_eq!(task.error_message, None);
        assert_eq!(task.generated_rows, Some(500));
        assert_eq!(task.generation_time_ms, Some(42));
        assert_eq!(task.save_format, None, "只产临时表：没有落点");
        assert_eq!(
            task.created_at.as_deref(),
            Some("2026-09-16T15:02:03+00:00")
        );
        assert!(!task.id.is_empty());

        assert_eq!(columns.len(), 4);
        assert_eq!(
            columns
                .iter()
                .map(|column| column.sort_order)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3],
            "列序即数组序（读回时按它排）"
        );
        assert!(columns.iter().all(|column| column.task_id == task.id));
        assert_eq!(columns[0].generator, "auto_increment");
        assert_eq!(
            columns[0].generator_params.as_deref(),
            Some(r#"{"start":1,"step":1}"#),
            "参数是变体载荷，原样存（重放靠它还原区间 / 步长）"
        );
        assert_eq!(columns[1].generator, "safe_email");
        assert_eq!(columns[1].generator_params, None, "单元变体没有参数");
        assert!(columns[1].is_unique);
        assert_eq!(columns[2].generator, "random_decimal");
        assert_eq!(columns[2].column_type, "DECIMAL(12, 2)", "存建表类型");
        assert_eq!(columns[3].null_ratio, 0.2);
        assert_eq!(columns[3].confidence.as_deref(), Some("high"));
        assert!(!columns[0].is_unique);
        assert!(columns.iter().all(|column| !column.is_foreign_key));
    }

    #[test]
    fn failed_runs_keep_the_reason_and_have_no_rows() {
        let run = RunRecord {
            save_format: Some(SAVE_FORMAT_TABLE.to_string()),
            outcome: RunOutcome::Failed {
                reason: "项目分析库已存在表 orders".to_string(),
            },
        };
        let (task, _) = task_of_run(&draft(), &run, Utc::now());

        assert_eq!(task.status, "failed");
        assert_eq!(task.error_message.as_deref(), Some("项目分析库已存在表 orders"));
        assert_eq!(task.generated_rows, None);
        assert_eq!(task.generation_time_ms, None);
        assert_eq!(task.save_format.as_deref(), Some("table"));
    }

    #[test]
    fn only_runs_that_produce_data_are_recorded() {
        let draft = draft();
        let done = Ok(MockJobDone::Generated(gen_info()));
        assert!(RunRecord::of(&MockJobKind::Generate, &done, &draft).is_some());

        let appended = Ok(MockJobDone::Appended {
            table: "orders".to_string(),
            total_rows: 900,
        });
        let record = RunRecord::of(
            &MockJobKind::AppendTo("orders".to_string()),
            &appended,
            &draft,
        )
        .expect("追加也是一次生成");
        assert_eq!(record.save_format.as_deref(), Some(SAVE_FORMAT_TABLE));
        assert_eq!(
            record.outcome,
            RunOutcome::Succeeded {
                rows: 500,
                elapsed_ms: None,
            }
        );

        // 出口类不产新配置：记了只是重复上一条
        assert!(RunRecord::of(&MockJobKind::Persist(gen_info()), &done, &draft).is_none());
        assert!(
            RunRecord::of(
                &MockJobKind::Export {
                    info: gen_info(),
                    format: crate::models::MockExportFormat::Csv,
                    path: "/tmp/orders.csv".to_string(),
                },
                &Ok(MockJobDone::Exported {
                    message: "已导出".to_string(),
                }),
                &draft,
            )
            .is_none()
        );

        // 取消的运行没有产出，不该挤掉真记录
        assert!(
            RunRecord::of(
                &MockJobKind::Generate,
                &Err("生成已取消".to_string()),
                &draft
            )
            .is_none()
        );
        // 而失败要记：历史行会把原因展出来
        let failed = RunRecord::of(&MockJobKind::Generate, &Err("表名非法".to_string()), &draft)
            .expect("失败也记");
        assert_eq!(
            failed.outcome,
            RunOutcome::Failed {
                reason: "表名非法".to_string()
            }
        );
    }

    #[test]
    fn replay_restores_the_configuration() {
        let source = draft();
        let (task, columns) = task_of_run(&source, &succeeded(), Utc::now());
        let replayed = draft_of_detail(&MockGenerationDetail { task, columns });

        assert_eq!(replayed.table_name, "orders");
        assert_eq!(replayed.options.rows, 500);
        assert_eq!(replayed.options.seed, Some(7));
        assert_eq!(replayed.options.locale, Locale::ZhCn);
        assert_eq!(replayed.columns.len(), 4);
        assert_eq!(replayed.columns[0].id, 1, "列 id 从 1 起编（面板会重编号）");
        assert_eq!(replayed.columns[0].def.name, "id");
        assert!(same_generator(
            &replayed.columns[0].def.generator,
            &GeneratorConfig::AutoIncrement { start: 1, step: 1 }
        ));
        assert!(same_generator(
            &replayed.columns[1].def.generator,
            &GeneratorConfig::SafeEmail
        ));
        assert!(replayed.columns[1].def.unique);
        assert!(same_generator(
            &replayed.columns[2].def.generator,
            &GeneratorConfig::RandomDecimal {
                min: 1.0,
                max: 9.0,
                scale: 2
            }
        ));
        assert!(same_generator(
            &replayed.columns[3].def.generator,
            &GeneratorConfig::Weighted {
                choices: vec![("已发货".to_string(), 3.0), ("待发货".to_string(), 1.0)]
            }
        ));
        assert_eq!(replayed.columns[3].def.nullable_ratio, 0.2);
        assert_eq!(replayed.columns[0].confidence, "high");
        assert_eq!(replayed.columns[0].sample_value, "");
    }

    #[test]
    fn generator_parts_round_trip_through_the_catalog() {
        for config in [
            GeneratorConfig::AutoIncrement { start: 5, step: 2 },
            GeneratorConfig::RandomInt { min: 3, max: 17 },
            GeneratorConfig::RandomFloat {
                min: 1.5,
                max: 9.25,
                precision: 3,
            },
            GeneratorConfig::Sentence { min: 2, max: 4 },
            GeneratorConfig::SafeEmail,
            GeneratorConfig::Weighted {
                choices: vec![("a".to_string(), 0.5), ("b".to_string(), 0.5)],
            },
        ] {
            let (name, params) = generator_parts(&config).expect("目录里认得出这个变体");
            let back = config_from_parts(&name, params.as_deref()).expect("目录里认得出这个标识");
            assert!(same_generator(&config, &back), "{name} 往返后应等价");
        }
    }

    #[test]
    fn unknown_generators_and_params_do_not_break_replay() {
        // 目录里没有的名字：交给调用方退到按类型给的默认生成器
        assert!(config_from_parts("不存在的生成器", None).is_none());
        // 认得的名字 + 认不得的参数键：守住能守的部分，不整列作废
        let back = config_from_parts("random_int", Some(r#"{"min":5,"不存在的键":1}"#))
            .expect("名字认得就应给出一份配置");
        let expected = config_from_parts("random_int", None).expect("默认配置");
        assert!(!same_generator(&back, &expected), "min 应按记录改掉");
        let payload = serde_json::to_value(&back).expect("可序列化");
        assert_eq!(payload["randomInt"]["min"], json!(5));
        // 参数串不是对象（旧记录 / 手改）：保留默认值
        assert!(config_from_parts("random_int", Some("不是 JSON")).is_some());
    }

    #[test]
    fn time_labels_are_short_and_never_panic() {
        assert_eq!(time_label("2026-09-16T15:02:03+00:00"), "09-16 15:02");
        assert_eq!(time_label("2026-09-16T15:02:03.123Z"), "09-16 15:02");
        // 认不出就原样返回（渲染层的 text_ellipsis 兜底）
        assert_eq!(time_label("不是时间"), "不是时间");
        assert_eq!(time_label(""), "");
    }
}
