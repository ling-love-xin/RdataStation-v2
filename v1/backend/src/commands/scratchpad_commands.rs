use std::path::PathBuf;
use std::sync::atomic::Ordering;

use notify::Watcher;
use serde_json::Value;
use tauri::{Emitter, State};

use crate::commands::analytics_resource_commands::AnalyticsResourceState;
use crate::core::error::CoreError;
use crate::core::persistence::{AnalyticsResource, CreateResourceRequest};
use crate::core::scratchpad::{
    AnalyzableFile, DiffResult, ExternalReference, ReplaceResult, ScratchpadChangeEntry,
    ScratchpadChangeEvent, ScratchpadEntry, ScratchpadResponse, ScratchpadState, ScratchpadStore,
    SearchResult,
};

async fn get_store(state: &ScratchpadState) -> Result<ScratchpadStore, CoreError> {
    let guard = state.store.lock().await;
    guard
        .clone()
        .ok_or_else(|| CoreError::from("草稿板存储未初始化，请先打开项目"))
}

#[tauri::command]
pub async fn init_scratchpad_store(
    project_path: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<(), CoreError> {
    scratchpad_state.init(PathBuf::from(project_path)).await
}

#[tauri::command]
pub async fn list_scratchpad_files(
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<ScratchpadResponse, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .get_full_response()
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn list_scratchpad_directory(
    parent_path: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<Vec<ScratchpadEntry>, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .list_directory_entries(&parent_path)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn create_scratchpad_entry(
    name: String,
    parent_path: Option<String>,
    is_folder: bool,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<ScratchpadEntry, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    let parent = parent_path.as_deref();
    scratchpad
        .create_entry(&name, parent, is_folder)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn delete_scratchpad_entry(
    relative_path: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<(), CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .delete_entry(&relative_path)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn rename_scratchpad_entry(
    relative_path: String,
    new_name: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<ScratchpadEntry, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .rename_entry(&relative_path, &new_name)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn read_scratchpad_file(
    relative_path: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<String, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .read_file(&relative_path)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn save_scratchpad_file(
    relative_path: String,
    content: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<(), CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .save_file(&relative_path, &content)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn import_external_file(
    source_path: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<ScratchpadEntry, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    let source = PathBuf::from(&source_path);
    scratchpad
        .import_external_file(&source)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn add_external_reference(
    alias: String,
    path: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<ExternalReference, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    let ref_path = PathBuf::from(&path);
    scratchpad
        .add_external_reference(alias, ref_path)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn remove_external_reference(
    alias: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<(), CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .remove_external_reference(&alias)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn open_scratchpad_in_explorer(
    path: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<(), CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    let target = PathBuf::from(&path);
    scratchpad
        .open_in_system_explorer(&target)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn check_scratchpad_file_size(
    relative_path: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<u32, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .check_file_size(&relative_path)
        .await
        .map(|n| n as u32)
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn get_scratchpad_entry(
    relative_path: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<Option<ScratchpadEntry>, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .get_entry_metadata(&relative_path)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

// ==================== Analysis Commands ====================

#[tauri::command]
pub async fn get_analyzable_files(
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<Vec<AnalyzableFile>, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .get_analyzable_files()
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

// ==================== Trash Commands ====================

#[tauri::command]
pub async fn list_scratchpad_trash(
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<Vec<ScratchpadEntry>, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .list_trash()
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn restore_scratchpad_from_trash(
    trash_name: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<ScratchpadEntry, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .restore_from_trash(&trash_name)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn empty_scratchpad_trash(
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<(), CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .empty_trash()
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn update_scratchpad_file_meta(
    relative_path: String,
    connection_id: Option<String>,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<(), CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .update_file_meta(&relative_path, connection_id)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

#[tauri::command]
pub async fn search_scratchpad_content(
    query: String,
    case_sensitive: Option<bool>,
    context_lines: Option<usize>,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<SearchResult, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .search_file_content(
            &query,
            case_sensitive.unwrap_or(false),
            context_lines.unwrap_or(0),
        )
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

// ==================== File Watcher Commands ====================

#[tauri::command]
pub async fn watch_scratchpad(
    app: tauri::AppHandle,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<(), CoreError> {
    if scratchpad_state.is_watching() {
        return Ok(());
    }

    let scratchpad = get_store(&scratchpad_state).await?;
    let watch_dir = scratchpad.scratchpad_dir().to_path_buf();

    scratchpad
        .ensure_dir()
        .await
        .map_err(|e| CoreError::from(e.to_string()))?;

    let watch_dir_clone = watch_dir.clone();
    let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                for path in &event.paths {
                    if let Ok(relative) = path.strip_prefix(&watch_dir_clone) {
                        let rel = relative.to_string_lossy().to_string();
                        if rel.is_empty() {
                            continue;
                        }
                        let kind = match event.kind {
                            notify::EventKind::Create(_) => "create",
                            notify::EventKind::Modify(_) => "modify",
                            notify::EventKind::Remove(_) => "delete",
                            _ => "unknown",
                        };
                        if kind != "unknown" {
                            let _ = tx.send((rel, kind.to_string()));
                        }
                    }
                }
            }
        })
        .map_err(|e| CoreError::from(e.to_string()))?;

    watcher
        .watch(&watch_dir, notify::RecursiveMode::Recursive)
        .map_err(|e| CoreError::from(e.to_string()))?;

    scratchpad_state.set_watching(true);

    let watcher_flag = scratchpad_state.watcher_active.clone();

    tokio::task::spawn_blocking(move || {
        let _watcher = watcher;
        let debounce = std::time::Duration::from_millis(300);
        let atomic_save_window = std::time::Duration::from_millis(500);
        let mut pending_changes: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut last_flush = std::time::Instant::now();
        let mut recent_deletes: std::collections::HashMap<String, std::time::Instant> =
            std::collections::HashMap::new();

        loop {
            if !watcher_flag.load(Ordering::Relaxed) {
                break;
            }
            match rx.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok((path, kind)) => {
                    recent_deletes.retain(|_, t| t.elapsed() < atomic_save_window);

                    if kind == "create" {
                        if recent_deletes.remove(&path).is_some() {
                            pending_changes.insert(path, "modify".to_string());
                            continue;
                        }
                    } else if kind == "delete" {
                        recent_deletes.insert(path.clone(), std::time::Instant::now());
                    }

                    pending_changes.insert(path, kind);

                    let now = std::time::Instant::now();
                    if !pending_changes.is_empty() && now.duration_since(last_flush) >= debounce {
                        let changes: Vec<ScratchpadChangeEntry> =
                            std::mem::take(&mut pending_changes)
                                .into_iter()
                                .map(|(path, kind)| ScratchpadChangeEntry { path, kind })
                                .collect();
                        let payload = ScratchpadChangeEvent { changes };
                        let _ = app.emit("scratchpad-changed", payload);
                        last_flush = now;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !pending_changes.is_empty() && last_flush.elapsed() >= debounce {
                        let changes: Vec<ScratchpadChangeEntry> =
                            std::mem::take(&mut pending_changes)
                                .into_iter()
                                .map(|(path, kind)| ScratchpadChangeEntry { path, kind })
                                .collect();
                        let payload = ScratchpadChangeEvent { changes };
                        let _ = app.emit("scratchpad-changed", payload);
                        last_flush = std::time::Instant::now();
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    tracing::warn!("[Scratchpad] File watcher disconnected, stopping watch");
                    watcher_flag.store(false, Ordering::Relaxed);
                    break;
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn unwatch_scratchpad(
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<(), CoreError> {
    scratchpad_state.set_watching(false);
    Ok(())
}

// ==================== Move Command ====================

#[tauri::command]
pub async fn move_scratchpad_entry(
    from_path: String,
    to_parent_path: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<ScratchpadEntry, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .move_entry(&from_path, &to_parent_path)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

// ==================== Replace Command ====================

#[tauri::command]
pub async fn replace_scratchpad_content(
    path: String,
    pattern: String,
    replacement: String,
    is_regex: bool,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<ReplaceResult, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .replace_in_file(&path, &pattern, &replacement, is_regex)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

// ==================== Diff Command ====================

#[tauri::command]
pub async fn diff_scratchpad_with_content(
    relative_path: String,
    other_content: String,
    left_label: String,
    right_label: String,
    scratchpad_state: State<'_, ScratchpadState>,
) -> Result<DiffResult, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;
    scratchpad
        .diff_with_content(&relative_path, &other_content, &left_label, &right_label)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

// ==================== Promote Command ====================

fn extension_to_resource_type(ext: &str) -> &str {
    match ext {
        "sql" => "sql_script",
        "py" => "python_script",
        "csv" => "csv_data",
        "parquet" => "parquet_data",
        "json" => "json_data",
        "xlsx" => "excel_data",
        "txt" | "md" => "document",
        _ => "file",
    }
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct PromoteResult {
    pub resource: AnalyticsResource,
    pub removed: bool,
}

#[tauri::command]
pub async fn promote_scratchpad_to_resource(
    app: tauri::AppHandle,
    relative_path: String,
    remove_after: bool,
    scratchpad_state: State<'_, ScratchpadState>,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<PromoteResult, CoreError> {
    let scratchpad = get_store(&scratchpad_state).await?;

    let file_content = scratchpad
        .read_file(&relative_path)
        .await
        .map_err(|e| CoreError::from(e.to_string()))?;
    let file_size = scratchpad
        .check_file_size(&relative_path)
        .await
        .map_err(|e| CoreError::from(e.to_string()))?;

    let file_name = std::path::Path::new(&relative_path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| relative_path.clone());

    let ext = match std::path::Path::new(&relative_path)
        .extension()
        .and_then(|e| e.to_str())
    {
        Some(s) => s.to_lowercase(),
        None => String::new(),
    };

    let resource_type = extension_to_resource_type(&ext).to_string();

    let mut config = serde_json::Map::new();
    config.insert(
        "source".to_string(),
        Value::String("scratchpad".to_string()),
    );
    config.insert(
        "scratchpad_relative_path".to_string(),
        Value::String(relative_path.clone()),
    );

    if resource_type == "sql_script" {
        config.insert("sql".to_string(), Value::String(file_content.clone()));
    }

    let req = CreateResourceRequest {
        resource_type,
        name: file_name,
        alias: None,
        config: Value::Object(config),
        scope: "project".to_string(),
        row_count: None,
        column_count: None,
        file_size: Some(file_size as i32),
        parent_resource_id: None,
        source_query: if ext == "sql" {
            Some(file_content)
        } else {
            None
        },
    };

    let ar_store = analytics_state
        .store
        .get()
        .ok_or_else(|| CoreError::from("分析资源存储未初始化"))?;

    let resource = ar_store
        .create_resource(req)
        .await
        .map_err(|e| CoreError::from(e.to_string()))?;

    let _ = app.emit("analytics-resource-changed", ());

    let mut removed = false;
    if remove_after {
        scratchpad
            .delete_entry(&relative_path)
            .await
            .map_err(|e| CoreError::from(e.to_string()))?;
        removed = true;
    }

    Ok(PromoteResult { resource, removed })
}
