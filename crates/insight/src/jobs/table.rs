//! 洞察接缝 · 表评估请求（自 jobs.rs 拆分，纯位移）

use super::*;

/// 「评估全表」：**逐列串行**回填事实明细并跑检查器，每列回来就回填一次（真实进度，不是转动图标）。
///
/// 串行是刻意选择：并发会撞上引擎的并发上限（D12），而失败重试反而让用户更困惑；
/// 一列一列跑既不会超限，又能把进度如实给出来。为了让进度真的动起来，每列单独 dispatch
/// 到后台执行器（而不是一整个循环全在后台），算完一列就回头更新一次面板。
/// 本模块只回填事实统计与检查器命中，**不合成质量总分 / 等级**（评级归归档后的资产库审计侧）。
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
        let base = {
            let table = temp_table.clone();
            let name = table_name.clone();
            let root = project_root.clone();
            let task = cx.background_executor().spawn(async move {
                InsightService::profile_table_view(root.as_deref(), &table, &name)
            });
            task.await
        };
        let base = match base {
            Ok(profile) => profile,
            Err(err) => {
                let info = InsightService::describe_error(&err);
                let _ = weak.update(cx, |panel, cx| {
                    panel.set_error(info.message, info.retryable, cx)
                });
                return;
            }
        };

        let total = base.columns.len();
        // 列检查器聚合（评估完成后随表级事实摘要一起写入；见下方 `with_checker`）
        let mut column_checks: Vec<crate::model::ColumnChecker> = Vec::new();
        let mut done_count = 0usize;
        let mut current = base.evaluating(0, total);
        let _ = weak.update(cx, |panel, cx| panel.set_table_profile(current.clone(), cx));

        for column in base.columns.iter() {
            // C5 取消令牌：每列派发前检查；置位则停止派发，中断列由下面回填灰态。
            // async 上下文用 update 闭包读面板（AsyncApp 不直接给 &App）
            let cancelled = weak
                .update(cx, |panel, _| {
                    panel
                        .eval_cancel
                        .load(std::sync::atomic::Ordering::Relaxed)
                })
                .unwrap_or(false);
            if cancelled {
                break;
            }
            let table = temp_table.clone();
            let name = column.name.clone();
            let root = project_root.clone();
            let task = cx.background_executor().spawn(async move {
                InsightService::get_column_insight_full(root.as_deref(), &table, &name)
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
                    // 单列失败不报废整表：直接报错（用户的下一步是重试或修正数据）
                    let info = InsightService::describe_error(&err);
                    let _ = weak.update(cx, |panel, cx| {
                        panel.set_error(info.message, info.retryable, cx)
                    });
                    return;
                }
            }
            let _ = weak.update(cx, |panel, cx| panel.set_table_profile(current.clone(), cx));
        }

        // 中断（取消令牌置位）：未完成列灰态 + 保留已完成明细与进度；不 finish
        //（quality 保持 None——评估未完成，表级事实摘要不应出现）
        let cancelled = weak
            .update(cx, |panel, _| {
                panel
                    .eval_cancel
                    .load(std::sync::atomic::Ordering::Relaxed)
            })
            .unwrap_or(false);
        if cancelled {
            let interrupted = current.mark_interrupted();
            let _ = weak.update(cx, |panel, cx| panel.set_table_profile(interrupted, cx));
            return;
        }

        // 评估收尾：清进度、挂空事实摘要位，再由检查器聚合填充命中 / 适用计数
        let done = current.finish_evaluation().with_checker(column_checks);
        let _ = weak.update(cx, |panel, cx| panel.set_table_profile(done, cx));
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