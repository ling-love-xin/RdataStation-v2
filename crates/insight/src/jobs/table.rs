//! 洞察接缝 · 表评估请求（自 jobs.rs 拆分，纯位移）

use super::*;

/// 「评估全表」：**逐列串行**回填事实明细并跑检查器，每列回来就回填一次（真实进度，不是转动图标）。
///
/// 串行是刻意选择：并发会撞上引擎的并发上限（D12），而失败重试反而让用户更困惑；
/// 一列一列跑既不会超限，又能把进度如实给出来。为了让进度真的动起来，每列单独 dispatch
/// 到后台执行器（而不是一整个循环全在后台），算完一列就回头更新一次面板。
/// 本模块只回填事实统计与检查器命中，**不合成质量总分 / 等级**（评级归归档后的资产库审计侧）。
///
/// 逐列的**基础量走预计算**：基准 bundle 的列摘要（一次查询算出全表各列
/// total / non_null / unique）+ `DESCRIBE` 的列类型喂给
/// `get_column_insight_full_with_basics`——每列免 `COUNT / COUNT(DISTINCT) / typeof`
/// 三查（`COUNT(DISTINCT)` 是全表重聚合，200 列表省 ~199 次单列扫描）。
/// 摘要缺席（那一步失败）时该列回退自算路径，行为同旧。
pub fn request_table_evaluation(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    temp_table: String,
    table_name: String,
    cx: &mut App,
) {
    let weak = view.downgrade();
    cx.spawn(async move |cx| {
        // 先拿列清单：以**磁盘现状**为准（目标带的列可能已过期）
        let bundle = {
            let table = temp_table.clone();
            let task = cx
                .background_executor()
                .spawn(async move { crate::insight_engine::get_table_eda_bundle(&table) });
            task.await
        };
        let bundle = match bundle {
            Ok(bundle) => bundle,
            Err(err) => {
                let info = InsightService::describe_error(&err);
                let _ = weak.update(cx, |panel, cx| {
                    panel.set_error(info.message, info.retryable, cx)
                });
                return;
            }
        };
        let base = crate::service::table_profile_view_of(&bundle, &table_name);

        // 逐列预计算基础量：摘要计数 + DESCRIBE 列类型（缺任一则该列不进表，回退自算）
        let basics_by_name: std::collections::HashMap<String, crate::insight_engine::ColumnBasics> =
            bundle
                .column_summary
                .as_ref()
                .map(|summary| {
                    summary
                        .iter()
                        .filter_map(|brief| {
                            let data_type = bundle
                                .profile
                                .columns
                                .iter()
                                .find(|c| c.column_name == brief.name)
                                .map(|c| c.data_type.clone())?;
                            Some((
                                brief.name.clone(),
                                crate::insight_engine::ColumnBasics {
                                    total: brief.total,
                                    non_null: brief.non_null,
                                    unique: brief.unique,
                                    data_type,
                                },
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();

        let total = base.columns.len();
        // 列检查器聚合（评估完成后随表级事实摘要一起写入；见下方 `with_checker`）
        let mut column_checks: Vec<crate::model::ColumnChecker> = Vec::new();
        let mut done_count = 0usize;
        let mut failed_count = 0usize;
        // 全列失败时用「最后一次错误」给整页错误态（单个失败不影响其它列）
        let mut last_error: Option<shared::error::CoreError> = None;
        let mut current = base.evaluating(0, total);
        let _ = weak.update(cx, |panel, cx| panel.set_table_profile(current.clone(), cx));

        for column in base.columns.iter() {
            // C5 取消令牌：每列派发前检查；置位则停止派发，中断列由下面回填灰态。
            // async 上下文用 update 闭包读面板（AsyncApp 不直接给 &App）
            let cancelled = weak
                .update(cx, |panel, _| {
                    panel.eval_cancel.load(std::sync::atomic::Ordering::Relaxed)
                })
                .unwrap_or(false);
            if cancelled {
                break;
            }
            let table = temp_table.clone();
            let name = column.name.clone();
            let root = project_root.clone();
            let basics = basics_by_name.get(&column.name).cloned();
            let task = cx.background_executor().spawn(async move {
                match &basics {
                    Some(basics) => InsightService::get_column_insight_full_with_basics(
                        root.as_deref(),
                        &table,
                        &name,
                        basics,
                    ),
                    None => InsightService::get_column_insight_full(root.as_deref(), &table, &name),
                }
            });
            match task.await {
                Ok(full) => {
                    column_checks.extend(crate::quality_scorer::compute_column_checks(&full));
                    // 明细回填 + 进度推进：本模块只回填事实统计与检查器，不合成质量总分
                    done_count += 1;
                    current = current
                        .with_column_detail(&column.name, &full)
                        .evaluating(done_count, total);
                }
                Err(err) => {
                    // 单列失败不连坐（架构 §5.5）：失败列标中断（灰态 + 补算入口，C5），
                    // 其余列继续——「哪些列没评上」要确定，而不是一个整页错误态把
                    // 已完成列的结果全部盖掉。全列失败才视为表级失败（临时表没了 /
                    // 连接断了这类对整表成立的错误）。
                    failed_count += 1;
                    tracing::warn!(
                        "评估全表：列 '{}' 失败，已跳过（列卡灰态可补算）：{}",
                        column.name,
                        err
                    );
                    last_error = Some(err);
                    current = current
                        .with_column_interrupted(&column.name)
                        .evaluating(done_count, total);
                }
            }
            // 回填**以面板现值为底**（而不是循环外累积的 `current`）：评估进行期间
            // 面板上可能有并发的其它回填（取样档迷你统计 / 用户动作）——从 `current`
            // 整体覆盖会把它们冲掉（2026-11-05 真机：mini_stats 回填在评估首列回填前
            // 到达，被 base 覆盖 → 10/10 列「样本不足，未生成分布图」）。
            let _ = weak.update(cx, |panel, cx| {
                let base_now = panel.data.table.clone().unwrap_or_else(|| current.clone());
                let merged = current
                    .merge_onto(&base_now)
                    .unwrap_or_else(|| current.clone());
                panel.set_table_profile(merged, cx);
            });
        }

        // 中断（取消令牌置位）：未完成列灰态 + 保留已完成明细与进度；不 finish
        //（quality 保持 None——评估未完成，表级事实摘要不应出现）
        let cancelled = weak
            .update(cx, |panel, _| {
                panel.eval_cancel.load(std::sync::atomic::Ordering::Relaxed)
            })
            .unwrap_or(false);
        if cancelled {
            let interrupted = current.mark_interrupted();
            let _ = weak.update(cx, |panel, cx| {
                let base_now = panel
                    .data
                    .table
                    .clone()
                    .unwrap_or_else(|| interrupted.clone());
                panel.set_table_profile(
                    interrupted.merge_onto(&base_now).unwrap_or(interrupted),
                    cx,
                );
            });
            return;
        }

        // 全列失败 = 表级失败（临时表没了 / 连接断了这类对整表成立的错误）：
        // 没有任何可保留的评估产物，整页错误态给出「为什么」——比一屏灰态补算卡诚实。
        let table_level_error =
            last_error.filter(|_| total > 0 && done_count == 0 && failed_count == total);
        if let Some(err) = table_level_error {
            let info = InsightService::describe_error(&err);
            let _ = weak.update(cx, |panel, cx| {
                panel.set_error(info.message, info.retryable, cx)
            });
            return;
        }

        // 评估收尾：清进度、挂空事实摘要位，再由检查器聚合填充命中 / 适用计数
        let done = current.finish_evaluation().with_checker(column_checks);
        let _ = weak.update(cx, |panel, cx| {
            let base_now = panel.data.table.clone().unwrap_or_else(|| done.clone());
            panel.set_table_profile(done.merge_onto(&base_now).unwrap_or(done), cx);
        });
    })
    .detach();
}

/// 补算单列明细（C5）：中断列灰态卡的「补算」入口。
///
/// 与评估全表同一条 `get_column_insight_full` 链路；单列回填后清除该列中断标记
/// （`with_column_detail` 里置 false）。检查器聚合不重算（表级摘要缺席时无意义；
/// 若表级摘要已存在，补算单列不改变聚合口径）。
pub fn request_column_evaluation(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    temp_table: String,
    column: String,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let query_table = temp_table.clone();
    let query_column = column.clone();
    let task = cx.background_executor().spawn(async move {
        InsightService::get_column_insight_full(
            project_root.as_deref(),
            &query_table,
            &query_column,
        )
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(full) => {
                if let Some(profile) = &panel.data.table {
                    let next = profile.with_column_detail(&column, &full);
                    panel.data.table = Some(next);
                }
                cx.notify();
            }
            Err(err) => {
                let info = InsightService::describe_error(&err);
                panel.set_error(info.message, info.retryable, cx);
            }
        });
    })
    .detach();
}

/// 多列表单：取真实列清单 + `category = multi` 的规则清单。
///
/// 与表探查共用同一次临时表内省（D32）；失败推整页错误态——表单本身就没东西可选。
pub fn request_multi_view(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    temp_table: String,
    table_name: String,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let task = cx.background_executor().spawn(async move {
        InsightService::multi_column_view(project_root.as_deref(), &temp_table, &table_name)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(view) => panel.set_multi_view(view, cx),
            Err(err) => {
                let info = InsightService::describe_error(&err);
                panel.set_error(info.message, info.retryable, cx);
            }
        });
    })
    .detach();
}

/// 跑一条多列规则。
///
/// 失败**不推整页错误态**：表单与已选好的列还在，只把原因挂在结果区
/// （整页转错误会让用户以为选项坏了，而实际只是这一条 SQL 没跑过）。
pub fn request_multi_run(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    temp_table: String,
    rule_id: String,
    columns: Vec<String>,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let task = cx.background_executor().spawn(async move {
        InsightService::run_multi_rule(project_root.as_deref(), &temp_table, &rule_id, &columns)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |panel, cx| match result {
            Ok((result, notes)) => panel.set_multi_result(result, notes, cx),
            Err(err) => panel.set_multi_notice(InsightService::describe_error(&err).message, cx),
        });
    })
    .detach();
}

/// Schema 健康报告：取数 → 视图模型（阻塞段在后台执行器上）。
///
/// 与其他取数不同，这一路要走**源库内省**（不是 DuckDB 临时表），
/// 因此带的是连接 ID + 库 + schema；失败推整页错误态（报告无从部分展示）。
pub fn request_schema_report(
    view: &Entity<InsightView>,
    conn_id: String,
    database: String,
    schema: String,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let task = cx
        .background_executor()
        .spawn(async move { InsightService::schema_report_view(conn_id, &database, &schema) });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(report) => panel.set_schema_report(report, cx),
            Err(err) => {
                let info = InsightService::describe_error(&err);
                panel.set_error(info.message, info.retryable, cx);
            }
        });
    })
    .detach();
}
