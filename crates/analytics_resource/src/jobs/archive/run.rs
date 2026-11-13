//! 归档 / 取回 / 撤销 / 批量移入回收站的执行器。

use super::*;
use crate::ArchiveService;

#[cfg(test)]
#[path = "run_tests.rs"]
mod run_tests;

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
    let service = match open_service(job.project_root.clone(), Some(job.keep_versions)).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "归档",
                reason,
            };
        }
    };
    run_archive_with_service(job, &service).await
}

/// 可注入服务的执行体：测试用同一份测试库跑完整链路，作业层不必另开一个库。
async fn run_archive_with_service(job: &ArchiveJob, service: &ArchiveService) -> OpOutcome {
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
    // 取回凭据反查（迁移 030）：把「取回 → 改 → 再归档」接回原存档升版。
    //
    // 反查放在**作业线程**——宿主事件路径按架构约束不做 I/O；且只在 UI 没有显式指定
    // 目标存档时生效，因此 ArchiveRequest 的所有构造点、归档对话框、草稿箱都无需改动。
    // 查不到就走原来的新建语义，行为完全不变。
    let mut receipt_dest: Option<std::path::PathBuf> = None;
    if request.existing_resource_id.is_none() {
        match service.store().find_checkout(&request.source_path).await {
            Ok(Some(receipt)) => {
                // 两道边界：① 源存档已删 / 进回收站时忽略凭据，不把正常的新建拖成失败；
                // ② reference 档的本体在外部源，取回得到的是草稿副本——若凭草稿升版，
                //    索引指纹会指向草稿内容、而 source_path 仍指外部源，两者不一致。
                //    reference 的「内容变了」走详情面板的「接受当前内容」（显式指定 id）。
                let alive = service
                    .store()
                    .get_resource_by_id(&receipt.resource_id)
                    .await
                    .map(|row| {
                        row.deleted_at.is_none()
                            && crate::model::PayloadMode::from_db_str(&row.payload_mode)
                                == crate::model::PayloadMode::Copy
                    })
                    .unwrap_or(false);
                if alive {
                    request.existing_resource_id = Some(receipt.resource_id.clone());
                    receipt_dest = Some(request.source_path.clone());
                    tracing::info!(
                        resource_id = %receipt.resource_id,
                        from_version = receipt.from_version,
                        "命中取回凭据，本次归档回到原存档升版"
                    );
                }
            }
            Ok(None) => {}
            Err(error) => {
                // 老库缺 030 表 / 读失败：退回新建语义，不阻断归档，但要说清。
                tracing::warn!(error = %error, "取回凭据反查失败，按新建归档处理");
            }
        }
    }

    match service.archive(request.clone()).await {
        Ok(outcome) => {
            // 业务断言是附属项：写失败不撤销已成功的归档（本体与登记都已成立），
            // 但必须**同时**打日志与写进回执——只打日志等于用户以为口径存上了，
            // 而本模块对「附属项失败」的既定口径就是「失败不回滚、但要说清」。
            let mut notes = outcome.notes;
            if let Some(assertion) = &job.assertion {
                if let Err(error) = service
                    .set_assertion(&outcome.resource_id, assertion.clone())
                    .await
                {
                    tracing::warn!(
                        error = %error,
                        resource_id = %outcome.resource_id,
                        "业务断言写入失败（归档本身已成功）"
                    );
                    notes.push(format!("业务断言未保存：{error}（可在归档后补填）"));
                }
            }
            // 结案取回凭据：只有真的生成了新版本才记「取回 → 升版」，幂等返回不算。
            // 写失败不撤销已成功的归档，但打日志——否则时间线会缺一条回溯链。
            if outcome.created_new_version {
                if let Some(dest) = receipt_dest.as_deref() {
                    match service.store().consume_checkout(dest, outcome.version).await {
                        Ok(()) => notes.push("已按取回凭据回到原存档升版，资产履历可回溯".to_string()),
                        Err(error) => {
                            tracing::warn!(error = %error, "取回凭据结案失败（归档已成功）")
                        }
                    }
                }
            }
            // 首次归档才给撤销凭据：再归档的"撤销"是版本回退（服务层也会挡），不在这条路上。
            // 用**反查之后**的 request 判定，否则凭凭据升版也会错误地给出撤销入口。
            let undo = request
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
                notes,
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
    run_checkout_with_service(job, &service).await
}

async fn run_checkout_with_service(job: &CheckoutJob, service: &ArchiveService) -> OpOutcome {
    let dest = free_dest(&job.request.dest_path);
    let request = CheckoutRequest {
        resource_id: job.request.resource_id.clone(),
        dest_path: dest.clone(),
    };
    match service.checkout(request).await {
        Ok(outcome) => {
            // 取回凭据：工作副本路径 → 来源存档与版本。用**最终落点**（free_dest 之后），
            // 否则重名避让产生的 `-2` 路径会让反查永远落空。
            // 写失败不撤销已成功的取回，但必须打 error：静默失败等于承诺了做不到的升版。
            if let Err(error) = service
                .store()
                .record_checkout(&outcome.dest_path, &outcome.resource_id, outcome.version)
                .await
            {
                tracing::error!(error = %error, "取回凭据写入失败：改完再归档将无法升版");
            }
            OpOutcome::CheckedOut {
                dest: outcome.dest_path,
                version: outcome.version,
                open_after: job.open_after,
            }
        }
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
