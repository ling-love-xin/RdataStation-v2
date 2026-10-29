// 继承父模块（mock_view）的全部 use 绑定
use super::*;

/// 后台任务种类（决定任务干什么、收尾时怎么归置结果）。
///
/// 分两组：**生成类**（`Generate` / `Scenario` / `AppendTo`）自己产出结果；
/// **出口类**（`Persist` / `Export` / `Scratchpad`）消费上一次生成的结果，
/// 因此把 `MockGenInfo` 带在身上——工作线程靠它找到内存临时表、拿到目标表名与列定义，
/// 而视图侧不能把「最近一次结果」另存一份隐式状态（两处状态必然漂移）。
///
/// 出口类**不读草稿**：场景模板产出的多张表与草稿无关，出口只认结果自己（见 [`MockGenInfo`]）。
#[derive(Debug, Clone)]
pub enum MockJobKind {
    /// 只生成到内存临时表（**不写库**）
    Generate,
    /// 生成后追加到既有分析表（自增起点按表内行数接续）
    AppendTo(String),
    /// 场景模板：按工作副本一次生成多张临时表（**不写库**；逐表进度）
    ///
    /// 带的是**编辑过的模板**（关系挂在列的 `dependency` 上），不是模板 id：
    /// 用户改完关系直接生成，再去引擎按 id 重取会丢掉编辑。
    Scenario(Box<ScenarioTemplate>),
    /// 出口：把多张已生成结果**依次**落成分析库新表（每张仍走「新建」语义：同名报错不覆盖）
    PersistAll(Vec<MockGenInfo>),
    /// 出口：把已生成结果持久化为分析库**新表**（同名已存在 → `Err`）
    Persist(MockGenInfo),
    /// 出口：把已生成结果导出为文件（路径由调用方在系统对话框里选好）
    Export {
        /// 已生成结果
        info: MockGenInfo,
        /// 文件格式
        format: MockExportFormat,
        /// 目标路径
        path: String,
    },
    /// 出口：把已生成结果保存到草稿箱（`{项目}/scratchpad/mock/`）
    Scratchpad {
        /// 已生成结果
        info: MockGenInfo,
        /// 文件格式
        format: MockExportFormat,
    },
    /// 出口：把多张已生成结果**依次**存到草稿箱（`{项目}/scratchpad/mock/`；逐张报进度）
    ///
    /// 与 [`Self::PersistAll`] 对称：落库走分析库新表，草稿箱走 CSV 文件。
    /// 批量先固定 CSV（最通用）；后续要支持 Parquet/Xlsx 再挂 dropdown。
    ScratchpadAll {
        /// 已生成结果（多张）
        infos: Vec<MockGenInfo>,
        /// 文件格式
        format: MockExportFormat,
    },
}

impl MockJobKind {
    /// 是否包含**生成阶段**。
    ///
    /// 两个后果：只有生成阶段能取消（引擎按批响应）；只有生成类收尾时要作废旧预览
    /// （它把临时表重建了，而出口类只读不改，临时表与预览仍彼此一致）。
    pub fn generates(&self) -> bool {
        matches!(self, Self::Generate | Self::AppendTo(_) | Self::Scenario(_))
    }

    /// 进度量纲是否按「张表」计（而非批次 / 行）。
    ///
    /// 场景生成与批量落库都是一次处理多张表，`rows_total` 为 0，面板据此把
    /// `batches_done / batches_total` 读作表计数。
    pub fn by_table(&self) -> bool {
        matches!(self, Self::Scenario(_) | Self::PersistAll(_) | Self::ScratchpadAll { .. })
    }

    /// 是否**以当前草稿为输入**（目标表 / 列 / 行数）。
    ///
    /// 只有 `Generate` 与 `AppendTo` 是：场景模板自带表与列（草稿不参与也不被改写），
    /// 三个出口只看已生成的结果（见 [`MockGenInfo`])。
    /// 面板据此决定要不要先校验草稿（表名 / 行数输入 / 至少一列）。
    pub fn uses_draft(&self) -> bool {
        matches!(self, Self::Generate | Self::AppendTo(_))
    }

    /// 任务开始时的阶段（进度条形态与文案据此切换；进行中时以宿主上报的阶段为准）。
    pub fn phase(&self) -> MockJobPhase {
        match self {
            Self::Generate | Self::AppendTo(_) | Self::Scenario(_) => MockJobPhase::Generating,
            Self::Persist(_) | Self::PersistAll(_) => MockJobPhase::Writing,
            Self::Export { .. } | Self::Scratchpad { .. } | Self::ScratchpadAll { .. } => {
                MockJobPhase::Exporting
            }
        }
    }

    /// 本次任务处理的行数（文案用）：生成类按草稿设定，出口类按已生成结果。
    ///
    /// 场景模板的总行数写在模板里（草稿不参与），因此这里给 0——面板对场景任务
    /// 按「张表」报进度（见 `render_job_row`），不拿行数当量纲。
    pub fn rows_total(&self, draft: &MockDraft) -> u32 {
        match self {
            Self::Generate | Self::AppendTo(_) => draft.options.rows,
            Self::Scenario(_) | Self::PersistAll(_) | Self::ScratchpadAll { .. } => 0,
            Self::Persist(info) | Self::Export { info, .. } | Self::Scratchpad { info, .. } => {
                info.row_count
            }
        }
    }

    /// 进行中的一行文案（面板结果区）。
    pub fn running_label(&self) -> String {
        match self {
            Self::Generate => "生成中…".to_string(),
            Self::AppendTo(table) => format!("生成并追加到 {table} 中…"),
            Self::Scenario(template) => format!("按场景模板「{}」生成中…", template.name),
            Self::PersistAll(infos) => format!("落库 {} 张表中…", infos.len()),
            Self::Persist(info) => format!(
                "写入项目分析库中…（{} 行）",
                with_thousands(info.row_count as u64)
            ),
            Self::Export { path, .. } => format!("导出中…（{path}）"),
            Self::Scratchpad { .. } => "保存到草稿箱中…".to_string(),
            Self::ScratchpadAll { infos, .. } => format!("存草稿箱 {} 张表中…", infos.len()),
        }
    }
}

/// 后台任务完成结果。
#[derive(Debug, Clone)]
pub enum MockJobDone {
    /// 生成完成
    Generated(MockGenInfo),
    /// 场景生成完成（多张临时表；顺序即模板里的表序）
    ScenarioGenerated {
        /// 模板名（结果区文案与来源标注）
        template_name: String,
        /// 逐表结果（含预览）
        tables: Vec<MockGenInfo>,
    },
    /// 批量落库完成：成功的表（表名 + 表内行数）与失败的表（表名 + 可读原因）
    ///
    /// 不用「全成功或全失败」：已落的表不回滚（回滚别人的数据比留下已落的多张更危险）。
    PersistedAll {
        /// 落库成功的表
        landed: Vec<(String, i64)>,
        /// 落库失败的表（同名已存在 / 写失败）
        failed: Vec<(String, String)>,
    },
    /// 生成并追加完成（表名 + 表内总行数）
    Appended {
        /// 目标表
        table: String,
        /// 追加后表内总行数
        total_rows: i64,
    },
    /// 持久化完成（新建表名 + 表内行数）
    Persisted {
        /// 新建的表名
        table: String,
        /// 新表行数
        rows: i64,
    },
    /// 导出 / 草稿箱完成（可读文案由装配层给，含落地路径）
    Exported {
        /// 结果文案
        message: String,
    },
    /// 批量存草稿箱完成：成功的表名与失败的表（表名 + 可读原因）
    ///
    /// 与 [`Self::PersistedAll`] 同口径：已存的不回滚，失败的单独列出来。
    ScratchpadAll {
        /// 保存成功的表名
        saved: Vec<String>,
        /// 保存失败的表（表名 + 可读原因）
        failed: Vec<(String, String)>,
    },
}

/// 后台任务阶段。
///
/// 生成阶段有批次粒度回调；写入与落盘跑在 DuckDB / 文件系统内部，
/// 拿不到中间进度，所以进度条改走不定量形态（`Progress::loading`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MockJobPhase {
    /// 生成行数据
    #[default]
    Generating,
    /// 写入分析库
    Writing,
    /// 写出文件（导出 / 草稿箱）
    Exporting,
}

impl MockJobPhase {
    /// 能否给出定量百分比（只有生成阶段能）。
    pub fn is_quantified(self) -> bool {
        matches!(self, Self::Generating)
    }

    /// 阶段文案（进度行 / 详情摘要用）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Generating => "生成中",
            Self::Writing => "写入项目分析库中",
            Self::Exporting => "写出文件中",
        }
    }
}

/// 后台任务进度快照。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MockJobProgress {
    /// 当前阶段
    pub phase: MockJobPhase,
    /// 已完成「量」（单表生成 = 批次；场景模板 = 张表）。
    /// 量纲由任务种类决定（见 `MockJobKind::rows_total`：场景任务为 0 行，按张表报）。
    pub batches_done: usize,
    /// 总量（单表生成 = 总批次；场景模板 = 总表数）。首次回调前为 0，表示「尚未开始」
    pub batches_total: usize,
    /// 本次任务的行数（文案用；场景模板为 0，因为行数写在模板里、草稿不参与）
    pub rows_total: u32,
}

impl MockJobProgress {
    /// 完成百分比（0.0~100.0；非生成阶段或总批次未知时为 0）。
    pub fn percent(&self) -> f32 {
        if self.batches_total == 0 {
            return 0.0;
        }
        (self.batches_done as f32 / self.batches_total as f32 * 100.0).clamp(0.0, 100.0)
    }

    /// 已完成行数（批次粒度估算：向下取整到批）。
    ///
    /// `rows_total` 为 0 时恒为 0——场景模板任务就是这种（量纲是「张表」，不是行），
    /// 面板对这类任务改用 `batches_done / batches_total` 报「{done} / {total} 张表」。
    pub fn rows_done(&self) -> u32 {
        if self.batches_total == 0 {
            return 0;
        }
        let per_batch = (self.rows_total as f64 / self.batches_total as f64).ceil() as u32;
        (self.batches_done as u32 * per_batch).min(self.rows_total)
    }
}

/// 后台任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockJobState {
    /// 无任务
    Idle,
    /// 进行中（可取消）
    Running(MockJobProgress),
}
