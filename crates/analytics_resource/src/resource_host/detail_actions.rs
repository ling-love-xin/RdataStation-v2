//! 详情动作：打开、统计、取回、升级/分发、指纹核对、版本历史。

use shared::i18n::tr;
use super::*;

/// 表型取数的唯一入口（双击打开与「查看统计」共用）。
///
/// 表型资产的本体是**分析库里的持久表**，没有文件路径可走——过去这里拿不到
/// `payload_path` 就静默返回，用户看到的是"点了没反应"。现在按登记表名直接取样：
/// `SELECT * FROM "<表名>"` 交给洞察侧当只读源表读出来（`on_duckdb` 绑定到当前
/// 项目分析库，表就在那里）。
fn open_analytics_table(host: &ResourceHost, detail: &ArchiveDetail, cx: &mut App) {
    let Some(table) = detail.analytics_table.as_deref() else {
        host.notice(tr("资产库：这条表型资产没有登记表名，无法取样"), cx);
        return;
    };
    // 全局行的本体在**全局**分析库（另一个文件）：只能经短命会话挂 `rds_global`
    // 读；项目行的表就在当前项目库里，直取即可。
    let source = match detail.scope {
        crate::model::RowScope::Global => {
            insight::SampleSource::on_global(table, detail.name.clone())
        }
        _ => {
            let sql = format!("SELECT * FROM {};", crate::global::quote_ident(table));
            insight::SampleSource::on_duckdb(sql, detail.name.clone())
        }
    };
    host.backend
        .open_insight_source_table(source, detail.name.clone(), cx);
}

/// 全局引用：按引用行登记的全局实际表名，短命只读挂载取样（与全局行同一路径）。
fn open_global_ref(host: &ResourceHost, detail: &ArchiveDetail, cx: &mut App) {
    let Some(table) = detail.source_table.as_deref() else {
        host.notice(tr("资产库：这条全局引用没有登记全局表名，无法取样"), cx);
        return;
    };
    let label = detail.alias.clone().unwrap_or_else(|| detail.name.clone());
    let source = insight::SampleSource::on_global(table, label.clone());
    host.backend.open_insight_source_table(source, label, cx);
    host.notice(
        format!(
            "资产库：已在洞察面板打开全局引用「{}」（取样）——全局表只读",
            detail.name
        ),
        cx,
    );
}

pub(super) fn request_open(host: &ResourceHost, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {

        // 全局引用：没有本地本体，按登记的实际全局表名只读取样。
        if detail.kind == ArchiveKind::TableRef
            && detail.origin_kind.as_deref() == Some(crate::model::ORIGIN_KIND_GLOBAL)
        {
            open_global_ref(host, detail, cx);
            return;
        }
        // 表型先行：它没有文件本体，不能走下面的 `payload_path`（那样必然静默失败）。
        if detail.kind == ArchiveKind::AnalyticsTable {
            open_analytics_table(host, detail, cx);
            host.notice(
                format!("资产库：已在洞察面板打开「{}」（取样）——本体仍只读", detail.name),
                cx,
            );
            return;
        }
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
        // **数据文件不走编辑器文本**：双击/打开要看的是数据本身；`.sql` / `.md` 这类不是
        // 数据，仍进编辑器。判定与「查看统计」同一口径——扩展名白名单只有一处
        // （`engine::duckdb::file_reader_function`），不按种类分。
        //
        // 走洞察的取样入口（与右键「查看统计」同一个）：那边把本体当只读源表读出来
        // （D58：取样 → 临时表），用户能直接看到这张表的形状与数据。
        //
        // 注：原型 P4.4 想要的是“编辑器结果表格（只读）”，那需要编辑器侧先开一个
        // “以结果区打开数据文件”的 API（现在只有“打开文本 + 执行”），属 P4.4 余项；
        // 在此之前“双击能看见数据”这件事由洞察入口承担，比摆一个点了没反应的按钮强。
        if detail
            .payload_rel_path
            .as_deref()
            .and_then(engine::duckdb::file_reader_function)
            .is_some()
        {
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
                if detail.origin_kind.as_deref() == Some(crate::model::ORIGIN_KIND_GLOBAL) {
                    open_global_ref(host, detail, cx);
                    return;
                }
                let (Some(conn_id), Some(table)) = (
                    detail.source_connection_id.as_deref(),
                    detail.source_table.as_deref(),
                ) else {
                    host.notice(tr("资产库：这条引用没有来源连接或表名，无法取样"), cx);
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
            // 表型：本体是项目 `analytics.duckdb` 里的持久表，按登记表名取样
            // （`analytics_table` 列是权威判据，见迁移 031）。
            ArchiveKind::AnalyticsTable => open_analytics_table(host, detail, cx),
        }
}

pub(super) fn request_checkout(host: &ResourceHost, detail: &ArchiveDetail, window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法取回", cx) else {
            return;
        };
        if host.read_only() {
            host.notice(tr("资产库：项目为只读模式，不能取回"), cx);
            return;
        }
        // 目标目录 = 草稿箱模块目录（取回产出的是一份**草稿**；目录名取自 crate 常量，
        // 不在宿主里再写一次字面量）。
        let target_dir = root.join(crate::resource_host::SCRATCHPAD_DIR);
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
            host.notice(tr("资产库：项目为只读模式，不能升级到全局"), cx);
            return;
        }
        // 两种本体：文件（`resources/<rel>`）与分析库里的一张表（表型存档）。
        // 表型不由这里读数据——工作线程用项目写连接整表导出（`source_table` 只给表名）。
        let (abs, rel_path, kind, source_table) = match detail.payload_rel_path.as_deref() {
            Some(rel) => {
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
                (abs, Some(rel.to_string()), kind, None)
            }
            None => {
                let Some(table) = detail.analytics_table.as_deref() else {
                    host.notice(
                        "资产库：这条存档没有登记本体表名（多半是迁移前的老行）。\
先在「索引修复」里认一下本体，再升级到全局",
                        cx,
                    );
                    return;
                };
                // 源库路径只作说明：真正打开哪个库由工作线程按项目根决定。
                (
                    root.join(crate::payload::META_DIR_NAME)
                        .join("analytics.duckdb"),
                    None,
                    PromoteKind::DataFile,
                    Some(table.to_string()),
                )
            }
        };
        resource_jobs::enqueue_promote(
            root,
            PromoteRequest {
                name: detail.name.clone(),
                kind,
                source_abs_path: abs,
                rel_path,
                content_hash: detail.content_hash.clone().unwrap_or_default(),
                file_size: None,
                // 来源链只记直接上游：全局行靠这两项回指「A 的哪一条、哪一版」。
                source_resource_id: Some(detail.id.clone()),
                source_version: Some(detail.version),
                source_table,
                // 一版一张表会增长：升级完按设置里的保留策略裁掉更早的表。
                keep_versions: Some(host.backend.keep_versions(cx)),
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
            host.notice(tr("资产库：项目为只读模式，不能从全局分发"), cx);
            return;
        }
        // 分发 = 跨项目唯一法定路径（全局 → 当前项目）；目标重名由工作线程拦下回执。
        resource_jobs::enqueue_distribute(root, name.to_string(), None, false);
        host.backend.refresh_resources(cx);
        host.notice("资产库：正在从全局分发…", cx);
}

pub(super) fn request_reference_from_global(host: &ResourceHost, name: &str, global_version: i32, _window: &mut Window, cx: &mut App) {
    let Some(root) = host.require_project("无法引用全局资产", cx) else {
        return;
    };
    if host.read_only() {
        host.notice(tr("资产库：项目为只读模式，不能引用全局资产"), cx);
        return;
    }
    // 引用 = 建 `table_ref` 行 + pin 全局版本；不复制数据，显示名沿用全局本名。
    resource_jobs::enqueue_reference(root, name.to_string(), global_version, None);
    host.backend.refresh_resources(cx);
    host.notice("资产库：正在引用到本项目…", cx);
}

pub(super) fn request_solidify_ref(host: &ResourceHost, resource_id: &str, project_version: i32, target: &str, _window: &mut Window, cx: &mut App) {
    let Some(root) = host.require_project("无法固化引用版本", cx) else {
        return;
    };
    if host.read_only() {
        host.notice(tr("资产库：项目为只读模式，不能固化引用版本"), cx);
        return;
    }
    resource_jobs::enqueue_solidify(root, resource_id.to_string(), project_version, target.to_string());
    host.backend.refresh_resources(cx);
    host.notice("资产库：正在固化引用版本…", cx);
}

/// 移入全局回收站（刀 B）：确认框说清「只软删全局这一份，表与版本仍在，可恢复」。
pub(super) fn request_delete_global(
    host: &ResourceHost,
    detail: &ArchiveDetail,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(root) = host.require_project("无法移入全局回收站", cx) else {
        return;
    };
    // 不检查项目只读：回收站只动全局登记，不写项目分析库 / resources。
    let name = detail.name.clone();
    // 分发面来自详情快照（`dispatch_label`）：让用户看见"这些项目里还有副本"再决定。
    let dispatched = if detail.dispatch_label.trim().is_empty() {
        "还没有分发到任何项目".to_string()
    } else {
        detail.dispatch_label.clone()
    };
    let description = format!(
        "将把全局分析库里的「{name}」当前版本移入全局回收站：只软删全局登记，表与版本快照都保留，之后可以从回收站恢复。\n{dispatched}：已分发到项目的副本不受影响，仍可继续使用。\n永久删除要再进回收站执行。"
    );
    let backend = host.backend.clone();
    window.open_alert_dialog(cx, move |alert, _window, _cx| {
        let backend = backend.clone();
        let root = root.clone();
        let name = name.clone();
        let description = description.clone();
        alert
            .confirm()
            .title("移入全局回收站？")
            .description(description)
            .button_props(
                gpui_kit::component::dialog::DialogButtonProps::default()
                    .ok_text("移入回收站")
                    .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                    .show_cancel(true),
            )
            .on_ok(move |_, _window, cx| {
                resource_jobs::enqueue_delete_global(root.clone(), name.clone());
                backend.refresh_resources(cx);
                say(&backend, "资产库：正在移入全局回收站…", cx);
                true
            })
    });
}

/// 查看来源链（刀 C）：从当前详情顺着 `origin_kind` / `origin_ref` 往上追，
/// 每一跳都去**面板当前快照**里按 id 找（不另查库；找不到就如实标灰）。
pub(super) fn request_show_origin_chain(
    host: &ResourceHost,
    detail: &ArchiveDetail,
    window: &mut Window,
    cx: &mut App,
) {
    let details: std::collections::HashMap<String, ArchiveDetail> = host
        .backend
        .snapshot_details(cx)
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect();
    let chain = crate::present::build_origin_chain(detail, &details);
    crate::dialogs::origin_chain::open_origin_chain(window, cx, chain);
}

pub(super) fn is_reference_stale(host: &ResourceHost, resource_id: &str) -> bool {

        host.backend.is_reference_stale(resource_id)
}

pub(super) fn accept_reference_current(host: &ResourceHost, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法接受当前内容", cx) else {
            return;
        };
        if host.read_only() {
            host.notice(tr("资产库：项目为只读模式，不能接受当前内容"), cx);
            return;
        }
        let Some(source) = detail.source_path.as_deref() else {
            host.notice(tr("资产库：引用型存档没有登记源路径（数据异常）"), cx);
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
        resource_jobs::enqueue_archive(root, host.read_only(), request, keep_versions, false, None);
        host.backend.refresh_resources(cx);
        host.notice(tr("资产库：正在接受当前内容并生成新版本…"), cx);
}

pub(super) fn request_version_history(host: &ResourceHost, resource_id: &str, _window: &mut Window, cx: &mut App) {

        // 取数在后台线程：版本行要读索引表 + 问 `.RSmeta` 下的副本清单，
        // 回来之后由侧栏轮询开窗（开窗要 `Window`，轮询任务里没有）。
        let Some(root) = host.require_project("无法打开版本历史", cx) else {
            return;
        };
        resource_jobs::enqueue_versions(root, resource_id.to_string());
        host.notice(tr("资产库：正在读取版本历史…"), cx);
}
