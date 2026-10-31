//! 洞察接缝 · 列画像请求（自 jobs.rs 拆分，纯位移）

use super::*;
/// 一次画像请求：后台执行所需的全部输入（均为所有权数据，因此可跨线程）。
///
/// 只派生 `PartialEq` 不派生 `Eq`：探查配置的 `Percent(f64)` 没有 `Eq`（f64 语义）。
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileRequest {
    Column {
        temp_table: String,
        column: String,
    },
    Table {
        temp_table: String,
        table_name: String,
    },
    /// 源目标：先按探查配置取样成分析临时表，再走与临时表目标同一条路
    SourceColumn {
        source: crate::model::SampleSource,
        column: String,
        /// 探查配置（确认窗口产物）：数据量形态 / 采样方式 / 过滤 / 种子
        config: crate::model::ProbeConfig,
    },
    SourceTable {
        source: crate::model::SampleSource,
        table_name: String,
        /// 探查配置（确认窗口产物）：数据量形态 / 采样方式 / 过滤 / 种子
        config: crate::model::ProbeConfig,
    },
}

impl ProfileRequest {
    /// 源目标的取样来源（临时表目标为 `None`）。
    pub fn source(&self) -> Option<&crate::model::SampleSource> {
        match self {
            ProfileRequest::SourceColumn { source, .. }
            | ProfileRequest::SourceTable { source, .. } => Some(source),
            _ => None,
        }
    }

    /// 探查配置（临时表目标为 `None`——临时表已是既定数据，配置不适用）。
    pub fn config(&self) -> Option<&crate::model::ProbeConfig> {
        match self {
            ProfileRequest::SourceColumn { config, .. }
            | ProfileRequest::SourceTable { config, .. } => Some(config),
            _ => None,
        }
    }

    /// 从面板目标解析出请求；**当前不支持的形态返回 `None`**。
    ///
    /// 列画像 / 表画像都走这里；多列（`MultiColumnRequested`）与结构
    /// （`SchemaReportRequested`）不经此路径，由各自事件分支处理。
    pub fn of(target: &InsightTarget) -> Option<Self> {
        Self::of_scope(target, crate::model::ProbeConfig::sample_default())
    }

    /// 带探查配置解析（确认窗口产物，C3）：`None` = 默认取样。
    pub fn of_with_config(
        target: &InsightTarget,
        config: Option<crate::model::ProbeConfig>,
    ) -> Option<Self> {
        Self::of_scope(target, config.unwrap_or_default())
    }

    /// 全量口径的请求（源目标手动切换为全量时用；临时表目标仍是取样语义）。
    pub fn of_full(target: &InsightTarget) -> Option<Self> {
        Self::of_scope(target, crate::model::ProbeConfig::full())
    }

    fn of_scope(target: &InsightTarget, config: crate::model::ProbeConfig) -> Option<Self> {
        match target {
            InsightTarget::Column {
                temp_table, column, ..
            } => Some(Self::Column {
                temp_table: temp_table.clone(),
                column: column.clone(),
            }),
            InsightTarget::Table {
                temp_table,
                table_name,
            } => Some(Self::Table {
                temp_table: temp_table.clone(),
                table_name: table_name.clone(),
            }),
            InsightTarget::SourceColumn { source, column, .. } => Some(Self::SourceColumn {
                source: source.clone(),
                column: column.clone(),
                config,
            }),
            InsightTarget::SourceTable { source, table_name } => Some(Self::SourceTable {
                source: source.clone(),
                table_name: table_name.clone(),
                config,
            }),
            InsightTarget::MultiColumn { .. } | InsightTarget::Schema { .. } => None,
        }
    }
}

/// 提交一次画像请求：后台执行 → 回填面板。
///
/// 回填只走面板的公开方法（`set_profile` / `set_table_profile` / `set_error`），
/// 错误经 `describe_error` 转成「给人看的文案 + 是否可重试」——面板因此永远不接触 `CoreError`。
pub fn request_profile(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    request: ProfileRequest,
    cx: &mut App,
) {
    let weak = view.downgrade();
    // 源目标（要取样）：拆成「取样 → 画像」两段，中间把加载说明推进一档。
    //
    // 取样是慢的那一段：源库查询 + DuckDB 建分析临时表（大表 CTAS 可能较久，
    // 且 CTAS 在库内完成、拿不到真百分比）——所以分阶段给文字说明 + 不确定进度条，
    // 而不是让用户对着干骨架以为卡住了。
    if let Some(source) = request.source() {
        let source = source.clone();
        // 探查配置先取出（`request` 还要留给画像阶段用，不能 move 进取样闭包）
        let config = request
            .config()
            .expect("源目标必有探查配置（ProfileRequest::config）")
            .clone();
        let _ = weak.update(cx, |panel, cx| {
            panel.set_loading_note(
                Some("正在取样并建分析临时表（大表可能需要一些时间）…".into()),
                cx,
            );
        });
        let sample_task = cx.background_executor().spawn(async move {
            // gpui 的 background_executor 不是 Tokio：驱动层 `tokio::time`（PG 探活）在
            // 非 Tokio 上下文会 panic（`there is no reactor running`）。包一层进程级单例
            // runtime 的 block_on，把整条取样 async 链放回 Tokio 上下文再 await。
            crate::service::block_on(
                crate::service::persistence::sample_source_to_analysis_table(&source, &config),
            )
        });
        cx.spawn(async move |cx| {
            let temp_table = match sample_task.await {
                Ok(table) => table,
                Err(err) => {
                    let info = InsightService::describe_error(&err);
                    let _ = weak.update(cx, |panel, cx| {
                        panel.set_error(info.message, info.retryable, cx)
                    });
                    return;
                }
            };
            // 取样完成，进入画像阶段
            let _ = weak.update(cx, |panel, cx| {
                panel.set_loading_note(Some("正在计算画像…".into()), cx);
            });
            let profile_task = cx.background_executor().spawn(async move {
                let root = project_root.as_deref();
                match request {
                    ProfileRequest::SourceColumn { column, .. } => {
                        InsightService::profile_column_view(root, &temp_table, &column)
                            .map(|view| ProfileOutcome::SourceColumn { temp_table, view })
                    }
                    ProfileRequest::SourceTable { table_name, .. } => {
                        InsightService::profile_table_view(root, &temp_table, &table_name)
                            .map(|view| ProfileOutcome::SourceTable { temp_table, view })
                    }
                    _ => unreachable!(),
                }
            });
            let result = profile_task.await;
            let _ = weak.update(cx, |panel, cx| match result {
                Ok(ProfileOutcome::SourceColumn { temp_table, view }) => {
                    panel.set_source_sample(temp_table, cx);
                    panel.set_profile(view, cx);
                }
                Ok(ProfileOutcome::SourceTable { temp_table, view }) => {
                    panel.set_source_sample(temp_table, cx);
                    panel.set_table_profile(view, cx);
                }
                Err(err) => {
                    let info = InsightService::describe_error(&err);
                    panel.set_error(info.message, info.retryable, cx);
                }
                _ => {}
            });
        })
        .detach();
        return;
    }

    let task = cx.background_executor().spawn(async move {
        let root = project_root.as_deref();
        match request {
            ProfileRequest::Column { temp_table, column } => {
                InsightService::profile_column_view(root, &temp_table, &column)
                    .map(ProfileOutcome::Column)
            }
            ProfileRequest::Table {
                temp_table,
                table_name,
            } => InsightService::profile_table_view(root, &temp_table, &table_name)
                .map(ProfileOutcome::Table),
            ProfileRequest::SourceColumn { .. } | ProfileRequest::SourceTable { .. } => {
                unreachable!("源目标已在上方两段式分支处理")
            }
        }
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        // 面板可能已关闭：弱句柄升级失败就丢弃结果（不 panic）
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(ProfileOutcome::Column(profile)) => panel.set_profile(profile, cx),
            Ok(ProfileOutcome::Table(profile)) => panel.set_table_profile(profile, cx),
            Ok(ProfileOutcome::SourceColumn { temp_table, view }) => {
                panel.set_source_sample(temp_table, cx);
                panel.set_profile(view, cx);
            }
            Ok(ProfileOutcome::SourceTable { temp_table, view }) => {
                panel.set_source_sample(temp_table, cx);
                panel.set_table_profile(view, cx);
            }
            Err(err) => {
                let info = InsightService::describe_error(&err);
                panel.set_error(info.message, info.retryable, cx);
            }
        });
    })
    .detach();
}

/// 取数产物（两种目标各自回填到对应的 setter）
enum ProfileOutcome {
    Column(ColumnProfileView),
    Table(TableProfileView),
    /// 源目标的列画像：带上取样得到的临时表（面板后续保存 / 下钻 / 多列都用它）
    SourceColumn {
        temp_table: String,
        view: ColumnProfileView,
    },
    SourceTable {
        temp_table: String,
        view: TableProfileView,
    },
}

/// 探查确认窗口的列候选内省（C1）：后台轻量拿源查询列清单，回填面板。
///
/// 失败不阻塞确认——面板保持「全部列」默认；也不打扰用户（没有可展示的错误）。
pub fn request_probe_columns(
    view: &Entity<InsightView>,
    source: crate::model::SampleSource,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let task = cx.background_executor().spawn(async move {
        crate::service::block_on(crate::service::persistence::probe_source_columns(&source))
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(columns) => panel.set_probe_columns(columns, cx),
            Err(err) => {
                tracing::debug!("探查确认窗口列候选内省失败（默认全部列）: {}", err);
            }
        });
    })
    .detach();
}

/// 取样档迷你统计（Q2 / C7）：对**当前评估全表**做查询级过滤现算，回填列卡迷你图。
///
/// 列清单（名称 + 类型族）在接缝后台现读——与评估同口径，不持过期列名；
/// 现算失败不打断画像（迷你图缺席即可，保持 `None` 不编假值）。
pub fn request_sample_mini_stats(
    view: &Entity<InsightView>,
    temp_table: String,
    cx: &mut App,
) {
    let columns: Vec<(String, crate::model::ColumnKind)> = view
        .read(cx)
        .data()
        .table
        .as_ref()
        .map(|profile| {
            profile
                .columns
                .iter()
                .map(|c| (c.name.clone(), c.kind))
                .collect()
        })
        .unwrap_or_default();
    if columns.is_empty() {
        return;
    }
    let weak = view.downgrade();
    let task = cx.background_executor().spawn(async move {
        // 迷你统计是 DuckDB 同步查询（无 Tokio 链），直接同步调用即可
        crate::insight_engine::compute_sample_mini_stats(&temp_table, &columns)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(stats) => panel.set_sample_mini_stats(stats, cx),
            Err(err) => {
                tracing::debug!("取样档迷你统计现算失败（迷你图缺席）: {}", err);
            }
        });
    })
    .detach();
}