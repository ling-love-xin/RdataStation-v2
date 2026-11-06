//! 详情动作：打开、统计、取回、升级/分发、指纹核对、版本历史。

use super::*;

pub(super) fn request_open(host: &ResourceHost, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {

        let Some(path) = host.payload_path(detail, cx) else {
            return;
        };
        // reference 懒校验（§12.1.1 ②）：源在位（`payload_path` 已守卫存在性）但内容与登记
        // 指纹不一致时**不打开**，置失效槽位交详情面板处置；旧行无指纹基准就不挡（与
        // indexer 旧行口径一致）。校验只在主动打开时做，不进索引状态统计。
        if detail.payload_mode == PayloadMode::Reference {
            if let Some(expected) = detail.content_hash.as_deref() {
                match PayloadStore::content_hash_sync(&path) {
                    Ok(actual) if actual != expected => {
                        host.backend.set_reference_stale(&detail.id, true);
                        host.backend.refresh_resources(cx);
                        host.notice(
                            "资产库：源已变更，快照失效——可在详情面板「接受当前内容」或「从历史还原」",
                            cx,
                        );
                        return;
                    }
                    Ok(_) => {
                        host.backend.set_reference_stale(&detail.id, false);
                    }
                    Err(error) => {
                        host.notice(format!("资产库：引用源校验失败（{error}）"), cx);
                        return;
                    }
                }
            }
        }
        // 分析表（数据文件）**不走编辑器文本**：双击/打开要看的是数据本身。
        //
        // 走洞察的取样入口（与右键「查看统计」同一个）：那边把本体当只读源表读出来
        // （D58：取样 → 临时表），用户能直接看到这张表的形状与数据。
        //
        // 注：原型 P4.4 想要的是“编辑器结果表格（只读）”，那需要编辑器侧先开一个
        // “以结果区打开数据文件”的 API（现在只有“打开文本 + 执行”），属 P4.4 余项；
        // 在此之前“双击能看见数据”这件事由洞察入口承担，比摆一个点了没反应的按钮强。
        if detail.kind == ArchiveKind::Analysis {
            match insight::SampleSource::duckdb_file(&path, detail.name.clone()) {
                Ok(source) => {
                    let name = detail.name.clone();
                    host.backend
                        .open_insight_source_table(source, name.clone(), cx);
                    host.notice(
                        format!("资产库：已在洞察面板打开「{name}」（取样）——本体仍只读"),
                        cx,
                    );
                }
                Err(error) => host.notice(format!("资产库：{error}"), cx),
            }
            return;
        }
        // **编辑器只读**：改它要先去取回（本体不可写是模块硬约束 2）。
        host.backend.request_open_in_editor_read_only(path, cx);
}

pub(super) fn request_view_stats(host: &ResourceHost, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {

        // 面板只给「能取到数」的行摆入口（`can_view_stats`），这里仍按种类分流：
        // 走得通的那类把**取样来源**交给洞察侧（D58），走不通的给回执而不是静默。
        match detail.kind {
            // 受管文件：本体交给 DuckDB 直接读（CSV / Parquet / Excel / JSON）。
            ArchiveKind::File => {
                let Some(path) = host.payload_path(detail, cx) else {
                    return;
                };
                match insight::SampleSource::duckdb_file(&path, detail.name.clone()) {
                    Ok(source) => {
                        host.backend
                            .open_insight_source_table(source, detail.name.clone(), cx)
                    }
                    // 面板已经按扩展名挡过一次：走到这里说明是可读格式但读不到
                    // （扩展名对不上 / 路径刚被挪走），把真实原因说清。
                    Err(error) => host.notice(format!("资产库：{error}"), cx),
                }
            }
            // 远端引用：没有本体，按来源连接重新取样（连接不在或已删时报给洞察侧）。
            ArchiveKind::TableRef => {
                let (Some(conn_id), Some(table)) = (
                    detail.source_connection_id.as_deref(),
                    detail.source_table.as_deref(),
                ) else {
                    host.notice("资产库：这条引用没有来源连接或表名，无法取样", cx);
                    return;
                };
                // 归档侧只存了一个 `schema.table` 字符串（见 `ArchiveBinding`），
                // 所以按点拆段后交给共用助手加引号（与导航树「查看统计」同口径）。
                let parts: Vec<&str> = table.split('.').collect();
                let sql = host.backend.insight_sample_sql(conn_id, &parts);
                let source =
                    insight::SampleSource::new(conn_id.to_string(), sql, detail.name.clone());
                host.backend
                    .open_insight_source_table(source, detail.name.clone(), cx);
            }
            // 分析表：本体是项目内的 `analytics.duckdb`，要 ATTACH + 用重建定义取数
            // （不是“读一个文件”那么简单）——随后续批次接（面板据此也不给入口）。
            ArchiveKind::Analysis => {
                host.notice("资产库：分析表型存档的洞察随 M8 后续批次接入", cx);
            }
        }
}

pub(super) fn request_checkout(host: &ResourceHost, detail: &ArchiveDetail, window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法取回", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能取回", cx);
            return;
        }
        // 目标目录 = 草稿箱模块目录（取回产出的是一份**草稿**；目录名取自 crate 常量，
        // 不在宿主里再写一次字面量）。
        let target_dir = root.join(scratchpad::MODULE_DIR_NAME);
        let extension = detail
            .payload_rel_path
            .as_deref()
            .and_then(|rel| std::path::Path::new(rel).extension())
            .and_then(|ext| ext.to_str());
        let seed = CheckoutDialogSeed {
            resource_name: detail.name.clone(),
            file_name: suggest_work_copy_name(&detail.name, extension),
            target_dir_label: target_dir.to_string_lossy().to_string(),
            version: detail.version,
        };

        let backend = host.backend.clone();
        let resource_id = detail.id.clone();
        let read_only = host.read_only();
        open_checkout_dialog(window, cx, seed, move |result, cx| {
            let request = CheckoutRequest {
                resource_id: resource_id.clone(),
                dest_path: target_dir.join(&result.file_name),
            };
            resource_jobs::enqueue_checkout(root.clone(), read_only, request, result.open_after);
            backend.refresh_resources(cx);
            backend.notice("资产库：正在取回…".to_string(), cx);
        });
}

pub(super) fn request_promote_to_global(host: &ResourceHost, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {

        use crate::global::{PromoteKind, PromoteRequest};
        let Some(root) = host.require_project("无法升级到全局", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能升级到全局", cx);
            return;
        }
        // File 档才有本体相对路径；分析表型存档的本体在项目 `analytics.duckdb`，
        // 数据读取 / 重建路径未接（resource_host.rs::request_open 同注），升级随 M8 后续批次。
        let Some(rel) = detail.payload_rel_path.as_deref() else {
            host.notice("资产库：分析表型存档的升级随 M8 后续批次接入", cx);
            return;
        };
        let abs = root.join("resources").join(rel);
        if !abs.exists() {
            host.notice(format!("资产库：本体文件不存在（{rel}），无法升级"), cx);
            return;
        }
        // 形态判定：数据文件 → 映射出表；文本脚本 → 内容表；其余（png / zip / docx 这类
        // 二进制）**不能升级**。加固前一律落到 `PromoteKind::Script` 的 `read_to_string`，
        // 撞 UTF-8 失败后把 `stream did not contain valid UTF-8` 原样丢给用户。
        let kind = if is_data_file_ext(&abs) {
            PromoteKind::DataFile
        } else if is_text_script_ext(&abs) {
            PromoteKind::Script
        } else {
            let ext = abs
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("（无扩展名）")
                .to_ascii_lowercase();
            host.notice(
                format!(
                    "资产库：{ext} 这类文件不能升级到全局——全局库只收能读成表的数据文件\
（csv / tsv / parquet / xlsx / json 等）或文本脚本（sql / py / md 等）。\
内容是数据的话，另存为 csv 再升级；纯二进制文件请留在项目里，用「分发」或共享目录传。"
                ),
                cx,
            );
            return;
        };
        resource_jobs::enqueue_promote(
            root,
            PromoteRequest {
                name: detail.name.clone(),
                kind,
                source_abs_path: abs,
                rel_path: Some(rel.to_string()),
                content_hash: detail.content_hash.clone().unwrap_or_default(),
                file_size: None,
            },
            false,
        );
        host.backend.refresh_resources(cx);
        host.notice("资产库：正在升级到全局…", cx);
}

pub(super) fn request_distribute_from_global(host: &ResourceHost, name: &str, _window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法从全局分发", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能从全局分发", cx);
            return;
        }
        // 分发 = 跨项目唯一法定路径（全局 → 当前项目）；目标重名由工作线程拦下回执。
        resource_jobs::enqueue_distribute(root, name.to_string(), None, false);
        host.backend.refresh_resources(cx);
        host.notice("资产库：正在从全局分发…", cx);
}

pub(super) fn is_reference_stale(host: &ResourceHost, resource_id: &str) -> bool {

        host.backend.is_reference_stale(resource_id)
}

pub(super) fn accept_reference_current(host: &ResourceHost, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法接受当前内容", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能接受当前内容", cx);
            return;
        }
        let Some(source) = detail.source_path.as_deref() else {
            host.notice("资产库：引用型存档没有登记源路径（数据异常）", cx);
            return;
        };
        // 接受当前内容 = 以当前源绝对路径为基准再归档（reference 分支：旧源留版本副本，
        // 只更新指纹 / 规模 / 版本，不覆盖本体；服务层 `archive_into_existing` 口径）。
        // 标签 / 分组沿用现状：空 tags + None group_id 时服务层不改动既有归属。
        let request = ArchiveRequest {
            source_path: PathBuf::from(source),
            rel_path: detail.payload_rel_path.clone().unwrap_or_default(),
            name: detail.name.clone(),
            alias: detail.alias.clone(),
            kind: detail.kind,
            payload_mode: PayloadMode::Reference,
            analysis: None,
            binding: ArchiveBinding {
                promoted_from: None,
                source_connection_id: None,
                source_table: None,
            },
            tags: Vec::new(),
            group_id: None,
            keep_versions: None,
            existing_resource_id: Some(detail.id.clone()),
        };
        let keep_versions = host.backend.keep_versions(cx);
        resource_jobs::enqueue_archive(root, host.read_only(), request, keep_versions, false);
        host.backend.refresh_resources(cx);
        host.notice("资产库：正在接受当前内容并生成新版本…", cx);
}

pub(super) fn request_version_history(host: &ResourceHost, resource_id: &str, _window: &mut Window, cx: &mut App) {

        // 取数在后台线程：版本行要读索引表 + 问 `.RSmeta` 下的副本清单，
        // 回来之后由侧栏轮询开窗（开窗要 `Window`，轮询任务里没有）。
        let Some(root) = host.require_project("无法打开版本历史", cx) else {
            return;
        };
        resource_jobs::enqueue_versions(root, resource_id.to_string());
        host.notice("资产库：正在读取版本历史…", cx);
}
