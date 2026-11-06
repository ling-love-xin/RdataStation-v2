//! 归档 / 取回 / 撤销 / 批量移入回收站的执行器。

use super::*;

/// 执行一次归档。
/// 探一个数据文件，拼成 [`AnalysisFacts`]（工作线程上执行）。
///
/// 用全局内存 DuckDB 连接（`DuckDBManager::global`）：它已经配好扩展目录与文件读取器，
/// 不必为一个探测另开库；锁住期间只读文件，不与其它分析争写。
fn probe_analysis_facts(path: &std::path::Path) -> Result<AnalysisFacts, String> {
    let path_text = path.to_string_lossy().to_string();
    let definition = engine::duckdb::file_select_sql(&path_text)
        .ok_or_else(|| format!("没有可用的读取器：{path_text}"))?;
    let conn = engine::duckdb::DuckDBManager::get_or_create_in_memory()
        .map_err(|e| format!("打开分析引擎失败：{e}"))?;
    let guard = conn.lock().map_err(|e| format!("分析引擎繁忙：{e}"))?;
    let probe = engine::duckdb::probe_file(&guard, &path_text).map_err(|e| e.to_string())?;
    Ok(AnalysisFacts::from_probe(
        Some(definition),
        probe.columns,
        probe.row_count,
    ))
}

pub(crate) async fn run_archive(job: &ArchiveJob) -> OpOutcome {
    let name = job.request.name.clone();
    // 采集侧（P4.1 第二半）：数据文件先探一次结构，指纹按“定义 + 结构”走。
    let mut request = job.request.clone();
    if job.analysis_probe {
        match probe_analysis_facts(&request.source_path) {
            Ok(facts) => request.analysis = Some(facts),
            Err(reason) => {
                // 降级而不是拒归档：文件先安全落进 `resources/`，按文件型登记指纹。
                tracing::warn!(error = %reason, path = %request.source_path.display(),
                    "数据文件探测失败，按文件型归档");
                request.kind = ArchiveKind::File;
                request.analysis = None;
            }
        }
    }
    let service = match open_service(job.project_root.clone(), Some(job.keep_versions)).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "归档",
                reason,
            };
        }
    };
    match service.archive(request.clone()).await {
        Ok(outcome) => {
            // 首次归档才给撤销凭据：再归档的"撤销"是版本回退（服务层也会挡），不在这条路上。
            let undo = job
                .request
                .existing_resource_id
                .is_none()
                .then(|| ArchiveUndo {
                    resource_id: outcome.resource_id.clone(),
                    name: name.clone(),
                    source_path: job.request.source_path.clone(),
                });
            OpOutcome::Archived {
                resource_id: outcome.resource_id.clone(),
                name,
                version: outcome.version,
                rel_path: outcome.file_rel_path,
                undo,
                notes: outcome.notes,
            }
        }
        Err(error) => OpOutcome::Failed {
            action: "归档",
            reason: error.to_string(),
        },
    }
}

/// 执行一次取回（检出）。
///
/// 落点在动文件**之前**定死（重名避让），回执里给的就是真实路径：用户看到"取回为 X"
/// 之后去草稿箱找 X，必须找得到。
pub(crate) async fn run_checkout(job: &CheckoutJob) -> OpOutcome {
    let service = match open_service(job.project_root.clone(), None).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "取回",
                reason,
            };
        }
    };
    let dest = free_dest(&job.request.dest_path);
    let request = CheckoutRequest {
        resource_id: job.request.resource_id.clone(),
        dest_path: dest.clone(),
    };
    match service.checkout(request).await {
        Ok(outcome) => OpOutcome::CheckedOut {
            dest: outcome.dest_path,
            version: outcome.version,
            open_after: job.open_after,
        },
        Err(error) => OpOutcome::Failed {
            action: "取回",
            reason: error.to_string(),
        },
    }
}

/// 执行一次撤销归档。
pub(crate) async fn run_undo(job: &UndoJob) -> OpOutcome {
    let service = match open_service(job.project_root.clone(), None).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "撤销归档",
                reason,
            };
        }
    };
    match service.undo_archive(&job.undo).await {
        Ok(()) => OpOutcome::Undone {
            name: job.undo.name.clone(),
        },
        Err(error) => OpOutcome::Failed {
            action: "撤销归档",
            reason: error.to_string(),
        },
    }
}

/// 执行一次移入回收站（批量）。
///
/// 逐条走服务层；服务层不做预回滚（部分成功就部分成功），所以错误文案里会带“已移入 N 项”
/// ——宿主只需原样转述，不要自己算进度。
pub(crate) async fn run_trash(job: &TrashJob) -> OpOutcome {
    let service = match open_service(job.project_root.clone(), None).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "移入回收站",
                reason,
            };
        }
    };
    match service.move_to_trash(&job.resource_ids).await {
        Ok(entries) => OpOutcome::Trashed {
            names: entries.into_iter().map(|entry| entry.name).collect(),
        },
        Err(error) => OpOutcome::Failed {
            action: "移入回收站",
            reason: error.to_string(),
        },
    }
}
