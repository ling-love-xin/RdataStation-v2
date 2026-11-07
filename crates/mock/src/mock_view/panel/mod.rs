// ==================== 配置面板（右 Dock） ====================

// 继承父模块（mock_view）的全部 use 绑定
use super::*;

// 任务计时（ETA）：墙钟只在视图侧读，见 `MockJobWatch::started_at`
use std::time::{Duration, Instant};

// 渲染子模块（impl MockPanel 的渲染方法与 Render trait）
mod render;
// 列操作子模块（impl MockPanel 的列增删改 / 生成器选择方法）
mod columns;
// 任务/Job 子模块（impl MockPanel 的后台任务启动 / 轮询 / 收尾方法）
mod jobs;
// 场景/关系子模块（impl MockPanel 的场景模板 / 关系配置 / 表格编辑方法）
mod scenario;
// 渲染行的共用表单助手（detail.rs / tests 也调用，经本层再导出到 mock_view）
pub(crate) use render::{complex_form_line, form_line};

/// 一份表配置的三只输入框（懒创建：`InputState` 要 window）。
///
/// 按配置 id 分开存：两个配置 tab 并存时，输入态必须各自同源到自己那份配置，
/// 否则「切到另一张 tab，表名框里还是上一张的名字」。
pub(crate) struct ConfigInputs {
    pub(crate) table: Entity<InputState>,
    pub(crate) rows: Entity<InputState>,
    pub(crate) seed: Entity<InputState>,
    /// 待写入表名的值（事件路径置位，下一帧渲染时落地）
    pending_table: Option<String>,
    /// 输入框写回订阅（必须持有，丢了就退订）
    _subs: Vec<Subscription>,
}

/// Mock 配置面板（右 Dock 17.5rem）：目标 + 选项 + 列来源 + 生成 + 出口。
pub struct MockPanel {
    host: Rc<dyn MockHost>,
    /// **活动**表配置：同一时刻面板只编辑它（字段区 / 生成 / 出口都读它）
    pub(crate) draft: MockTableConfig,
    /// 活动配置的 id（与表名序号同源，也是 `DetailTarget::Config(id)` 的键）
    pub(crate) config_id: u64,
    /// 其余表配置：点「＋」时把上一份收进来，切换配置时再换出去。
    /// 面板只**编辑**活动那份，但每个 tab 都按自己的 id 读（`config_for`），
    /// 所以未激活 tab 渲染出来的仍是它自己那份。
    pub(crate) configs: std::collections::HashMap<u64, MockTableConfig>,
    /// 列 id 计数器（列可增删，键用 id）
    pub(crate) next_id: u64,
    /// 最近一次运行的**结果表**（单表生成 = 1 条；场景模板 = N 条）
    pub(crate) results: Vec<MockGenInfo>,
    /// 预览的**重查取样**（排序 / 多要几行；每张表最多一份，见 [`PreviewCache`]）
    pub(crate) preview_cache: Option<PreviewCache>,
    /// 预览要显示多少行（面板上的「行数」档；生成时只取 [`PREVIEW_ROWS`] 行，要更多得重查）
    preview_limit: usize,
    /// 中央详情 tab 三段分栏里**预览区**的初始高度（rem；拖拽后由 `on_resize` 回写，
    /// 跨 tab / 跨窗口沿用）
    pub(crate) detail_preview_rem: f32,
    /// 列编辑对话框第三行**左栏宽度**（px；拖拽后由 `on_resize` 回写，重开对话框沿用）。
    /// 仍受 220–520 的拖拽范围约束，越界值在渲染时 clamp。
    pub(crate) column_list_px: f32,
    /// 字段区的列搜索框，**每份配置一只**（各自筛各自的列；懒创建，搜索词存在框里）
    column_filters: HashMap<u64, Entity<InputState>>,
    /// 各配置搜索词变化时重画字段区的订阅（必须持有：丢了就自动退订）
    column_filter_subs: Vec<Subscription>,
    /// 当前选中的结果表下标（出口作用于它；越界视为 0）
    current: usize,
    /// 当前结果来自哪套场景模板（`None` = 单表生成；面板据此给一句来源说明）
    scenario_source: Option<String>,
    /// 最近一次成功落库的表名（出口反馈用）
    landed: Option<String>,
    /// 本会话里已落库的表（项目库里确实有了；切项目时清空——那是另一个库）
    landed_tables: Vec<String>,
    pub(crate) outcome: Option<String>,
    pub(crate) error: Option<String>,
    /// 可导入结构的连接（事件路径加载）
    sources: Vec<SchemaSource>,
    /// 分析库既有表（追加候选，事件路径加载）
    existing_tables: Vec<String>,
    /// 内置场景模板清单（构造时算一次；见 [`ScenarioChoice`]）
    scenario_templates: Vec<ScenarioChoice>,
    /// 场景工作副本：选模板后进入**可编辑**态（改完关系再生成）；`None` = 单表态。
    ///
    /// 关系就在模板各列的 `dependency` 上（单一权威），这里不另存一份关系清单。
    pub(crate) scenario: Option<ScenarioTemplate>,
    /// 最近一次**场景生成**用的关系快照（结果还在，关系就还得说得出——
    /// 用户可能已经退出场景态，也可能改过工作副本）。
    last_relations: Vec<ScenarioRelation>,
    /// 「加关系」对话框里正在选的四个位置（都是名字；`None` = 未选）。
    pub(crate) relation_pick: Rc<RefCell<RelationPick>>,
    /// 「追加确认」对话框的状态（`None` = 没开）。
    append_pick: Rc<RefCell<Option<AppendPick>>>,
    /// 「编辑表」对话框正在改哪张表（`None` = 没开）
    pub(crate) table_editing: Option<String>,
    /// 「编辑表」对话框的输入：表名 / 行数
    pub(crate) edit_name_input: Option<Entity<InputState>>,
    pub(crate) edit_rows_input: Option<Entity<InputState>>,
    /// 每一份表配置各有一组输入（表名 / 行数 / 种子）。
    ///
    /// 为什么按配置 id 分开而不是共用一组：两个配置 tab 并存时，共用一组输入就会出现
    /// 「切到另一张 tab，输入框里还是上一张的表名」——输入态与配置态在这里是两份数据，
    /// 必须同源到同一份配置。
    pub(crate) config_inputs: std::collections::HashMap<u64, ConfigInputs>,
    /// 场景生成用的种子输入（`None` = 场景态未开）；值与 `self.scenario.seed` 互相同步
    scenario_seed_input: Option<Entity<InputState>>,
    /// 待写入场景种子输入的值（`load_scenario` 置位，下一帧 `ensure_inputs` 落地）
    scenario_seed_pending: Option<String>,
    /// 清单搜索框（懒创建：`InputState` 要 window；搜索词存在它里，不另存一份）
    search_input: Option<Entity<InputState>>,
    /// 工具栏「＋」的表名序号（每次点击递增：mock_table_1 → mock_table_2 → …）
    draft_seq: u32,
    /// 导入结构对话框：选中的连接（`None` = 未选）
    import_conn: Rc<RefCell<Option<String>>>,
    import_catalog: Option<Entity<InputState>>,
    import_schema: Option<Entity<InputState>>,
    import_table: Option<Entity<InputState>>,
    /// 进行中的后台任务（`None` = 空闲；生成与追加共用）
    job: Option<MockJobWatch>,
    /// 当前任务**提交时**的项目根（出口类任务的写入点也是那一刻快照的）。
    ///
    /// 用途：任务收尾时对比当前项目——切项目后回来的结果要能说清「写进的是上一个项目」，
    /// 而不是让用户在新项目里找一张不存在的表。
    job_project: Option<PathBuf>,
    /// 生成历史（最近 [`history::HISTORY_LIMIT`] 条，时间倒序）
    pub(crate) history: Vec<MockGenerationTask>,
    /// 用户模板（与历史同一次后台读拿回，见 `HistorySnapshot`）
    pub(crate) templates: Vec<MockUserTemplate>,
    /// 历史与模板是否读到过（区分「还没读」与「读完了是空的」）
    pub(crate) history_loaded: bool,
    /// 历史与模板读写在途（后台）
    pub(crate) history_loading: bool,
    /// 历史与模板读写失败的原因（成功一次就清掉）
    pub(crate) history_error: Option<String>,
    /// 「保存为模板」对话框的名称输入框
    template_name: Option<Entity<InputState>>,
    /// 本会话点过的生成器（目录名，最近在前；生成器菜单的「最近使用」，见 B7）
    recent_generators: Vec<(String, bool)>,
    /// 本会话的自定义生成器（分组「＋」精调层创建；SQLite 持久化记录在册，见文档 §9.5）
    pub(crate) user_generators: Vec<UserGenerator>,
    /// 底部折叠段（生成历史 / 用户模板）是否展开（默认收起：面板天天用的是上面那份清单）
    pub(crate) fold_open: bool,
    /// 上次清空结果是不是切项目造成的（旧 tab 的文案据此区分「另一个项目」与「这一轮」）
    project_switched: bool,
    /// 鼠标正指着场景清单里的哪一张表（行内操作是悬停才显形的，要重绘就得有状态）。
    ///
    /// 键盘那边不记：焦点由 `FocusHandle::is_focused` 当场查，再存一份就要维护
    /// focus / blur 回调，还容易与 gpui 的焦点系统对不上。
    hovered_scenario_table: Option<String>,
    /// 鼠标正指着历史里的哪一条（同上）
    hovered_history: Option<String>,
    /// 鼠标正指着用户模板里的哪一个（同上）
    hovered_template: Option<String>,
}

/// 进行中任务的视图侧状态（进度镜像 + 轮询泵句柄 + 计时）。
struct MockJobWatch {
    /// 任务种类（收尾时据此分派结果）
    kind: MockJobKind,
    /// 最近一次拉到的进度
    progress: MockJobProgress,
    /// 已请求取消（按钮转「正在取消…」）
    cancel_requested: bool,
    /// 任务提交时刻（ETA 的分母）。
    ///
    /// **记在视图侧而不是让宿主上报**：宿主那边的时间基准（轮询周期 / 线程调度）与
    /// 视图侧看到的不一定同步，两处时间相减会算出负数。这里只信自己的墙钟。
    started_at: Instant,
    /// 平滑后的「每单位（批 / 张表）耗时」（毫秒）；`None` = 第一个单位还没完成
    unit_ms: Option<f64>,
    /// 上一次前进（或刚起步）的时刻：算出「已经卡了多久」
    last_sample_at: Instant,
    /// 上次唤醒 UI 用的 ETA 秒数（变了才唤醒，不然每 120ms 都要重排一次面板）
    eta_shown: Option<u64>,
    /// 轮询泵（持句柄即存活；任务结束或面板销毁时自动停）
    _pump: Task<()>,
}

impl MockJobWatch {
    /// 剩余时间：均速推算 + 已经卡住的时间。
    ///
    /// 只算均速那一项会让卡住的任务一直报它卡住前的那句（「剩 12 秒」卡了 30 秒还在说 12 秒），
    /// 而 ETA 的意义正是「还要等多久」——卡住也要算进去。
    fn eta(&self) -> Option<Duration> {
        let base = eta_from_unit_ms(
            self.unit_ms,
            self.progress.batches_done,
            self.progress.batches_total,
        )?;
        Some(base + self.last_sample_at.elapsed())
    }
}

/// 交给后台执行器的一件事（读与写共用一条路径）。
#[derive(Clone)]
enum HistoryTask {
    /// 读快照（历史 + 模板）
    List,
    /// 删一条历史后读回
    Delete(String),
    /// 记一次生成运行后读回
    Record {
        draft: MockTableConfig,
        run: RunRecord,
    },
    /// 把表配置存成模板后读回
    SaveTemplate {
        name: String,
        draft: MockTableConfig,
    },
    /// 删一个模板后读回
    DeleteTemplate(String),
    /// 取一条历史的完整配置（重放）
    Replay(String),
    /// 取一个模板的完整配置（应用）
    ApplyTemplate(String),
}

/// 后台读的回填形态（与 [`HistoryTask`] 一一对应）。
pub(crate) enum HistoryReply {
    /// 历史与模板已刷新
    Listed(history::HistorySnapshot),
    /// 重放：拿回当时的完整配置（列表不动）
    Replayed(Box<MockGenerationDetail>),
    /// 应用模板：拿回模板与它的列（列表不动）
    TemplateApplied(Box<(MockUserTemplate, Vec<MockTemplateColumn>)>),
}

/// 内置场景模板的条目（面板菜单用）：只留名字与规模，模板本体在生成时按 id 重取。
///
/// 缓存的原因：[`crate::templates::get_builtin_templates`] 会现场构造整套列定义
/// （数百个 `ColumnDef`），每帧重建不划算——「渲染期零 I/O」同样包含「零重活」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioChoice {
    /// 模板 id（提交任务用）
    pub id: String,
    /// 模板名（菜单与来源文案用）
    pub name: String,
    /// 模板里的表数
    pub table_count: usize,
    /// 模板里的总行数（菜单预览用）
    pub total_rows: u32,
}

impl ScenarioChoice {
    /// 菜单条目文案：`零售电商（5 张表 · 12,000 行）`。
    pub fn menu_label(&self) -> String {
        format!(
            "{}（{} 张表 · {} 行）",
            self.name,
            self.table_count,
            with_thousands(self.total_rows as u64)
        )
    }
}

/// 场景态里的一条表间关系（渲染与测试用的**派生视图**：真身在列的 `dependency` 上）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioRelation {
    /// 子表（持有外键列的表）
    pub child_table: String,
    /// 子列（外键列）
    pub child_column: String,
    /// 父表（被引用的表）
    pub parent_table: String,
    /// 父列（被引用的列，目前只支持自增主键）
    pub parent_column: String,
}

impl ScenarioRelation {
    /// 文案：`orders.user_id → users.id`。
    pub fn label(&self) -> String {
        format!(
            "{}.{} → {}.{}",
            self.child_table, self.child_column, self.parent_table, self.parent_column
        )
    }
}

/// 进度行画在哪一处（D38 的「状态单点」）：单表任务在中央那张表的表头，
/// 集合任务（场景 N 张表 / 批量落库）在右 Dock 的场景清单下——同一时刻只有一处。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobRowScope {
    /// 单表任务（生成 / 追加 / 三个出口）
    Single,
    /// 集合任务（场景模板 / 批量落库）
    Collection,
}

/// 清单行的状态点：三种落库状态 + 三种生成状态（两者不是一回事——生成的是临时表，
/// 落库的才是项目库里的）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableStatus {
    /// 已落库（本会话落过）
    Persisted,
    /// 有结果但还没落库
    NotPersisted,
    /// 它引用的父表还没落库（孩子那条路会落空）
    DanglingParent,
    /// 生成中（带百分比）
    Generating,
    /// 生成失败
    Failed,
    /// 还没生成
    Idle,
}

/// 从模板里扫出全部表间关系（真身在列的 `dependency` 上，这里是派生）。
fn relations_of(template: &ScenarioTemplate) -> Vec<ScenarioRelation> {
    template
        .tables
        .iter()
        .flat_map(|table| {
            table.columns.iter().filter_map(move |col| {
                let dep = col.dependency.as_ref()?;
                Some(ScenarioRelation {
                    child_table: table.name.clone(),
                    child_column: col.name.clone(),
                    parent_table: dep.ref_table.clone(),
                    parent_column: dep.ref_column.clone(),
                })
            })
        })
        .collect()
}

/// 「加关系」对话框里正在选的四个位置（都是名字；`None` = 未选）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RelationPick {
    pub(crate) child_table: Option<String>,
    pub(crate) child_column: Option<String>,
    pub(crate) parent_table: Option<String>,
    pub(crate) parent_column: Option<String>,
}

/// 「追加确认」对话框正在等的确认（S3）：选中的目标表 + 它的现状快照。
///
/// `None` = 没开对话框。状态在**打开时**读一次（对话框期间库可能被改，
/// 但「确认前看一眼现状」的用途不受影响）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AppendPick {
    /// 目标表名
    table: String,
    /// 目标表现状（行数 / 隐含主键 / 最大值）；读不到时为 `None`（只显示表名）
    status: Option<AppendTargetStatus>,
}

/// 内置场景模板的轻量清单（构造面板时取一次）。
pub(crate) fn builtin_scenario_choices() -> Vec<ScenarioChoice> {
    crate::templates::get_builtin_templates()
        .into_iter()
        .map(|template| ScenarioChoice {
            table_count: template.tables.len(),
            total_rows: template.tables.iter().map(|t| t.row_count).sum(),
            id: template.id,
            name: template.name,
        })
        .collect()
}

impl MockPanel {
    /// 创建面板（宿主在 `cx.new` 中调用；构造不做 I/O）。
    pub fn new(host: Rc<dyn MockHost>, _cx: &mut Context<Self>) -> Self {
        Self {
            host,
            draft: MockTableConfig::default(),
            config_id: 0,
            configs: std::collections::HashMap::new(),
            next_id: 1,
            results: Vec::new(),
            preview_cache: None,
            preview_limit: PREVIEW_ROWS,
            detail_preview_rem: 20.0,
            column_list_px: f32::from(ui::COLUMN_GENERATOR_LIST_WIDTH),
            column_filters: HashMap::new(),
            column_filter_subs: Vec::new(),
            current: 0,
            scenario_source: None,
            landed: None,
            landed_tables: Vec::new(),
            outcome: None,
            error: None,
            sources: Vec::new(),
            existing_tables: Vec::new(),
            scenario_templates: builtin_scenario_choices(),
            scenario: None,
            last_relations: Vec::new(),
            relation_pick: Rc::new(RefCell::new(RelationPick::default())),
            append_pick: Rc::new(RefCell::new(None)),
            table_editing: None,
            edit_name_input: None,
            edit_rows_input: None,
            config_inputs: std::collections::HashMap::new(),
            scenario_seed_input: None,
            scenario_seed_pending: None,
            search_input: None,
            draft_seq: 0,
            import_conn: Rc::new(RefCell::new(None)),
            import_catalog: None,
            import_schema: None,
            import_table: None,
            job: None,
            job_project: None,
            history: Vec::new(),
            templates: Vec::new(),
            history_loaded: false,
            history_loading: false,
            history_error: None,
            template_name: None,
            recent_generators: Vec::new(),
            user_generators: Vec::new(),
            fold_open: false,
            project_switched: false,
            hovered_scenario_table: None,
            hovered_history: None,
            hovered_template: None,
        }
    }

    // ==================== 只读访问器（宿主 / 详情 tab / 测试） ====================

    /// 当前表配置
    pub fn draft(&self) -> &MockTableConfig {
        &self.draft
    }

    /// 活动配置的 id（中央 tab 身份 `DetailTarget::Config(id)` 用它）。
    pub fn config_id(&self) -> u64 {
        self.config_id
    }

    /// 按 id 读某份配置：活动那份就是 `draft`，其余在 `configs`。
    /// 找不到（tab 指向的配置已不存在）时退回活动那份，渲染不出错也不 panic。
    pub(crate) fn config_for(&self, id: u64) -> &MockTableConfig {
        if id == self.config_id {
            &self.draft
        } else {
            self.configs.get(&id).unwrap_or(&self.draft)
        }
    }

    /// 按 id 取可写引用：活动那份直接给 `draft`，其余从 `configs` 取（缺失则补一份空配置）。
    pub(crate) fn config_mut(&mut self, id: u64) -> &mut MockTableConfig {
        if id == self.config_id {
            &mut self.draft
        } else {
            self.configs
                .entry(id)
                .or_insert_with(MockTableConfig::default)
        }
    }

    /// 面板里**全部**表配置的轻量快照（活动那份 + `configs`），按 id 升序、id 去重。
    ///
    /// 右面板清单只画这一层：多配置时每份占一行，点一行就切到它自己的 tab。
    /// 只画活动那一份时，开过第二个「＋」之后就再也回不到前一份了。
    pub(crate) fn config_rows(&self) -> Vec<(u64, String, usize, u64)> {
        let mut rows: Vec<(u64, String, usize, u64)> = self
            .configs
            .iter()
            .map(|(id, config)| {
                (
                    *id,
                    config.table_name.clone(),
                    config.columns.len(),
                    config.options.rows as u64,
                )
            })
            .collect();
        rows.push((
            self.config_id,
            self.draft.table_name.clone(),
            self.draft.columns.len(),
            self.draft.options.rows as u64,
        ));
        rows.sort_by_key(|(id, ..)| *id);
        rows.dedup_by_key(|(id, ..)| *id);
        rows
    }

    /// 把某份配置换成**活动**配置（旧活动收进 `configs`，内容都不丢）。
    ///
    /// 调用点：① Dock 真正激活某张配置 tab（`MockDetailView::set_active`，切 tab 即切配置）；
    /// ② 「写」路径的动作（点按钮 / 打字回写 / 生成 / 出口）——这些只发生在当前可见的那张 tab 上。
    /// **渲染期不调**：未激活 tab 也会渲染，在 render 里切换会互相抢。
    pub(crate) fn focus_config(&mut self, id: u64) {
        if id == self.config_id {
            return;
        }
        let Some(target) = self.configs.remove(&id) else {
            return;
        };
        let previous = std::mem::replace(&mut self.draft, target);
        self.configs.insert(self.config_id, previous);
        self.config_id = id;
        // 结果与提示是上一次生成留下的现场，换配置后不再适用
        self.outcome = None;
        self.error = None;
    }

    /// 事件路径置位「下一帧把表名框写成这个值」（编辑表 / 导入结构改名用）。
    ///
    /// 输入框还没建时置位会丢，这没关系：`ensure_inputs_for` 建的时候直接取配置里的表名。
    fn set_table_pending(&mut self, value: String) {
        if let Some(inputs) = self.config_inputs.get_mut(&self.config_id) {
            inputs.pending_table = Some(value);
        }
    }

    /// 表名输入框（活动配置的那只；测试与渲染用）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn table_input(&self) -> Option<&Entity<InputState>> {
        self.config_inputs.get(&self.config_id).map(|i| &i.table)
    }

    /// 行数输入框（同上）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn rows_input(&self) -> Option<&Entity<InputState>> {
        self.config_inputs.get(&self.config_id).map(|i| &i.rows)
    }

    /// 最近一次生成结果
    pub fn gen_info(&self) -> Option<&MockGenInfo> {
        self.current_info()
    }

    /// 当前结果表（`current` 越界时回退到第一条；空结果返回 `None`）。
    fn current_info(&self) -> Option<&MockGenInfo> {
        self.results
            .get(self.current.min(self.results.len().saturating_sub(1)))
            .filter(|_| !self.results.is_empty())
    }

    /// 结果表清单（场景生成后不止一张；出口作用于当前选中的那张）。
    pub fn results(&self) -> &[MockGenInfo] {
        &self.results
    }

    /// 当前选中的结果表下标。
    pub fn current_result(&self) -> usize {
        self.current
    }

    /// 当前结果来自哪套场景模板（`None` = 单表生成）。
    pub fn scenario_source(&self) -> Option<&str> {
        self.scenario_source.as_deref()
    }

    /// 可用的内置场景模板（面板「场景模板 ▾」菜单；构造时算好，渲染期零重活）。
    pub fn scenario_templates(&self) -> &[ScenarioChoice] {
        &self.scenario_templates
    }

    /// 最近一次成功文案
    pub fn outcome(&self) -> Option<&str> {
        self.outcome.as_deref()
    }

    /// 最近一次失败原因
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// 最近一次成功落库的表名
    pub fn landed(&self) -> Option<&str> {
        self.landed.as_deref()
    }

    /// 可导入结构的连接
    pub fn sources(&self) -> &[SchemaSource] {
        &self.sources
    }

    /// 分析库既有表
    pub fn existing_tables(&self) -> &[String] {
        &self.existing_tables
    }

    /// 是否正在跑后台任务（生成 / 追加 / 出口）
    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    /// 进行中的任务是否**含生成阶段**（决定「生成中…」文案与取消按钮）
    pub fn is_generating(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.kind.generates())
    }

    /// 进行中任务的种类（无任务时为 `None`）；面板与测试据此区分量纲（批次 / 张表）。
    pub fn job_kind(&self) -> Option<&MockJobKind> {
        self.job.as_ref().map(|job| &job.kind)
    }

    /// 进行中任务的进度（无任务时为 `None`）
    pub fn job_progress(&self) -> Option<MockJobProgress> {
        self.job.as_ref().map(|job| job.progress)
    }

    /// 进行中任务的百分比（无任务 / 进度不可量化时为 0）。
    ///
    /// 清单行的状态点与中央表头共用同一份口径（两处都写 `percent()` 会在精度上漂移）。
    fn job_percent(&self) -> f32 {
        self.job
            .as_ref()
            .map(|job| job.progress.percent())
            .unwrap_or(0.0)
    }

    /// 进行中任务的剩余时间（ETA）；样本不足 / 不足 1 秒时为 `None`。
    ///
    /// 视图侧只做**读数**：采样在 `poll_job` 里做（那才有「每单位耗时」的样本），
    /// 渲染期不碰时钟，也不让渲染结果依赖渲染那一刻的时间。
    pub fn job_eta(&self) -> Option<Duration> {
        self.job.as_ref()?.eta()
    }

    /// 任务快照（状态栏读它：文案 / 百分比 / ETA / 能不能取消一次拿全）。
    pub fn job_status(&self) -> Option<MockJobStatus> {
        let job = self.job.as_ref()?;
        Some(MockJobStatus {
            progress: job.progress,
            eta: self.job_eta(),
            cancellable: job.kind.generates(),
            cancel_requested: job.cancel_requested,
        })
    }

    /// 清单行的进度形态（右 Dock 表名那一行下缘那条线的数据来源）。
    ///
    /// `index` = 这一行在**集合**里的位置（集合任务按张推进；单表任务不看它）。
    /// 判定本身是纯函数 [`row_progress_of`]，这里只负责把「是不是任务目标」算出来。
    pub fn row_progress(&self, table: &str, index: usize) -> Option<RowProgress> {
        let job = self.job.as_ref()?;
        let is_target = self
            .results
            .first()
            .map(|info| info.table_name.as_str())
            .unwrap_or(self.draft.table_name.as_str())
            == table;
        row_progress_of(job.kind.by_table(), job.progress, index, is_target)
    }

    /// 已请求取消（按钮文案转「正在取消…」）
    pub fn cancel_requested(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.cancel_requested)
    }

    // ==================== 事件路径入口 ====================

    /// 重新加载「可导入结构的连接」与「分析库既有表」（打开面板 / 切换项目 / 下拉刷新）。
    pub fn refresh_sources(&mut self, cx: &mut Context<Self>) {
        self.sources = self.host.schema_sources();
        self.existing_tables = self.host.existing_tables();
        cx.notify();
    }

    /// 只重读「导入结构」的连接候选（打开导入对话框前 / 连接清册变化时）。
    /// 与 [`MockPanel::refresh_sources`] 分开的原因：连接候选从内存里的连接清册派生，
    /// 一次遍历就够，可以跟着连接的新建 / 编辑随时刷；而「既有分析表」那一份要开
    /// 项目分析库文件（真 I/O），只留给面板打开与切项目那一拍。
    pub fn refresh_connection_sources(&mut self, cx: &mut Context<Self>) {
        self.sources = self.host.schema_sources();
        cx.notify();
    }

    /// 字段区的列搜索框（懒创建；`None` = 这张 tab 还没渲染过）。
    ///
    /// `pub` 而不是 `pub(crate)`：面板的只读访问器一律公开（宿主 / 详情 tab / 测试都从这儿读，
    /// 与 `scenario_templates()` / `job_kind()` 同口径），不为了少一个 `pub` 让测试去摸私有字段。
    pub fn column_filter_input(&self) -> Option<Entity<InputState>> {
        self.column_filters.get(&self.config_id).cloned()
    }

    /// 某份配置的列搜索词（空 = 不筛）。搜索词就存在输入框实体里，不另存一份。
    pub(crate) fn column_filter_query(&self, id: u64, cx: &App) -> String {
        self.column_filters
            .get(&id)
            .map(|input| input.read(cx).value().trim().to_string())
            .unwrap_or_default()
    }

    /// 字段区表头行里的列搜索框（靠右，固定宽）。
    ///
    /// 搜索框空着也不隐藏：一个偶尔冒出来的控件比一个一直在那儿的小输入框更让人意外。
    /// 无列时传 `enabled = false`：**留着位置只降不透明度**，不加第一列时工具栏不会凭空多出一个框。
    pub(crate) fn render_column_filter(
        &mut self,
        id: u64,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.ensure_inputs_for(id, window, cx);
        let theme = cx.theme().clone();
        match self.column_filters.get(&id).cloned() {
            // 边框自己画：组件默认的 `input` 描边在暗色下几乎看不见，直接看不出这里能输入。
            // 换成 `background` 内凹底 + `border` 描边 + 圆角，与右侧 xsmall 按钮同高（24px）。
            Some(input) => div()
                .flex_none()
                .w(rems(ui::COLUMN_FILTER_WIDTH))
                .h_6()
                .h_flex()
                .items_center()
                .px_2()
                .rounded(theme.radius)
                .bg(theme.colors.background)
                .border_1()
                .border_color(theme.colors.border)
                .opacity(if enabled { 1.0 } else { 0.45 })
                .child(
                    Input::new(&input)
                        .appearance(false)
                        .bordered(false)
                        .xsmall(),
                ),
            None => div().flex_none().w(rems(ui::COLUMN_FILTER_WIDTH)),
        }
    }

    // ==================== 预览的按列重排（表头 / 右键菜单） ====================

    /// 这一表当前该显示的取样，与生效中的排序（`None` = 按生成顺序）。
    ///
    /// 缓存只对**派生它的那份取样**与**当时的行数档**有效：结果被换掉（重新生成 / 切项目）、
    /// 或行数档改了就自动失效（见 [`PreviewCache`]），不需要谁记得来清。
    pub fn preview_for(&self, table: &str) -> Option<(&MockPreview, Option<&PreviewSort>)> {
        let info = self.results.iter().find(|info| info.table_name == table)?;
        let cache = self
            .preview_cache
            .as_ref()
            .filter(|cache| cache.applies_to(table, &info.preview, self.preview_limit));
        Some((
            cache.map_or(&info.preview, |cache| &cache.preview),
            cache.and_then(|cache| cache.sort.as_ref()),
        ))
    }

    /// 预览要显示多少行（面板上的「行数」档）。
    pub fn preview_limit(&self) -> usize {
        self.preview_limit
    }

    /// 改预览行数档（面板菜单）：超出生成的取样（[`PREVIEW_ROWS`]）就要重查。
    ///
    /// 与 `sort_preview` 同一处缓存、同一套失效规则：有排序就带着排序重查，没有就只多要几行；
    /// 档位回到 [`PREVIEW_ROWS`] 且没有排序时直接丢掉缓存（生成那份取样就够，不必查库）。
    pub fn set_preview_limit(&mut self, limit: usize, cx: &mut Context<Self>) {
        if limit == self.preview_limit || limit == 0 {
            return;
        }
        let previous = std::mem::replace(&mut self.preview_limit, limit);
        // 带着当前排序重查（缓存里有排序就沿用；没有就是「多要几行」）
        let order = self
            .preview_cache
            .as_ref()
            .filter(|cache| cache.limit == previous)
            .and_then(|cache| cache.sort.clone());
        self.refresh_preview_sample(order, cx);
    }

    /// 表头箭头 / 右键菜单：把预览取样换成**按该列排序后的前 N 行**；
    /// `descending = None` = 取消排序（回到生成顺序；行数档大于生成取样时仍然重查）。
    ///
    /// 只有面板能走这三条出口：重查是 I/O（要走宿主端口）、而重查要的临时表名在结果清单里。
    /// 失败与「内存库正忙」都只写一句可读原因，**不动**已有的取样——预览是只读观察窗，
    /// 点一下箭头不应该把已经在看的数据弄没。
    pub fn sort_preview(
        &mut self,
        table: &str,
        column: &str,
        descending: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let order = descending.map(|descending| PreviewSort {
            column: column.to_string(),
            descending,
        });
        self.refresh_preview_sample_for(table, order, cx);
    }

    /// 重查取样（当前表 = 「当前结果表」，排序可空）。行数档回到生成取样且无排序时退化成「丢缓存」。
    fn refresh_preview_sample(&mut self, order: Option<PreviewSort>, cx: &mut Context<Self>) {
        let table = self.current_info().map(|info| info.table_name.clone());
        let Some(table) = table else {
            return;
        };
        self.refresh_preview_sample_for(&table, order, cx);
    }

    /// 重查取样（指定表；排序可空）。
    ///
    /// 无事可做的情形（只清缓存、不查库）：目标行数 ≤ 生成取样且有排序要取消 / 未变。
    fn refresh_preview_sample_for(
        &mut self,
        table: &str,
        order: Option<PreviewSort>,
        cx: &mut Context<Self>,
    ) {
        let limit = self.preview_limit;
        let Some(info) = self.results.iter().find(|info| info.table_name == table) else {
            return;
        };
        // 不需要查库：不排序且只要生成那几行——直接丢缓存回落到生成取样
        if order.is_none() && limit <= PREVIEW_ROWS {
            let changed = self.preview_cache.is_some();
            self.preview_cache = None;
            if changed {
                cx.notify();
            }
            return;
        }
        // 借租约不能跨到宿主调用（`preview_sample` 是同步 I/O）：先把要用的取出来
        let temp_table = info.temp_table_name.clone();
        let base = info.preview.clone();
        let hint = order
            .as_ref()
            .map(|order| order.label())
            .unwrap_or_else(|| "取样".to_string());
        let queried = self.host.preview_sample(
            &temp_table,
            order.as_ref().map(|o| (o.column.as_str(), o.descending)),
            limit,
        );
        match queried {
            Ok(Some(preview)) => {
                self.preview_cache = Some(PreviewCache {
                    table: table.to_string(),
                    base,
                    limit,
                    sort: order,
                    preview,
                });
                // 成功就把上一次的失败说明收掉（同一拍里换，不留陈旧错误）
                self.error = None;
            }
            // 内存库被别的任务占着：原因说清楚，取样保持原样
            Ok(None) => {
                self.error = Some(format!(
                    "内存库正被别的任务占用（生成 / 出口进行中）：{hint}稍后再试"
                ));
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    /// 项目已切换：作废与旧项目绑定的生成结果与出口反馈（`cleared` = 宿主刚清掉的临时表数）。
    ///
    /// 为什么必须作废：宿主在切项目时删掉了本进程的 mock 临时表（内存库是进程级单例，
    /// 不切项目就会一直在），`gen_info` 里的临时表名已经指向不存在的表——留着它，
    /// 用户下一次点「导出 / 落库」只会拿到一个难懂的「表不存在」。
    ///
    /// 表配置（目标表名 / 列 / 行数种子）**保留**：那是用户的配置，跨项目可以继续用。
    pub fn forget_generated(&mut self, cleared: usize, cx: &mut Context<Self>) {
        self.results.clear();
        self.current = 0;
        // 排序是按临时表重查来的：临时表都没了，那份取样也就没意义了
        // 排序 / 重查取样是按临时表重查来的：临时表都没了，那份取样也就没意义了
        // （行数档是用户的偏好，跨项目留着）
        self.preview_cache = None;
        self.scenario_source = None;
        self.last_relations.clear();
        self.landed = None;
        // 另一个项目 = 另一个分析库：上一项目的落库记录、「追加到既有表」候选在这里都不成立
        self.landed_tables.clear();
        self.existing_tables.clear();
        // 连接清册也随作用域变（全局 + 该项目的 `P_` / `GP_`）：宿主在切项目后会重读，
        // 真没重读也不打紧——导入对话框在清册为空时会自己拉一次
        self.sources.clear();
        self.error = None;
        // 旧表 tab 不自动关（D35：关不关 tab 是用户的窗口布局）：它们得能说清自己为什么没结果，
        // 而「这一轮没有它的结果」在切项目后是假话（不是这一轮的事，是另一个项目的事）。
        self.project_switched = true;
        // 没清到东西就不打扰用户（切项目很常见，每次都报一句是噪声）
        self.outcome = (cleared > 0).then(|| {
            format!("已切换项目：清掉 {cleared} 张 mock 临时表，生成结果已作废（请重新生成）")
        });
        cx.notify();
    }

    /// 上一次清空结果是不是切项目造成的（旧 tab 的文案据此区分「另一个项目」与「这一轮」）。
    ///
    /// 什么时候落下：`forget_generated`（只由切项目触发）置位；下一次生成成功后清掉。
    pub fn results_dropped_by_project_switch(&self) -> bool {
        self.project_switched
    }

    /// 打开中央「Mock 数据」详情 tab（表配置 tab：字段清单 + 预览）。
    pub fn open_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_detail_for(DetailTarget::Config(self.config_id), window, cx);
    }

    /// 打开（或聚焦）某个详情 tab：表配置，或某张结果表。
    pub fn open_detail_for(
        &mut self,
        target: DetailTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.host.open_detail(target, window, cx);
    }

    /// 打开某张**结果表**的详情 tab（右 Dock 结果区点一行 / 「查看详情」走这条）。
    pub fn open_table_detail(&mut self, table: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_detail_for(DetailTarget::Table(table.to_string()), window, cx);
    }

    /// 「查看详情」看谁：有结果就看当前表（tab 就是那张表），没结果看表配置。
    pub fn current_detail_target(&self) -> DetailTarget {
        self.results
            .get(self.current)
            .map(|info| DetailTarget::Table(info.table_name.clone()))
            .unwrap_or(DetailTarget::Config(self.config_id))
    }

    /// 中央某个 tab 被激活：把它对应的表设为「当前表」（出口作用于它）。
    ///
    /// 表不在本轮结果里（tab 是上一轮留下的）就什么都不做：`current` 不该指向不存在的表。
    pub fn focus_table(&mut self, table: &str, cx: &mut Context<Self>) {
        if let Some(index) = self
            .results
            .iter()
            .position(|info| info.table_name == table)
        {
            if index != self.current {
                self.current = index;
                cx.notify();
            }
        }
    }

    // ==================== 生成历史（后台读写；渲染只读状态） ====================

    /// 重读生成历史（打开面板 / 切换项目时调用）。
    ///
    /// 只投一次后台读：开项目库是文件 I/O，事件路径上做会冻 UI，渲染期更不行。
    pub fn refresh_history(&mut self, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::List, cx);
    }

    /// 删一条历史（删除与重读在同一件后台动作里完成）。
    pub fn delete_history(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::Delete(task_id), cx);
    }

    /// 重放一条历史：把当时的表名 / 行数 / 种子 / 语言与列配置写回表配置。
    ///
    /// **不自动生成**：重放是「把配置摆回来」，跑不跑由用户定（同模板应用口径）。
    pub fn replay_history(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::Replay(task_id), cx);
    }

    /// 把当前表配置存成用户模板（名字由保存对话框给，空名在这里拦住）。
    ///
    /// 两道门都落在动作本身而不只在对话框上：对话框只是入口之一，
    /// 校验在这里才能保证「不管谁调都不会存出一份套不出东西的模板」。
    pub fn save_template(&mut self, name: String, cx: &mut Context<Self>) {
        if self.draft.columns.is_empty() {
            self.history_error =
                Some("先加列（或导入结构）再存模板：空配置存下来没有意义".to_string());
            cx.notify();
            return;
        }
        if name.trim().is_empty() {
            self.history_error = Some("模板名不能为空".to_string());
            cx.notify();
            return;
        }
        let draft = self.draft.clone();
        self.spawn_history(HistoryTask::SaveTemplate { name, draft }, cx);
    }

    /// 应用一个用户模板：把它存的行数 / 种子 / 语言与列写回表配置（**不动目标表名**）。
    pub fn apply_template(&mut self, template_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::ApplyTemplate(template_id), cx);
    }

    /// 删一个用户模板（删除与重读在同一件后台动作里完成）。
    pub fn delete_template(&mut self, template_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::DeleteTemplate(template_id), cx);
    }

    /// 把面板要做的一件事交给后台执行器，完成后经弱句柄回填。
    ///
    /// 未打开项目时不去跑后台：历史没有落点，直接给一句可读的原因
    /// （而不是一个看起来像「本项目没有记录」的空列表）。
    fn spawn_history(&mut self, task: HistoryTask, cx: &mut Context<Self>) {
        let Some(root) = self.host.project_root() else {
            // 未打开项目：历史没有落点，给一句可读的原因（而不是看起来像「本项目没有记录」）。
            // 只有「重读列表」才清空：删记录 / 重放失败时，已显示的列表比一片空白有用。
            if matches!(task, HistoryTask::List) {
                self.history.clear();
                self.templates.clear();
                self.history_loaded = true;
            }
            self.history_loading = false;
            self.history_error = Some("未打开项目：生成历史与模板随项目保存".to_string());
            cx.notify();
            return;
        };
        self.history_loading = true;
        self.history_error = None;
        let work = cx.background_executor().spawn(async move {
            // 项目库的打开与读写要 tokio 运行时（GPUI 后台线程不带）：`history::drive` 自己进一个
            match task {
                HistoryTask::List => history::drive(history::list(&root, history::HISTORY_LIMIT))
                    .map(HistoryReply::Listed),
                HistoryTask::Delete(id) => history::drive(history::run(
                    &root,
                    HistoryAction::DeleteTask(id),
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::Record { draft, run } => history::drive(history::run(
                    &root,
                    HistoryAction::Record { draft, run },
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::SaveTemplate { name, draft } => history::drive(history::run(
                    &root,
                    HistoryAction::SaveTemplate { name, draft },
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::DeleteTemplate(id) => history::drive(history::run(
                    &root,
                    HistoryAction::DeleteTemplate(id),
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::ApplyTemplate(id) => {
                    history::drive(history::template_detail(&root, &id))
                        .map(|detail| HistoryReply::TemplateApplied(Box::new(detail)))
                }
                HistoryTask::Replay(id) => history::drive(history::detail(&root, &id))
                    .map(|detail| HistoryReply::Replayed(Box::new(detail))),
            }
        });
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let reply = work.await;
            // 面板可能已关闭：弱句柄升级失败就丢弃结果（不 panic）
            let _ = weak.update(cx, |panel, cx| panel.accept_history(reply, cx));
        })
        .detach();
        cx.notify();
    }

    /// 回填后台结果（`pub(crate)`：窗口测试直接喂它，不等真实后台）。
    pub(crate) fn accept_history(
        &mut self,
        reply: Result<HistoryReply, String>,
        cx: &mut Context<Self>,
    ) {
        self.history_loading = false;
        match reply {
            Ok(HistoryReply::Listed(snapshot)) => {
                self.history = snapshot.tasks;
                self.templates = snapshot.templates;
                self.history_loaded = true;
                self.history_error = None;
            }
            Ok(HistoryReply::Replayed(detail)) => {
                self.apply_replay(*detail, cx);
                self.history_loaded = true;
                self.history_error = None;
            }
            Ok(HistoryReply::TemplateApplied(detail)) => {
                let (template, columns) = *detail;
                self.apply_template_config(&template, &columns);
                self.history_loaded = true;
                self.history_error = None;
            }
            // 失败不改列表：能看到的旧列表比一片空白有用
            Err(reason) => self.history_error = Some(reason),
        }
        cx.notify();
    }

    /// 应用模板落地：行数 / 种子 / 语言与列换成模板里那一套，**目标表名不动**
    /// （纯状态动作，测试直接调）。
    pub(crate) fn apply_template_config(
        &mut self,
        template: &MockUserTemplate,
        columns: &[MockTemplateColumn],
    ) {
        let mut draft = history::draft_of_template(template, columns);
        // 模板不存表名：用户正在写的目标表名保住（换表名是另一件事）
        draft.table_name = self.draft.table_name.clone();
        let rows = draft.options.rows;
        let count = draft.columns.len();
        self.next_id = count as u64 + 1;
        self.draft = draft;
        // 旧结果作废：表配置已是另一套配置，临时表还是上一套的
        self.results.clear();
        self.current = 0;
        self.landed = None;
        self.error = None;
        self.outcome = Some(format!(
            "已应用模板 {}（{} 行 · {count} 列；种子与语言一并写入）",
            template.name,
            with_thousands(rows as u64)
        ));
    }

    /// 重放落地：表配置整体换成历史里的那一套（纯状态动作，测试直接调）。
    ///
    /// S5：场景历史（`scene_id` 有值）不落表配置——多表配置存不进单表列行，
    /// 重放按模板 id 重新取内置模板并套用当时的种子（编辑过的关系不随历史恢复）。
    pub(crate) fn apply_replay(&mut self, detail: MockGenerationDetail, cx: &mut Context<Self>) {
        if let Some(scene_id) = detail.task.scene_id.clone() {
            let seed = detail.task.seed.map(|seed| seed as u32);
            let Some(mut template) = crate::templates::get_template_by_id(&scene_id) else {
                self.fail(
                    format!("场景模板不存在：{scene_id}（可能已从内置清单移除）"),
                    cx,
                );
                return;
            };
            template.seed = seed;
            let name = template.name.clone();
            self.load_scenario(template, cx);
            self.outcome = Some(format!(
                "已重放场景 {name}（seed {}）：确认后再点生成",
                seed.map(|s| s.to_string())
                    .unwrap_or_else(|| "随机".to_string())
            ));
            return;
        }
        let draft = history::draft_of_detail(&detail);
        let table = draft.table_name.clone();
        let columns = draft.columns.len();
        self.next_id = draft
            .columns
            .iter()
            .map(|column| column.id)
            .max()
            .unwrap_or(0)
            + 1;
        self.draft = draft;
        self.set_table_pending(self.draft.table_name.clone());
        // 旧结果作废：表配置已是另一套配置，临时表还是上一套的
        self.results.clear();
        self.current = 0;
        self.landed = None;
        self.error = None;
        self.outcome = Some(format!(
            "已重放 {table} 的配置（{columns} 列）：确认后再点生成"
        ));
    }

    /// 导航右键「生成 Mock 数据」：按**源库表**预填目标表名并导入其结构。
    ///
    /// 这正是 v1 的主路径（源库结构 → 造新数据），不再依赖「分析库已有同名表」。
    pub fn preset_from_source(&mut self, request: SchemaRequest, cx: &mut Context<Self>) {
        self.draft.table_name = request.table.clone();
        self.set_table_pending(request.table.clone());
        match self.host.import_columns(&request) {
            Ok(columns) => {
                self.draft.columns = columns;
                self.next_id = self.draft.columns.iter().map(|c| c.id).max().unwrap_or(0) + 1;
                self.results.clear();
                self.current = 0;
                self.landed = None;
                self.error = None;
                self.outcome = Some(format!(
                    "已从 {}.{}.{} 导入 {} 列",
                    request.catalog,
                    request.schema,
                    request.table,
                    self.draft.columns.len()
                ));
            }
            Err(e) => {
                self.outcome = None;
                self.landed = None;
                self.error = Some(e);
            }
        }
        cx.notify();
    }

    /// 生成到内存临时表（**后台任务**：立即返回，进度与结果由轮询回填）。
    ///
    /// 不在事件路径里写库：真正落库是出口的事（不变式 I5）。
    pub fn run_generate(&mut self, cx: &mut Context<Self>) {
        let table = self.draft().table_name.clone();
        let rows = self.draft().options.rows;
        tracing::info!(
            table = %table,
            rows = rows,
            "【探针】Mock「生成」on_click 触发（生成到内存临时表）"
        );
        self.start_job(MockJobKind::Generate, cx);
    }

    /// 场景模板：把模板**载入为可编辑的工作副本**（不立即生成）。
    ///
    /// 为何不选即生成：用户要能先看到「本次要生成哪几张表」、并把表间关系调好，

    /// 切换当前选中的结果表（场景生成后不止一张）。
    pub fn select_result(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.results.len() && index != self.current {
            self.current = index;
            cx.notify();
        }
    }

    /// S3：追加前先看目标表现状——打开确认对话框（行数 / 隐含主键 / 最大值），
    /// 确认后再追加。
    ///
    /// 状态在**打开时**读一次：`append_target_status` 是宿主查分析库的只读操作，
    /// 对话框期间库被改的概率与影响都小，「确认前看一眼现状」的用途不受影响。
    pub fn open_append_dialog(
        &mut self,
        table: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        // 隐含主键 = 表配置里 AutoIncrement 生成器的列（v2 不建主键 DDL，自增列就是主键）
        let pk_hint = self
            .draft
            .columns
            .iter()
            .find(|c| matches!(c.def.generator, GeneratorConfig::AutoIncrement { .. }))
            .map(|c| c.def.name.clone());
        let status = match &pk_hint {
            Some(pk) => self.host.append_target_status(&table, Some(pk)),
            None => self.host.append_target_status(&table, None),
        };
        *self.append_pick.borrow_mut() = Some(AppendPick {
            table: table.clone(),
            status,
        });

        let pick = self.append_pick.clone();
        let panel = cx.entity();
        let table_clone = table.clone();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let fg = theme.colors.foreground;
            let current = pick.borrow().clone().unwrap_or(AppendPick {
                table: table_clone.clone(),
                status: None,
            });

            let mut body = div().v_flex().gap_2();
            // 目标表现状：行数 / 隐含主键 / 最大值
            match &current.status {
                Some(status) => {
                    let pk_text = match (&status.pk_column, status.max_pk) {
                        (Some(pk), Some(max)) => format!("{pk} 最大 {max}"),
                        (Some(pk), None) => format!("{pk}（空表）"),
                        (None, _) => "（未识别到自增主键列）".to_string(),
                    };
                    body = body.child(kv(
                        theme,
                        "目标表现状",
                        div()
                            // 值用主色：这是这一行的信息主体，不是标签的注解
                            .text_xs()
                            .text_color(fg)
                            .child(format!(
                                "{} 行 · {pk_text}",
                                with_thousands(status.row_count.max(0) as u64)
                            )),
                    ));
                    // 自增起点预告：本次生成的自增列会从 MAX + 1 接续
                    if let (Some(pk), Some(max)) = (&status.pk_column, status.max_pk) {
                        body = body.child(info_line(
                            theme,
                            format!("{pk} 从 {} 开始接续（MAX + 1）", max + 1),
                        ));
                    }
                }
                None => {
                    body = body.child(warning_line(
                        theme,
                        "读不到目标表现状：将按现有行数接续自增，唯一冲突会整批拒绝。",
                    ));
                }
            }

            let cancel_panel = panel.clone();
            let ok_panel = panel.clone();
            let table_for_ok = current.table.clone();
            dialog
                .title(format!("追加到 {}", current.table))
                .child(body)
                .footer(
                    DialogFooter::new()
                        .child(
                            button("mock-append-cancel", "取消", ActionKind::Soft).on_click(
                                move |_, window, app| {
                                    let _ = cancel_panel.update(app, |panel, _cx| {
                                        *panel.append_pick.borrow_mut() = None;
                                    });
                                    window.close_dialog(app);
                                },
                            ),
                        )
                        .child(
                            button("mock-append-ok", "确认追加", ActionKind::Main).on_click(
                                move |_, window, app| {
                                    let _ = ok_panel.update(app, |panel, cx| {
                                        panel.append_table(table_for_ok.clone(), cx);
                                    });
                                    window.close_dialog(app);
                                },
                            ),
                        ),
                )
        });
    }

    /// 出口：追加到**项目**分析库既有表（显式选择；后台任务：生成 + 追加在一次任务里完成）。
    pub fn append_table(&mut self, table: String, cx: &mut Context<Self>) {
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        *self.append_pick.borrow_mut() = None;
        self.start_job(MockJobKind::AppendTo(table), cx);
    }


    /// 出口：持久化为**项目**分析库新表（后台任务：大行数落库同样会阻塞界面）。
    pub fn persist_table(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.current_info().cloned() else {
            self.fail("请先生成（预览确认后再落库）", cx);
            return;
        };
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        // 目标表名在 `start_job` 的 `sync_inputs` 里从输入框取（任务内部据此命名新表）
        self.start_job(MockJobKind::Persist(info), cx);
    }

    /// 出口：导出文件（调用方已选好路径；后台任务）。
    pub fn export_file(&mut self, format: &MockExportFormat, path: String, cx: &mut Context<Self>) {
        let Some(info) = self.current_info().cloned() else {
            self.fail("请先生成（预览确认后再导出）", cx);
            return;
        };
        if self.host.read_only() {
            self.fail("只读模式：不允许写出文件", cx);
            return;
        }
        self.start_job(
            MockJobKind::Export {
                info,
                format: format.clone(),
                path,
            },
            cx,
        );
    }

    /// 出口：保存到草稿箱（`{项目}/mock/`；后台任务）。
    pub fn save_scratchpad(&mut self, format: &MockExportFormat, cx: &mut Context<Self>) {
        let Some(info) = self.current_info().cloned() else {
            self.fail("请先生成（预览确认后再保存）", cx);
            return;
        };
        if self.host.read_only() {
            self.fail("只读模式：不允许写出文件", cx);
            return;
        }
        self.start_job(
            MockJobKind::Scratchpad {
                info,
                format: format.clone(),
            },
            cx,
        );
    }


    /// 从输入框同步目标表名 / 行数 / 种子（生成与出口前调用）。
    fn sync_inputs(&mut self, cx: &App) -> Result<(), String> {
        let id = self.config_id;
        let inputs = self.config_inputs.get(&id);
        let name = inputs.map(|inputs| inputs.table.read(cx).value().to_string());
        let rows = inputs.map(|inputs| inputs.rows.read(cx).value().to_string());
        let seed = inputs.map(|inputs| inputs.seed.read(cx).value().to_string());
        match name {
            Some(value) => self.draft.table_name = validate_table_name(&value)?,
            None => self.draft.table_name = validate_table_name(&self.draft.table_name)?,
        }
        if let Some(value) = rows {
            self.draft.options.rows = parse_rows(&value)?;
        }
        if let Some(value) = seed {
            self.draft.options.seed = parse_seed(&value)?;
        }
        Ok(())
    }

    fn succeed(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = None;
        self.outcome = Some(message);
        self.host.notify(cx);
        cx.notify();
    }

    /// 项目级出口（落库 / 追加 / 草稿箱）能不能用：**有结果 + 非只读 + 已打开项目**。
    ///
    /// 三个条件就是三个出口的可行性判据，写在一处：视图据此禁用按钮（渲染期只读它），
    /// 禁用理由也在同一处（`render_actions` 的两行状态提示）。
    /// 为何不靠「点了再报错」：三个出口都是项目级的，未打开项目时按钮永远点得动、
    /// 每次都弹同一句拒绝，不如直接不可点（与「生成前禁用」同一口径）。
    pub fn sinks_ready(&self) -> bool {
        self.has_result() && !self.host.read_only() && self.host.project_root().is_some()
    }

    fn fail(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.outcome = None;
        self.error = Some(message.into());
        cx.notify();
    }

    /// 生成结果是否就绪（出口按钮是否可用）
    fn has_result(&self) -> bool {
        !self.results.is_empty()
    }

    /// 「另存为」的默认文件名：取**当前结果表**的名字（场景模板下与表配置无关）。
    ///
    /// 没结果时回退到表配置的目标表名：按钮在无结果时本来就点不开（`has_result` 拦着），
    /// 这里只保证函数本身不会给出空名字。
    pub(crate) fn export_file_name(&self, format: &MockExportFormat) -> String {
        let table = self
            .current_info()
            .map(|info| info.table_name.clone())
            .unwrap_or_else(|| self.draft.table_name.clone());
        mock_file_name(&table, format)
    }

    /// 测试用：直接写行数输入框（校验失败路径：非法行数止于视图，不触宿主）。
    #[cfg(test)]
    pub(crate) fn set_rows_input(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.rows_input().cloned() {
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
    }

    // ==================== 渲染：配置面板 ====================

    /// 目标表名 + 行数 / 种子 / 语言。
    /// 进行中任务的进度归属（`None` = 空闲）。
    pub fn job_row_scope(&self) -> Option<JobRowScope> {
        let job = self.job.as_ref()?;
        Some(if job.kind.by_table() {
            JobRowScope::Collection
        } else {
            JobRowScope::Single
        })
    }

    /// 这张表引用的**出边关系**（清单里挂在它那一行下面的 `↳` 子行）。
    ///
    /// 真身在列上的 `dependency`；这里用生成时的关系快照（`last_relations`）——
    /// 用户可能已经退出场景态，也可能改过工作副本。
    pub fn outgoing_relations(&self, table: &str) -> Vec<ScenarioRelation> {
        self.last_relations
            .iter()
            .filter(|relation| relation.child_table == table)
            .cloned()
            .collect()
    }

    /// 清单行的状态点（生成状态优先，其次是落库状态）：
    /// 生成的是临时的，落库的才是项目库里的——两者不是一回事。
    pub fn table_status(&self, table: &str) -> TableStatus {
        if let Some(job) = self.job.as_ref() {
            // 只有生成类才叫「生成中」：出口类（写入 / 导出）只动已有结果，不改生成状态
            if !job.kind.by_table() && job.progress.phase.is_quantified() {
                let target = self
                    .results
                    .first()
                    .map(|info| info.table_name.as_str())
                    .unwrap_or(self.draft.table_name.as_str());
                if target == table {
                    return TableStatus::Generating;
                }
            }
        }
        if self.results.iter().any(|info| info.table_name == table) {
            // 悬空引用优先于「落了没」：它指的是可行动的事实（父表还没落，值会悬空），
            // 而「已落库」只说明本表自己落了——落没落都不影响那句⚠还成不成立
            if self.table_references_unlanded(table) {
                return TableStatus::DanglingParent;
            }
            if self.landed_tables.iter().any(|landed| landed == table) {
                return TableStatus::Persisted;
            }
            return TableStatus::NotPersisted;
        }
        if self.error.is_some() && self.draft.table_name == table {
            return TableStatus::Failed;
        }
        TableStatus::Idle
    }

    /// 这张表引用的父表里，还有没落库的吗（`⚠ 引用的表未落库` 的数据来源）。
    ///
    /// 只看**本会话的**落库记录（`landed_tables`）：项目库里本来就有同名表不算它落过。
    pub fn table_references_unlanded(&self, table: &str) -> bool {
        self.outgoing_relations(table).iter().any(|relation| {
            !self
                .landed_tables
                .iter()
                .any(|landed| landed == &relation.parent_table)
        })
    }

    /// 懒创建表头那几个输入（`InputState::new` 需要 window；事件路径置位的待写值也在这里落地）。
    ///
    /// 表头画在**中央 tab**，但输入状态归面板（单一权威）：面板出让的是渲染，不是状态。
    /// 建（或补齐）**某一份配置**的三只输入框。
    ///
    /// 按配置 id 建：中央每个配置 tab 渲染自己的表头时都会调它，这样两个 tab 各有
    /// 自己的输入态；活动指针只决定「生成 / 出口读哪份」，不影响输入框归属。
    fn ensure_inputs_for(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.config_inputs.contains_key(&id) {
            let config = self.config_for(id);
            let initial_name = config.table_name.clone();
            let initial_rows = config.options.rows.to_string();
            let initial_seed = config
                .options
                .seed
                .map(|seed| seed.to_string())
                .unwrap_or_default();

            let table = cx.new(|cx| InputState::new(window, cx).placeholder("目标表名"));
            table.update(cx, |s, cx| s.set_value(initial_name, window, cx));
            let rows = cx.new(|cx| InputState::new(window, cx).placeholder("行数"));
            rows.update(cx, |s, cx| s.set_value(initial_rows, window, cx));
            let seed = cx.new(|cx| InputState::new(window, cx).placeholder("随机种子"));
            seed.update(cx, |s, cx| s.set_value(initial_seed, window, cx));

            let mut subs = Vec::new();
            // 打字即写回**这一份**配置：闭包带配置 id，配置被换出活动位也不会串到别的表。
            // 右 Dock 清单行与中央 tab 标题都读 `draft`（不是输入框），不订阅就会出现
            // 「从按下第一个键到点生成之前，用户眼里的表名与刚打的名字是两个值」。
            subs.push(
                cx.subscribe(&table, move |panel, emitter, ev: &InputEvent, cx| {
                    if !matches!(ev, InputEvent::Change) {
                        return;
                    }
                    // 软校验：打到一半的非法值（空串 / 含空格）不写回、也不报错——
                    // 报错留给「生成」那一刻（`sync_inputs`），打字途中刷红字只会干扰输入
                    if let Ok(name) = validate_table_name(&emitter.read(cx).value()) {
                        let target = panel.config_mut(id);
                        if target.table_name != name {
                            target.table_name = name;
                            cx.notify();
                        }
                    }
                }),
            );
            subs.push(
                cx.subscribe(&rows, move |panel, emitter, ev: &InputEvent, cx| {
                    if !matches!(ev, InputEvent::Change) {
                        return;
                    }
                    if let Ok(value) = parse_rows(&emitter.read(cx).value()) {
                        let target = panel.config_mut(id);
                        if target.options.rows != value {
                            target.options.rows = value;
                            cx.notify();
                        }
                    }
                }),
            );
            subs.push(
                cx.subscribe(&seed, move |panel, emitter, ev: &InputEvent, cx| {
                    if !matches!(ev, InputEvent::Change) {
                        return;
                    }
                    if let Ok(value) = parse_seed(&emitter.read(cx).value()) {
                        let target = panel.config_mut(id);
                        if target.options.seed != value {
                            target.options.seed = value;
                            cx.notify();
                        }
                    }
                }),
            );

            self.config_inputs.insert(
                id,
                ConfigInputs {
                    table,
                    rows,
                    seed,
                    pending_table: None,
                    _subs: subs,
                },
            );
        }
        let pending = self
            .config_inputs
            .get_mut(&id)
            .and_then(|inputs| inputs.pending_table.take());
        if let (Some(value), Some(input)) = (
            pending,
            self.config_inputs
                .get(&id)
                .map(|inputs| inputs.table.clone()),
        ) {
            // `set_value` 不发 Change 事件：这里的回填不会回头触发上面的写回（无回环）
            input.update(cx, |s, cx| s.set_value(value, window, cx));
        }
        // S5：场景种子输入（只在场态渲染；随模板载入预填）
        if self.scenario.is_some() {
            if self.scenario_seed_input.is_none() {
                let state = cx.new(|cx| InputState::new(window, cx).placeholder("场景种子"));
                self.scenario_seed_input = Some(state);
            }
            if let (Some(pending), Some(input)) = (
                self.scenario_seed_pending.take(),
                self.scenario_seed_input.clone(),
            ) {
                input.update(cx, |s, cx| s.set_value(pending, window, cx));
            }
        } else {
            self.scenario_seed_input = None;
            self.scenario_seed_pending = None;
        }
        if !self.column_filters.contains_key(&id) {
            let state =
                cx.new(|cx| InputState::new(window, cx).placeholder("筛选列（列名 / 生成器）"));
            // 搜索词一变就重画：输入框自己会重绘，但列卡片是面板画的
            let sub = cx.subscribe(&state, |_panel, _input, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            });
            self.column_filter_subs.push(sub);
            self.column_filters.insert(id, state);
        }
        // 清单搜索框（工具栏下方 / 清单顶部；搜索词一变就重画，与 `column_filter` 同一订阅模式）
        if self.search_input.is_none() {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder("搜索表名"));
            self.search_input = Some(state);
        }
    }
}
