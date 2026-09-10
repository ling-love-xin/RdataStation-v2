use crate::core::error::{common_err, CoreError};
use crate::core::models::QueryResult;
use crate::mock::{
    ColumnDef, ColumnMappingResponse, DependencyConfig, ImportSchemaInput, MockConfig, MockEngine,
    MockExportInput, MockGenerateResult, MockPersistAssetInput, MockPersistAssetResult,
    MockSaveToScratchpadInput, MockScenarioResult, ScenarioTemplate,
};
use tauri::Emitter;
use tauri::Manager;

#[tauri::command]
#[specta::specta]
pub async fn mock_generate(
    config: MockConfig,
    app: tauri::AppHandle,
) -> Result<MockGenerateResult, CoreError> {
    let app_clone = app.clone();
    MockEngine::generate_with_progress(config, move |current, total| {
        let _ = app_clone.emit(
            "mock:generate-progress",
            serde_json::json!({
                "current": current,
                "total": total
            }),
        );
    })
    .await
    .map_err(CoreError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn mock_preview(table_name: String, limit: usize) -> Result<QueryResult, CoreError> {
    MockEngine::preview(&table_name, limit).map_err(CoreError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn mock_export(input: MockExportInput) -> Result<String, CoreError> {
    MockEngine::export(
        &input.temp_table_name,
        &input.format,
        input.output_path.as_deref(),
        input.table_name.as_deref(),
    )
    .map_err(CoreError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn mock_map_column(
    column_name: String,
    data_type: String,
) -> Result<ColumnMappingResponse, CoreError> {
    MockEngine::map_column(&column_name, &data_type).map_err(CoreError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn mock_map_columns_batch(
    columns: Vec<(String, String)>,
) -> Result<Vec<ColumnMappingResponse>, CoreError> {
    MockEngine::map_columns_batch(columns).map_err(CoreError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn mock_list_templates() -> Result<Vec<ScenarioTemplate>, CoreError> {
    MockEngine::list_templates().map_err(CoreError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn mock_import_schema(input: ImportSchemaInput) -> Result<Vec<ColumnDef>, CoreError> {
    MockEngine::import_schema(&input).map_err(CoreError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn mock_apply_template(template_id: String) -> Result<ScenarioTemplate, CoreError> {
    MockEngine::apply_template(&template_id).map_err(CoreError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn mock_save_to_scratchpad(
    input: MockSaveToScratchpadInput,
    app_handle: tauri::AppHandle,
) -> Result<String, CoreError> {
    let scratchpad_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| common_err(format!("Failed to get app data dir: {}", e)))?
        .join("scratchpad")
        .join("mock");

    std::fs::create_dir_all(&scratchpad_dir)
        .map_err(|e| common_err(format!("Failed to create scratchpad/mock dir: {}", e)))?;

    let scratchpad_path = scratchpad_dir
        .to_str()
        .ok_or_else(|| common_err("Invalid scratchpad dir path"))?;

    MockEngine::save_to_scratchpad(&input.temp_table_name, &input.format, scratchpad_path)
        .map_err(CoreError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn mock_persist_as_asset(
    input: MockPersistAssetInput,
) -> Result<MockPersistAssetResult, CoreError> {
    let (table_name, row_count, column_count) =
        MockEngine::persist_as_asset(&input.temp_table_name, &input.name)
            .map_err(CoreError::from)?;

    Ok(MockPersistAssetResult {
        table_name,
        row_count: row_count as i32,
        column_count,
    })
}

#[tauri::command]
#[specta::specta]
pub fn mock_resolve_dependencies(columns: Vec<ColumnDef>) -> Result<DependencyConfig, CoreError> {
    Ok(MockEngine::resolve_dependencies(&columns))
}

#[tauri::command]
#[specta::specta]
pub async fn mock_cancel_generate() -> Result<(), CoreError> {
    MockEngine::cancel();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn mock_generate_scenario(
    template_id: String,
    app: tauri::AppHandle,
) -> Result<MockScenarioResult, CoreError> {
    let template = MockEngine::apply_template(&template_id).map_err(CoreError::from)?;

    let app_clone = app.clone();
    MockEngine::generate_scenario(&template, move |current, total| {
        let _ = app_clone.emit(
            "mock:scenario-progress",
            serde_json::json!({
                "current": current,
                "total": total
            }),
        );
    })
    .await
    .map_err(CoreError::from)
}
