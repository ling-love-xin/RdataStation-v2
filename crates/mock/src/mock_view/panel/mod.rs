// ==================== 配置面板（右 Dock） ====================

// 继承父模块（mock_view）的全部 use 绑定
use super::*;

// 渲染子模块（impl MockPanel 的渲染方法与 Render trait）
mod render;
// 渲染行的共用表单助手（detail.rs / tests 也调用，经本层再导出到 mock_view）
pub(crate) use render::{complex_form_line, form_line};

/// Mock 配置面板（右 Dock 17.5rem）：目标 + 选项 + 列来源 + 生成 + 出口。
pub struct MockPanel {
    host: Rc<dyn MockHost>,
    pub(crate) draft: MockDraft,
    /// 列 id 计数器（列可增删，键用 id）
    pub(crate) next_id: u64,
    /// 最近一次运行的**结果表**（单表生成 = 1 条；场景模板 = N 条）
    pub(crate) results: Vec<MockGenInfo>,
    /// 预览的**重查取样**（排序 / 多要几行；每张表最多一份，见 [`PreviewCache`]）
    pub(crate) preview_cache: Option<PreviewCache>,
    /// 预览要显示多少行（面板上的「行数」档；生成时只取 [`PREVIEW_ROWS`] 行，要更多得重查）
    preview_limit: usize,
    /// 字段区的列搜索框（懒创建：`InputState` 要 window；搜索词就存在它里，不另存一份）
    column_filter: Option<Entity<InputState>>,
    /// 搜索词变化时重画字段区的订阅（必须持有：丢了就自动退订）
    column_filter_sub: Option<Subscription>,
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
    pub(crate) table_input: Option<Entity<InputState>>,
    /// 待写入表名输入的值（事件路径置位，下一帧渲染时落地）
    table_pending: Option<String>,
    rows_input: Option<Entity<InputState>>,
    seed_input: Option<Entity<InputState>>,
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
    recent_generators: Vec<String>,
    /// 底部折叠段（生成历史 / 用户模板）是否展开（默认收起：面板天天用的是上面那份清单）
    pub(crate) fold_open: bool,
    /// 上次清空结果是不是切项目造成的（旧 tab 的文案据此区分「另一个项目」与「这一轮」）
    project_switched: bool,
}

/// 进行中任务的视图侧状态（进度镜像 + 轮询泵句柄）。
struct MockJobWatch {
    /// 任务种类（收尾时据此分派结果）
    kind: MockJobKind,
    /// 最近一次拉到的进度
    progress: MockJobProgress,
    /// 已请求取消（按钮转「正在取消…」）
    cancel_requested: bool,
    /// 轮询泵（持句柄即存活；任务结束或面板销毁时自动停）
    _pump: Task<()>,
}

/// 交给后台执行器的一件事（读与写共用一条路径）。
#[derive(Clone)]
enum HistoryTask {
    /// 读快照（历史 + 模板）
    List,
    /// 删一条历史后读回
    Delete(String),
    /// 记一次生成运行后读回
    Record { draft: MockDraft, run: RunRecord },
    /// 把草稿存成模板后读回
    SaveTemplate { name: String, draft: MockDraft },
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
            draft: MockDraft::default(),
            next_id: 1,
            results: Vec::new(),
            preview_cache: None,
            preview_limit: PREVIEW_ROWS,
            column_filter: None,
            column_filter_sub: None,
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
            table_input: None,
            table_pending: None,
            rows_input: None,
            seed_input: None,
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
            fold_open: false,
            project_switched: false,
        }
    }

    // ==================== 只读访问器（宿主 / 详情 tab / 测试） ====================

    /// 当前草稿
    pub fn draft(&self) -> &MockDraft {
        &self.draft
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
        self.column_filter.clone()
    }

    /// 字段区的列搜索词（空 = 不筛）。搜索词就存在输入框实体里，不另存一份。
    pub(crate) fn column_filter_query(&self, cx: &App) -> String {
        self.column_filter
            .as_ref()
            .map(|input| input.read(cx).value().trim().to_string())
            .unwrap_or_default()
    }

    /// 字段区表头行里的列搜索框（靠右，固定宽）。
    ///
    /// 搜索框空着也不隐藏：一个偶尔冒出来的控件比一个一直在那儿的小输入框更让人意外。
    pub(crate) fn render_column_filter(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.ensure_inputs(window, cx);
        match self.column_filter.clone() {
            Some(input) => div()
                .flex_none()
                .w(rems(ui::COLUMN_FILTER_WIDTH))
                .child(Input::new(&input)),
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
    /// 草稿（目标表名 / 列 / 行数种子）**保留**：那是用户的配置，跨项目可以继续用。
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

    /// 打开中央「Mock 数据」详情 tab（草稿 tab：字段清单 + 预览）。
    pub fn open_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_detail_for(DetailTarget::Draft, window, cx);
    }

    /// 打开（或聚焦）某个详情 tab：草稿，或某张结果表。
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

    /// 「查看详情」看谁：有结果就看当前表（tab 就是那张表），没结果看草稿。
    pub fn current_detail_target(&self) -> DetailTarget {
        self.results
            .get(self.current)
            .map(|info| DetailTarget::Table(info.table_name.clone()))
            .unwrap_or(DetailTarget::Draft)
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

    /// 重放一条历史：把当时的表名 / 行数 / 种子 / 语言与列配置写回草稿。
    ///
    /// **不自动生成**：重放是「把配置摆回来」，跑不跑由用户定（同模板应用口径）。
    pub fn replay_history(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::Replay(task_id), cx);
    }

    /// 把当前草稿存成用户模板（名字由保存对话框给，空名在这里拦住）。
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

    /// 应用一个用户模板：把它存的行数 / 种子 / 语言与列写回草稿（**不动目标表名**）。
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
        // 旧结果作废：草稿已是另一套配置，临时表还是上一套的
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

    /// 重放落地：草稿整体换成历史里的那一套（纯状态动作，测试直接调）。
    ///
    /// S5：场景历史（`scene_id` 有值）不落草稿——多表配置存不进单表列行，
    /// 重放按模板 id 重新取内置模板并套用当时的种子（编辑过的关系不随历史恢复）。
    pub(crate) fn apply_replay(&mut self, detail: MockGenerationDetail, cx: &mut Context<Self>) {
        if let Some(scene_id) = detail.task.scene_id.clone() {
            let seed = detail.task.seed.map(|seed| seed as u32);
            let Some(mut template) = crate::templates::get_template_by_id(&scene_id) else {
                self.fail(format!("场景模板不存在：{scene_id}（可能已从内置清单移除）"), cx);
                return;
            };
            template.seed = seed;
            let name = template.name.clone();
            self.load_scenario(template, cx);
            self.outcome = Some(format!(
                "已重放场景 {name}（seed {}）：确认后再点生成",
                seed.map(|s| s.to_string()).unwrap_or_else(|| "随机".to_string())
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
        self.table_pending = Some(self.draft.table_name.clone());
        // 旧结果作废：草稿已是另一套配置，临时表还是上一套的
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
        self.table_pending = Some(request.table.clone());
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
    /// 再开始生成（关系是这次生成的一部分，不能生成完再补——那就要去改已落地的数据了）。
    pub fn open_scenario(&mut self, template_id: &str, cx: &mut Context<Self>) {
        if self.is_running() {
            self.fail("已有任务在进行中（请等它结束或先取消）", cx);
            return;
        }
        let Some(template) = crate::templates::get_template_by_id(template_id) else {
            self.fail(format!("场景模板不存在：{template_id}"), cx);
            return;
        };
        self.load_scenario(template, cx);
    }

    /// 把**当前草稿**加进场景工作副本（自定义多表场景的入口）。
    ///
    /// 为何从草稿来：导入源库结构、智能映射、列编辑、行数 / 种子这一整套都在单表态里
    /// 现成可用——“把一张表调到满意，然后把它加进场景”比再做一个多表编辑器便宜得多，
    /// 也不破坏现有交互。
    pub fn add_draft_to_scenario(&mut self, cx: &mut Context<Self>) {
        let name = self.draft.table_name.trim().to_string();
        if name.is_empty() {
            self.fail("草稿的目标表名为空：先在「目标表名」里填一个", cx);
            return;
        }
        if self.draft.columns.is_empty() {
            self.fail("草稿还没有列：先导入源库结构或手工加列", cx);
            return;
        }
        let Some(template) = self.scenario.as_mut() else {
            self.fail("先选一套场景模板（或在模板基础上改）", cx);
            return;
        };
        if template.tables.iter().any(|table| table.name == name) {
            self.fail(
                format!("场景里已经有表 {name}：先删掉那张，或改草稿的目标表名"),
                cx,
            );
            return;
        }
        let table = TemplateTable {
            name: name.clone(),
            row_count: self.draft.options.rows,
            columns: self.draft.columns.iter().map(|c| c.def.clone()).collect(),
        };
        template.tables.push(table);
        self.error = None;
        self.outcome = Some(format!(
            "已把草稿 {name}（{} 行 · {} 列）加进本次生成",
            with_thousands(u64::from(self.draft.options.rows)),
            self.draft.columns.len()
        ));
        cx.notify();
    }

    /// 从场景工作副本里删掉一张表，**并连带清掉指向它的关系**。
    ///
    /// 不清的话，剩余引用会变成“指向模板里没有的表”——生成前校验会报错，
    /// 但那时用户已经不知道是谁指向它了。所以这里直接清掉并报出清了几条。
    pub fn remove_scenario_table(&mut self, table: &str, cx: &mut Context<Self>) {
        let Some(template) = self.scenario.as_mut() else {
            return;
        };
        let before = template.tables.len();
        template.tables.retain(|t| t.name != table);
        if template.tables.len() == before {
            return;
        }
        let mut dropped = 0usize;
        for child in template.tables.iter_mut() {
            for column in child.columns.iter_mut() {
                let points_here = column
                    .dependency
                    .as_ref()
                    .is_some_and(|dep| dep.ref_table == table);
                if points_here {
                    column.dependency = None;
                    dropped += 1;
                }
            }
        }
        self.error = None;
        self.outcome = Some(if dropped == 0 {
            format!("已从本次生成里删掉表 {table}")
        } else {
            format!("已删掉表 {table}，并清掉 {dropped} 条指向它的关系")
        });
        cx.notify();
    }

    /// 载入场景工作副本（[`Self::open_scenario`] 按 id 取到模板后调它；测试直接给模板）。
    pub(crate) fn load_scenario(&mut self, template: ScenarioTemplate, cx: &mut Context<Self>) {
        self.scenario = Some(template.clone());
        // S5：种子输入跟着模板走（重放 / 换模板时把当时的 seed 摆回来）
        self.scenario_seed_pending = template.seed.map(|seed| seed.to_string());
        *self.relation_pick.borrow_mut() = RelationPick::default();
        // 已结果及其关系快照不动：载入工作副本不等于重新生成，旧结果仍然能导出
        self.error = None;
        self.outcome = None;
        cx.notify();
    }

    /// 退出场景态（丢弃工作副本，草稿与已有结果不受影响）。
    pub fn close_scenario(&mut self, cx: &mut Context<Self>) {
        self.scenario = None;
        *self.relation_pick.borrow_mut() = RelationPick::default();
        cx.notify();
    }

    /// 工具栏「▦ 新增场景」：从空白场景（0 张表）开始建工作副本。
    ///
    /// 与「选模板」同一入口（[`Self::load_scenario`]）：进来的是 0 张表的模板，
    /// 之后用「＋ 加表（当前草稿）」「＋ 加关系」从零搭起（`add_draft_to_scenario` 的
    /// 「先选一套场景模板」门槛由空模板本身满足）。
    pub fn new_blank_scene(&mut self, cx: &mut Context<Self>) {
        if self.is_running() {
            self.fail("已有任务在进行中（请等它结束或先取消）", cx);
            return;
        }
        self.load_scenario(crate::templates::blank_scene_template(), cx);
    }

    /// 工具栏「＋」：新建一张草稿表（对齐原型 `newMockTable`——每次点击从一张新表开始配）。
    ///
    /// 单表态的草稿只有一份（清单那一行就是它，design.md 单表态 = 草稿与结果一行），
    /// 所以「清单加一条记录」落到实现里 = 把草稿换成一张**新表**（表名递增、列清空、
    /// 行数复位），并聚焦它的中央 tab（配字段 / 名称都在 tab 里）；上一张若已生成，
    /// 结果仍留在清单（`results`），新表生成后再续一条。
    pub fn new_draft_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_seq += 1;
        self.draft = MockDraft {
            table_name: format!("mock_table_{}", self.draft_seq),
            columns: Vec::new(),
            options: MockRunOptions::new(1000, None, self.draft.options.locale.clone()),
        };
        // 新表从「未生成」开始：清掉上一张的生成反馈（error 留给失败现场，这里不误清）
        self.outcome = None;
        cx.notify();
        self.open_detail_for(DetailTarget::Draft, window, cx);
    }

    /// 工具栏「⟳ 刷新」：重读内存临时表的**当前行数**，同步清单行尾数字。
    ///
    /// 测试过程中可能改过这张表（加字段 / 增删行 / 外部写入）：`results` 里的行数是
    /// 生成时的快照，刷新把它与库内现状对齐。查不到（连接正忙 / 表已被清）保持现状，
    /// 不打断用户——刷新是随手动作，不为它弹错。
    pub fn refresh_tables(&mut self, cx: &mut Context<Self>) {
        let mut changed = 0usize;
        for info in self.results.iter_mut() {
            if let Ok(Some(rows)) = self.host.temp_table_row_count(&info.temp_table_name) {
                if info.row_count != rows as u32 {
                    info.row_count = rows as u32;
                    changed += 1;
                }
            }
        }
        self.outcome = Some(if changed == 0 {
            "已刷新：临时表行数与清单一致".to_string()
        } else {
            format!("已刷新：{changed} 张表行数已同步")
        });
        self.error = None;
        cx.notify();
    }

    /// 场景态下的「生成」（后台任务：一次产出模板里的全部表；**不写库**）。
    pub fn run_scenario(&mut self, cx: &mut Context<Self>) {
        let Some(mut template) = self.scenario.clone() else {
            self.fail("先选一套场景模板", cx);
            return;
        };
        // S5：种子输入 → 模板 seed（留空 = 随机，与单表 seed 输入同一语义）。
        // 同步在这里而不是 `sync_inputs`：场景不读草稿，输入也只在此刻消费。
        if let Some(input) = self.scenario_seed_input.clone() {
            match parse_seed(&input.read(cx).value()) {
                Ok(seed) => template.seed = seed,
                Err(e) => {
                    self.fail(e, cx);
                    return;
                }
            }
        }
        // 空白场景（0 张表）先别提交：生成空结果没有意义，引导用户先加第一张表
        if template.tables.is_empty() {
            self.fail("空白场景还没有表：先用「＋ 加表（当前草稿）」添加第一张", cx);
            return;
        }
        // 生成前校验引用（父表 / 父列不合适就不提交；错误在面板上就地显示）
        if let Err(e) = MockEngine::resolve_reference_domains(&template) {
            self.fail(format!("表间关系有问题：{e}"), cx);
            return;
        }
        self.start_job(MockJobKind::Scenario(Box::new(template)), cx);
    }

    /// 场景工作副本（`None` = 单表态）。
    pub fn scenario(&self) -> Option<&ScenarioTemplate> {
        self.scenario.as_ref()
    }

    /// 关系闭包里**还没落库**的表（按结果表顺序；已落库的跳过）。
    ///
    /// 为何要它：关系是跨表的，而出口以「当前表」为单位——用户很容易只落其中
    /// 几张，留下悬空的引用。这里把「还差哪几张」摆出来，让他一次落完。
    /// 空 = 当前表不参与关系，或关系里的表都落了。
    pub fn pending_relation_tables(&self) -> Vec<String> {
        if self.last_relations.is_empty() {
            return Vec::new();
        }
        let Some(current) = self.current_info().map(|info| info.table_name.clone()) else {
            return Vec::new();
        };
        // 从当前表出发沿关系双向走一遍，得到它与关系闭包（父子链）
        let mut closure = vec![current];
        loop {
            let mut added = false;
            for relation in &self.last_relations {
                for (from, to) in [
                    (&relation.child_table, &relation.parent_table),
                    (&relation.parent_table, &relation.child_table),
                ] {
                    if closure.iter().any(|name| name == from)
                        && !closure.iter().any(|name| name == to)
                    {
                        closure.push(to.clone());
                        added = true;
                    }
                }
            }
            if !added {
                break;
            }
        }
        self.results
            .iter()
            .map(|info| info.table_name.clone())
            .filter(|name| closure.iter().any(|c| c == name))
            .filter(|name| !self.landed_tables.iter().any(|landed| landed == name))
            .collect()
    }

    /// 本会话已落库的表。
    pub fn landed_tables(&self) -> &[String] {
        &self.landed_tables
    }

    /// 出口：把关系闭包里还没落库的表**依次**落成分析库新表（后台任务）。
    ///
    /// 逐张走「新建」语义：同名已存在就跳过它并报出原因（不覆盖、也不回滚别人）。
    pub fn persist_related(&mut self, cx: &mut Context<Self>) {
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        let pending = self.pending_relation_tables();
        if pending.is_empty() {
            self.fail("关系里没有待落库的表", cx);
            return;
        }
        tracing::info!(
            tables = ?pending,
            "【探针】Mock「落库这些表」(persist_related) 触发"
        );
        let infos: Vec<MockGenInfo> = self
            .results
            .iter()
            .filter(|info| pending.contains(&info.table_name))
            .cloned()
            .collect();
        self.start_job(MockJobKind::PersistAll(infos), cx);
    }

    /// 批量存草稿箱：把当前所有结果表一次写到 `{项目}/scratchpad/mock/`。
    ///
    /// 与「落库这 N 张」对称：落库走分析库新表，草稿箱走 CSV 文件。
    /// 没项目根直接拦（草稿箱只写项目目录；出口区按钮也据此禁用）。
    fn scratchpad_all(&mut self, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            self.fail("没有结果表可导出", cx);
            return;
        }
        if self.host.project_root().is_none() {
            self.fail("未打开项目：草稿箱只写项目目录", cx);
            return;
        }
        let infos = self.results.clone();
        self.start_job(
            MockJobKind::ScratchpadAll {
                infos,
                format: MockExportFormat::Csv,
            },
            cx,
        );
    }

    /// 当前结果表的**跨表后果**（用于出口提示；`None` = 它不参与任何关系）。
    ///
    /// 为何要给这句：**出口只作用于当前表**，而关系是跨表的——只落子表不落父表，
    /// 落地的数据就悬空了（而 mock 不改已落地的数据，也不会去替用户补）。
    pub fn current_relation_note(&self) -> Option<String> {
        let table = self.current_info()?.table_name.clone();
        self.relation_note_for(&table)
    }

    /// 指定那张表的**跨表后果**（结果表 tab 的表头用：那张 tab 看的可能是别的表，见 D35）。
    pub(crate) fn relation_note_for(&self, table: &str) -> Option<String> {
        let current = table.to_string();
        let mut out: Vec<String> = Vec::new();
        let parents: Vec<String> = self
            .last_relations
            .iter()
            .filter(|r| r.child_table == current)
            .map(|r| {
                format!(
                    "{} → {}.{}",
                    r.child_column, r.parent_table, r.parent_column
                )
            })
            .collect();
        if !parents.is_empty() {
            let mut parents = parents;
            parents.sort();
            parents.dedup();
            out.push(format!(
                "引用了 {}：只落这张表，被引用的表不会跟着落库",
                parents.join("、")
            ));
        }
        let children: Vec<String> = self
            .last_relations
            .iter()
            .filter(|r| r.parent_table == current)
            .map(|r| format!("{}.{}", r.child_table, r.child_column))
            .collect();
        if !children.is_empty() {
            let mut children = children;
            children.sort();
            children.dedup();
            out.push(format!(
                "被 {} 引用：只落这张表，引用它的一方会落空",
                children.join("、")
            ));
        }
        (!out.is_empty()).then(|| out.join("；"))
    }

    /// 最近一次**场景生成**用的关系快照（结果还在就保留；单表生成会清掉）。
    pub fn last_relations(&self) -> &[ScenarioRelation] {
        &self.last_relations
    }
    /// 工作副本里的表间关系（派生视图：扫列上的 `dependency`，不另存清单）。
    pub fn scenario_relations(&self) -> Vec<ScenarioRelation> {
        let Some(template) = self.scenario.as_ref() else {
            return Vec::new();
        };
        relations_of(template)
    }

    /// 关系目标的**取值域**文案（`1..1,000`）；父列自增且父表在册时给出。
    ///
    /// 面板在关系行里显示它：改父表行数前后，用户能直接看到采样域变了——
    /// 这正是“域由父表行数算出”的可见回报。
    pub fn relation_range(&self, relation: &ScenarioRelation) -> Option<String> {
        let template = self.scenario.as_ref()?;
        let parent = template
            .tables
            .iter()
            .find(|t| t.name == relation.parent_table)?;
        let column = parent
            .columns
            .iter()
            .find(|c| c.name == relation.parent_column)?;
        let GeneratorConfig::AutoIncrement { start, step } = column.generator else {
            return None;
        };
        let domain = ReferenceDomain {
            table: parent.name.clone(),
            column: column.name.clone(),
            first: i64::from(start),
            step: i64::from(step),
            count: parent.row_count,
        };
        Some(format!(
            "{}..{}",
            with_thousands(domain.first.max(0) as u64),
            with_thousands(domain.last().max(0) as u64)
        ))
    }

    /// 打开「编辑表」对话框（表名 / 行数）。
    ///
    /// 改行数是场景里最常用的一步（比如把财务模板的 10 万行缩下来），而引用它的表
    /// **自动跟着变**——域由父表行数算出，不需要用户去同步任何东西。
    pub fn open_table_dialog(&mut self, table: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self
            .scenario
            .as_ref()
            .and_then(|template| template.tables.iter().find(|t| t.name == table))
            .cloned()
        else {
            self.fail("先选一套场景模板", cx);
            return;
        };
        self.table_editing = Some(current.name.clone());
        // 上一次的错误留在面板上会让人以为是这次的输入有问题：打开就清掉
        self.error = None;
        let name_input = self
            .edit_name_input
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("表名")))
            .clone();
        let rows_input = self
            .edit_rows_input
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("行数")))
            .clone();
        name_input.update(cx, |state, cx| {
            state.set_value(current.name.clone(), window, cx)
        });
        rows_input.update(cx, |state, cx| {
            state.set_value(current.row_count.to_string(), window, cx)
        });

        let panel = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let muted = theme.colors.muted_foreground;
            let danger = theme.colors.danger;
            // 校验没过时对话框不关：错误就地显示，改完能直接再点「应用」
            let error = panel.read(cx).error.clone();
            let mut body =
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div().text_xs().text_color(muted).child(
                            "只改这张表在本次生成里的配置；已生成的预览与已落地的表都不动。",
                        ),
                    )
                    .child(form_line(theme, "表名", &name_input))
                    .child(form_line(theme, "行数", &rows_input))
                    .child(
                        div().text_xs().text_color(muted).child(
                            "引用它的表会自动跟着变：取值域由父表行数算出（改完下次生成生效）",
                        ),
                    );
            if let Some(error) = error {
                body = body.child(div().text_xs().text_color(danger).child(error));
            }
            let cancel_panel = panel.clone();
            let ok_panel = panel.clone();
            dialog.title("编辑表").child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("mock-table-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(move |_, window, app| {
                                let _ = cancel_panel
                                    .update(app, |panel, _cx| panel.table_editing = None);
                                window.close_dialog(app);
                            }),
                    )
                    .child(
                        Button::new("mock-table-ok")
                            .with_variant(ButtonVariant::Primary)
                            .label("应用")
                            .on_click(move |_, window, app| {
                                let applied = ok_panel.update(app, |panel, cx| {
                                    panel.apply_table_edit(cx);
                                    panel.table_editing.is_none()
                                });
                                if applied {
                                    window.close_dialog(app);
                                }
                            }),
                    ),
            )
        });
    }

    /// 应用「编辑表」：校验 → 改工作副本 → **同步指向它的关系的 `ref_table`**。
    pub fn apply_table_edit(&mut self, cx: &mut Context<Self>) {
        let Some(original) = self.table_editing.clone() else {
            return;
        };
        // 表名走与单表路径**同一个**校验（非空 / 字母数字下划线 / 不以数字开头）：
        // 让非法名在这里就被拦住，而不是到生成时才由引擎报出来
        let name = match self
            .edit_name_input
            .as_ref()
            .map(|input| validate_table_name(&input.read(cx).value()))
        {
            Some(Ok(name)) => name,
            Some(Err(e)) => {
                self.fail(e, cx);
                return;
            }
            None => return,
        };
        let rows = match self
            .edit_rows_input
            .as_ref()
            .map(|input| parse_rows(&input.read(cx).value()))
        {
            Some(Ok(rows)) => rows,
            Some(Err(e)) => {
                self.fail(e, cx);
                return;
            }
            None => return,
        };
        let renamed = name != original;
        let Some(template) = self.scenario.as_mut() else {
            return;
        };
        if renamed && template.tables.iter().any(|t| t.name == name) {
            self.fail(format!("本次生成里已经有表 {name}"), cx);
            return;
        }
        let Some(table) = template.tables.iter_mut().find(|t| t.name == original) else {
            return;
        };
        table.name = name.clone();
        table.row_count = rows;
        let mut retargeted = 0usize;
        if renamed {
            // 改了名，指向它的引用必须跟着改——否则立即变成“指向模板里没有的表”
            for child in template.tables.iter_mut() {
                for column in child.columns.iter_mut() {
                    let points_here = column.dependency.as_mut().is_some_and(|dep| {
                        if dep.ref_table == original.as_str() {
                            dep.ref_table = name.clone();
                            true
                        } else {
                            false
                        }
                    });
                    if points_here {
                        retargeted += 1;
                    }
                }
            }
        }
        self.table_editing = None;
        self.error = None;
        self.outcome = Some(if retargeted == 0 {
            format!(
                "已更新表 {name}（{} 行）：下次生成生效",
                with_thousands(u64::from(rows))
            )
        } else {
            format!(
                "已更新表 {name}（{} 行），并同步 {retargeted} 条指向它的引用：下次生成生效",
                with_thousands(u64::from(rows))
            )
        });
        cx.notify();
    }

    /// 打开「加关系」对话框（子表.列 → 父表.列）。
    ///
    /// 父列只列**自增列**：只有它能算出取值域，其他列在引擎侧会被拒——
    /// 把不能选的东西藏起来，比先让用户选完再报错好。
    pub fn open_relation_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(template) = self.scenario.clone() else {
            self.fail("先选一套场景模板", cx);
            return;
        };
        // 每次打开都从「未选」开始：上一次的选择带到下一次容易误改
        *self.relation_pick.borrow_mut() = RelationPick::default();
        let pick = self.relation_pick.clone();
        let panel = cx.entity();
        let template = Rc::new(template);

        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let muted = theme.colors.muted_foreground;
            let warning = theme.colors.warning;
            let current = pick.borrow().clone();
            let table_names: Vec<String> =
                template.tables.iter().map(|t| t.name.clone()).collect();
            let child_columns: Vec<String> = current
                .child_table
                .as_deref()
                .and_then(|name| template.tables.iter().find(|t| t.name == name))
                .map(|t| t.columns.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default();
            let parent_columns: Vec<String> = current
                .parent_table
                .as_deref()
                .and_then(|name| template.tables.iter().find(|t| t.name == name))
                .map(|t| {
                    t.columns
                        .iter()
                        .filter(|c| matches!(c.generator, GeneratorConfig::AutoIncrement { .. }))
                        .map(|c| c.name.clone())
                        .collect()
                })
                .unwrap_or_default();

            // 一个下拉行：标签 + 当前值按钮 + 菜单
            let row = |id: &'static str,
                       label: &'static str,
                       current: Option<&String>,
                       placeholder: &'static str,
                       items: Vec<String>,
                       empty_note: &'static str,
                       on_pick: Rc<dyn Fn(String, &mut App)>| {
                let pick_for_menu = on_pick.clone();
                let text = match current {
                    Some(value) => format!("{value} ▾"),
                    None => format!("{placeholder} ▾"),
                };
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .w(rems(4.0))
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(label),
                    )
                    .child(
                        Button::new(id)
                            .ghost()
                            .label(text)
                            .dropdown_menu(move |menu, _window, _cx| {
                                let mut menu = menu;
                                if items.is_empty() {
                                    return menu.item(PopupMenuItem::new(empty_note).disabled(true));
                                }
                                for item in items.iter() {
                                    let value = item.clone();
                                    let pick_for_item = pick_for_menu.clone();
                                    menu = menu.item(PopupMenuItem::new(item.clone()).on_click(
                                        move |_, _, app| pick_for_item(value.clone(), app),
                                    ));
                                }
                                menu
                            }),
                    )
            };

            // 每个下拉的点击：改工作选择，再请面板重绘（对话框由窗口重绘时重建）
            let pick_child_table = pick.clone();
            let panel_child_table = panel.clone();
            let on_child_table = Rc::new(move |value: String, app: &mut App| {
                let mut pick = pick_child_table.borrow_mut();
                pick.child_table = Some(value);
                pick.child_column = None;
                drop(pick);
                panel_child_table.update(app, |_, cx| cx.notify());
            });
            let pick_child_column = pick.clone();
            let panel_child_column = panel.clone();
            let on_child_column = Rc::new(move |value: String, app: &mut App| {
                pick_child_column.borrow_mut().child_column = Some(value);
                panel_child_column.update(app, |_, cx| cx.notify());
            });
            let pick_parent_table = pick.clone();
            let panel_parent_table = panel.clone();
            let on_parent_table = Rc::new(move |value: String, app: &mut App| {
                let mut pick = pick_parent_table.borrow_mut();
                pick.parent_table = Some(value);
                pick.parent_column = None;
                drop(pick);
                panel_parent_table.update(app, |_, cx| cx.notify());
            });
            let pick_parent_column = pick.clone();
            let panel_parent_column = panel.clone();
            let on_parent_column = Rc::new(move |value: String, app: &mut App| {
                pick_parent_column.borrow_mut().parent_column = Some(value);
                panel_parent_column.update(app, |_, cx| cx.notify());
            });

            let ready = current.child_table.is_some()
                && current.child_column.is_some()
                && current.parent_column.is_some();
            let mut body = div()
                .v_flex()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("引用只在**本次多表生成**内成立：值取自父表主键的取值域（由自增参数与行数算出）。"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("不读已落地的数据，也不修改已生成的表 / 文件。"),
                )
                .child(row(
                    "mock-relation-child-table",
                    "子表",
                    current.child_table.as_ref(),
                    "选择子表",
                    table_names.clone(),
                    "（模板里没有表）",
                    on_child_table,
                ))
                .child(row(
                    "mock-relation-child-column",
                    "子列",
                    current.child_column.as_ref(),
                    "选择子列",
                    child_columns,
                    "（先选子表）",
                    on_child_column,
                ))
                .child(row(
                    "mock-relation-parent-table",
                    "父表",
                    current.parent_table.as_ref(),
                    "选择父表",
                    table_names,
                    "（模板里没有表）",
                    on_parent_table,
                ))
                .child(row(
                    "mock-relation-parent-column",
                    "父列",
                    current.parent_column.as_ref(),
                    "选择父列",
                    parent_columns.clone(),
                    "（先选父表）",
                    on_parent_column,
                ));
            if current.parent_table.is_some() && parent_columns.is_empty() {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(warning)
                        .child("这张表没有自增主键列——引用目标只能是自增列（域算得出才不用去读数据）"),
                );
            }

            let cancel_panel = panel.clone();
            let ok_panel = panel.clone();
            dialog.title("加表间关系").child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("mock-relation-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(move |_, window, app| {
                                let _ = cancel_panel.update(app, |panel, cx| {
                                    *panel.relation_pick.borrow_mut() = RelationPick::default();
                                    cx.notify();
                                });
                                window.close_dialog(app);
                            }),
                    )
                    .child({
                        let mut ok = Button::new("mock-relation-ok")
                            .with_variant(ButtonVariant::Primary)
                            .label("添加")
                            .disabled(!ready);
                        if ready {
                            ok = ok.on_click(move |_, window, app| {
                                let _ = ok_panel.update(app, |panel, cx| panel.add_relation(cx));
                                window.close_dialog(app);
                            });
                        }
                        ok
                    }),
            )
        });
    }

    /// 把对话框里选好的四个位置写成一条关系（挂在子列上）。
    ///
    /// 同时把该列的 `generator` **对齐到父域**：单表生成时按它取值，
    /// 域必须落在父域内（否则同一列换个入口就产出不一样的值）。
    pub(crate) fn add_relation(&mut self, cx: &mut Context<Self>) {
        let pick = self.relation_pick.borrow().clone();
        let (Some(child_table), Some(child_column), Some(parent_table), Some(parent_column)) = (
            pick.child_table,
            pick.child_column,
            pick.parent_table,
            pick.parent_column,
        ) else {
            self.fail("子表 / 子列 / 父表 / 父列都要选", cx);
            return;
        };

        let mut problem: Option<String> = None;
        let mut message = String::new();
        if let Some(template) = self.scenario.as_mut() {
            let domain = template
                .tables
                .iter()
                .find(|t| t.name == parent_table)
                .and_then(|parent| {
                    let column = parent.columns.iter().find(|c| c.name == parent_column)?;
                    let GeneratorConfig::AutoIncrement { start, step } = column.generator else {
                        return None;
                    };
                    Some(ReferenceDomain {
                        table: parent_table.clone(),
                        column: parent_column.clone(),
                        first: i64::from(start),
                        step: i64::from(step),
                        count: parent.row_count,
                    })
                });
            match domain {
                None => {
                    problem = Some(format!(
                        "{parent_table}.{parent_column} 不能作引用目标（只支持自增主键列）"
                    ));
                }
                Some(domain) => {
                    let child = template
                        .tables
                        .iter_mut()
                        .find(|t| t.name == child_table)
                        .and_then(|t| t.columns.iter_mut().find(|c| c.name == child_column));
                    match child {
                        None => {
                            problem = Some(format!("子表 {child_table} 没有列 {child_column}"));
                        }
                        Some(column) => {
                            column.dependency =
                                Some(ColumnDependency::referenced(&parent_table, &parent_column));
                            column.generator = GeneratorConfig::RandomInt {
                                min: domain.first as i32,
                                max: domain.last() as i32,
                            };
                            message = format!(
                                "已加关系 {child_table}.{child_column} → {}",
                                domain.label()
                            );
                        }
                    }
                }
            }
        }
        // 提交前再校一遍：同一列被指向两张表之类的问题在这里就能看出来。
        // 不覆盖已有问题：用户刚做的动作报出来的原因（父列不可用）比它的下游后果更直接。
        if problem.is_none() {
            let check = match self.scenario.as_ref() {
                Some(template) => MockEngine::resolve_reference_domains(template).err(),
                None => None,
            };
            if let Some(e) = check {
                problem = Some(format!("表间关系有问题：{e}"));
            }
        }
        *self.relation_pick.borrow_mut() = RelationPick::default();
        match problem {
            Some(problem) => self.fail(problem, cx),
            None => self.succeed(message, cx),
        }
    }

    /// 删一条关系（只拆引用；该列的生成器保留，它现在是普通随机列）。
    pub fn remove_relation(
        &mut self,
        child_table: &str,
        child_column: &str,
        cx: &mut Context<Self>,
    ) {
        let mut removed = false;
        if let Some(template) = self.scenario.as_mut() {
            if let Some(column) = template
                .tables
                .iter_mut()
                .find(|t| t.name == child_table)
                .and_then(|t| t.columns.iter_mut().find(|c| c.name == child_column))
            {
                removed = column.dependency.take().is_some();
            }
        }
        if removed {
            self.outcome = Some(format!("已删除关系 {child_table}.{child_column}"));
            self.error = None;
            cx.notify();
        }
    }

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
    pub fn open_append_dialog(&mut self, table: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        // 隐含主键 = 草稿里 AutoIncrement 生成器的列（v2 不建主键 DDL，自增列就是主键）
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
            let muted = theme.colors.muted_foreground;
            let warning = theme.colors.warning;
            let info = theme.colors.info;
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
                    body = body
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_none()
                                        .text_xs()
                                        .text_color(muted)
                                        .child("目标表现状"),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_xs()
                                        .text_color(fg)
                                        .child(format!(
                                            "{} 行 · {pk_text}",
                                            with_thousands(status.row_count.max(0) as u64)
                                        )),
                                ),
                        );
                    // 自增起点预告：本次生成的自增列会从 MAX + 1 接续
                    if let (Some(pk), Some(max)) = (&status.pk_column, status.max_pk) {
                        body = body.child(
                            div()
                                .text_xs()
                                .text_color(info)
                                .child(format!("{pk} 从 {} 开始接续（MAX + 1）", max + 1)),
                        );
                    }
                }
                None => {
                    body = body.child(
                        div()
                            .text_xs()
                            .text_color(warning)
                            .child("读不到目标表现状：将按现有行数接续自增，唯一冲突会整批拒绝。"),
                    );
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
                            Button::new("mock-append-cancel")
                                .secondary()
                                .label("取消")
                                .on_click(move |_, window, app| {
                                    let _ = cancel_panel.update(app, |panel, _cx| {
                                        *panel.append_pick.borrow_mut() = None;
                                    });
                                    window.close_dialog(app);
                                }),
                        )
                        .child(
                            Button::new("mock-append-ok")
                                .with_variant(ButtonVariant::Primary)
                                .label("确认追加")
                                .on_click(move |_, window, app| {
                                    let _ = ok_panel.update(app, |panel, cx| {
                                        panel.append_table(table_for_ok.clone(), cx);
                                    });
                                    window.close_dialog(app);
                                }),
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

    /// 启动后台任务（生成 / 追加 / 出口）：校验输入 → 提交 → 起轮询泵。
    fn start_job(&mut self, kind: MockJobKind, cx: &mut Context<Self>) {
        if self.job.is_some() {
            self.fail("已有任务在进行中（请等它结束或先取消）", cx);
            return;
        }
        // 草稿驱动的任务才校验草稿：场景模板与出口都不读草稿（见 `MockJobKind::uses_draft`），
        // 拿它们的输入拦截会给出与当前动作无关的错误
        if kind.uses_draft() {
            if let Err(e) = self.sync_inputs(cx) {
                self.fail(e, cx);
                return;
            }
            if self.draft.columns.is_empty() {
                self.fail("请先添加列：导入源库结构，或手工加列", cx);
                return;
            }
        }
        if let Err(e) = self.host.start_job(&self.draft, kind.clone()) {
            self.fail(e, cx);
            return;
        }
        // 记下提交时的项目：出口任务的写入路径也是那一刻快照的（工作线程碰不了 `Shared`），
        // 收尾时用它判断归属（见 `finish_job`）
        self.job_project = self.host.project_root();
        let progress = MockJobProgress {
            phase: kind.phase(),
            batches_done: 0,
            batches_total: 0,
            rows_total: kind.rows_total(&self.draft),
        };
        self.error = None;
        self.outcome = Some(kind.running_label());
        self.job = Some(MockJobWatch {
            kind,
            progress,
            cancel_requested: false,
            _pump: self.spawn_job_pump(cx),
        });
        cx.notify();
    }

    /// 起后台任务轮询泵：每 120ms 拉一次进度 / 结果，任务收尾后自退。
    ///
    /// 用「弱句柄 + 定时器」而非 render 内轮询：任务进行中没有任何其他事件会让
    /// 面板重绘，不主动唤醒就看不到进度。
    fn spawn_job_pump(&self, cx: &mut Context<Self>) -> Task<()> {
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(120)).await;
                let keep = weak
                    .update(cx, |panel, cx| panel.poll_job(cx))
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
        })
    }

    /// 拉一次任务状态（返回「是否继续轮询」；窗口测试直接调它，不依赖定时器）。
    pub(crate) fn poll_job(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(kind) = self.job.as_ref().map(|job| job.kind.clone()) else {
            return false;
        };

        // 结果优先：worker 收尾时一次性「清进度 + 写结果」，不会出现两者皆无的观测空洞
        if let Some(result) = self.host.take_job_done() {
            self.job = None;
            self.finish_job(&kind, result, cx);
            return false;
        }

        match self.host.job_state() {
            MockJobState::Running(progress) => {
                let mut notify = false;
                if let Some(job) = self.job.as_mut() {
                    // 取消要**持续压**：任务可能跨多个轮询周期才结束（S4 起取消是任务级令牌），
                    // 这里每个轮询周期重发一次直到任务真的结束——`cancel_job` 幂等，重发无副作用。
                    if job.cancel_requested {
                        self.host.cancel_job();
                    }
                    if job.progress != progress {
                        job.progress = progress;
                        notify = true;
                    }
                }
                if notify {
                    cx.notify();
                }
                true
            }
            MockJobState::Idle => {
                // 结果槽空且无进行中任务：后台线程异常（崩溃 / 被杀）
                self.job = None;
                self.fail("后台生成任务异常结束（工作线程已退出）", cx);
                false
            }
        }
    }

    /// 任务收尾：写回结果（成功文案 / 落库目标）或错误。
    ///
    /// **生成类**不论成败都先作废旧结果：任务内部重建过临时表，旧预览与临时表已不一致，
    /// 留着它就会让出口拿新数据配旧预览。**出口类**只读临时表，预览仍然对得上，
    /// 不能一并作废——否则「落库完想接着导出」就没得导了。
    fn finish_job(
        &mut self,
        kind: &MockJobKind,
        result: Result<MockJobDone, String>,
        cx: &mut Context<Self>,
    ) {
        if kind.generates() {
            self.results.clear();
            self.current = 0;
        }
        // 任务是在**哪个项目**下提交的：出口类任务的写入路径在提交那一刻快照（`JobPaths`），
        // 切项目不会改写入点。结果回来时若项目已变，得说清楚，别让用户在新项目里找一张不存在的表
        // （临时表名 / `landed_tables` 也都是上一个项目的）。
        let project_changed = self.job_project != self.host.project_root();
        // 历史在任务收尾时记（写库在后台）：出口类与取消不记，见 `RunRecord::of`。
        // 跨项目的结果**不记**：历史属于项目，把上一个项目跑出来的东西记到当前项目头上是误导。
        let record = if project_changed {
            None
        } else {
            RunRecord::of(kind, &result, &self.draft)
        };
        match result {
            Ok(MockJobDone::Generated(info)) => {
                self.project_switched = false;
                self.landed = None;
                self.error = None;
                self.outcome = Some(format!(
                    "已生成 {} 行（耗时 {} ms）→ 临时表 {}",
                    with_thousands(info.row_count as u64),
                    info.elapsed_ms,
                    info.temp_table_name
                ));
                self.results = vec![info];
                self.current = 0;
                self.scenario_source = None;
                // 单表结果没有跨表关系可言：清掉上一轮的场景关系快照
                self.last_relations.clear();
                self.host.notify(cx);
            }
            Ok(MockJobDone::ScenarioGenerated {
                template_name,
                tables,
            }) => {
                self.project_switched = false;
                self.landed = None;
                self.error = None;
                let total: u32 = tables.iter().map(|table| table.row_count).sum();
                self.outcome = Some(format!(
                    "已按「{template_name}」生成 {} 张表（合计 {} 行）：出口作用于当前选中的那张",
                    tables.len(),
                    with_thousands(total as u64)
                ));
                // 关系快照随结果留存：出口要按它提醒「只落这张会怎么悬空」，
                // 而工作副本随时可能被改或退出场景态
                self.last_relations = self.scenario.as_ref().map(relations_of).unwrap_or_default();
                self.results = tables;
                self.current = 0;
                self.scenario_source = Some(template_name);
                self.host.notify(cx);
            }
            Ok(MockJobDone::Appended { table, total_rows }) => {
                self.landed = Some(table.clone());
                self.error = None;
                self.outcome = Some(format!(
                    "已追加到 {table}（表内共 {} 行）",
                    with_thousands(total_rows.max(0) as u64)
                ));
                self.host.notify(cx);
            }
            Ok(MockJobDone::Persisted { table, rows }) => {
                self.landed = Some(table.clone());
                if !self.landed_tables.contains(&table) {
                    self.landed_tables.push(table.clone());
                }
                // 新表要能立刻作为「追加到既有表」的目标
                self.existing_tables = self.host.existing_tables();
                self.succeed(
                    format!(
                        "已在项目分析库新建表 {table}（{} 行）",
                        with_thousands(rows.max(0) as u64)
                    ),
                    cx,
                );
            }
            Ok(MockJobDone::PersistedAll { landed, failed }) => {
                for (table, _) in &landed {
                    if !self.landed_tables.contains(table) {
                        self.landed_tables.push(table.clone());
                    }
                }
                self.landed = landed.last().map(|(table, _)| table.clone());
                self.existing_tables = self.host.existing_tables();
                let mut parts: Vec<String> = Vec::new();
                if !landed.is_empty() {
                    parts.push(format!(
                        "已落库 {} 张：{}",
                        landed.len(),
                        landed
                            .iter()
                            .map(|(table, _)| table.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    ));
                }
                if !failed.is_empty() {
                    parts.push(format!(
                        "失败 {} 张：{}",
                        failed.len(),
                        failed
                            .iter()
                            .map(|(table, reason)| format!("{table}（{reason}）"))
                            .collect::<Vec<_>>()
                            .join("；")
                    ));
                }
                if failed.is_empty() {
                    self.succeed(parts.join("；"), cx);
                } else {
                    // 部分失败：成功的保留（不回滚别人的表），失败的原因原样摆出来
                    self.error = Some(parts.join("；"));
                    self.outcome = None;
                    self.host.notify(cx);
                    cx.notify();
                }
            }
            Ok(MockJobDone::Exported { message }) => self.succeed(message, cx),
            Ok(MockJobDone::ScratchpadAll { saved, failed }) => {
                // 与 PersistedAll 同口径：成功的不回滚，部分失败时把原因摆出来
                let mut parts: Vec<String> = Vec::new();
                if !saved.is_empty() {
                    parts.push(format!(
                        "已存草稿箱 {} 张：{}",
                        saved.len(),
                        saved.join("、")
                    ));
                }
                if !failed.is_empty() {
                    parts.push(format!(
                        "失败 {} 张：{}",
                        failed.len(),
                        failed
                            .iter()
                            .map(|(table, reason)| format!("{table}（{reason}）"))
                            .collect::<Vec<_>>()
                            .join("；")
                    ));
                }
                if failed.is_empty() {
                    self.succeed(parts.join("；"), cx);
                } else {
                    self.error = Some(parts.join("；"));
                    self.outcome = None;
                    self.host.notify(cx);
                    cx.notify();
                }
            }
            Err(e) => {
                let cancelled = e.contains("取消");
                // 同名表已存在是落库失败的常见情形：刷新清单，引导到「追加到既有表」
                if !cancelled && matches!(kind, MockJobKind::Persist(_)) && e.contains("已存在")
                {
                    self.existing_tables = self.host.existing_tables();
                }
                self.outcome = None;
                self.error = Some(match (kind, cancelled) {
                    // 用户主动取消：不当错误报，但要提醒临时表可能残留部分行
                    (_, true) => format!("{e}（临时表可能残留部分行，下次生成会重建）"),
                    // 追加失败与生成失败区分开：追加还多一层「写入既有表」的语义
                    (MockJobKind::AppendTo(_), false) => format!("追加失败：{e}"),
                    (_, false) => e,
                });
            }
        }
        cx.notify();
        if let Some(run) = record {
            let draft = self.draft.clone();
            self.spawn_history(HistoryTask::Record { draft, run }, cx);
        }
        // 项目已变：给已写好的成败文案追加一句归属说明（结果本身仍展示——它就是刚跑出来的）
        if project_changed {
            let note =
                "（注意：这个任务是在上一个项目下提交的，写入的也是那个项目；当前项目没有变化）";
            match (&mut self.outcome, &mut self.error) {
                (Some(text), _) => text.push_str(note),
                (None, Some(text)) => text.push_str(note),
                (None, None) => self.outcome = Some(note.trim_start_matches('（').to_string()),
            }
        }
    }

    /// 请求取消进行中的任务：只有**含生成阶段**的任务能取消（引擎按批响应）。
    ///
    /// 出口类跑在 DuckDB / 文件系统内部，探不到中断点，强杀会留下半张表或半个文件，
    /// 所以不给取消：按钮也不渲染（见 `render_job_row`）。
    pub fn cancel_job(&mut self, cx: &mut Context<Self>) {
        let Some(job) = self.job.as_mut() else {
            return;
        };
        if !job.kind.generates() || job.cancel_requested {
            return;
        }
        job.cancel_requested = true;
        self.host.cancel_job();
        cx.notify();
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

    /// 手工加列（按类型给默认生成器）。
    pub fn add_column(&mut self, name: String, data_type: ColumnDataType, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        let spec = new_column_spec(id, name, data_type);
        self.draft.columns.push(spec);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 从字段预设加列（列名冲突自动加序号：`name` → `name_2`）。
    /// 应用后是普通列，照常走列编辑对话框微调，不锁定后续编辑。
    pub fn add_preset_column(&mut self, preset: &FieldPreset, cx: &mut Context<Self>) {
        let base = preset.column.name.clone();
        let mut name = base.clone();
        let mut n = 2u32;
        while self.draft.columns.iter().any(|c| c.def.name == name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        let id = self.next_id;
        self.next_id += 1;
        let spec = MockColumnSpec {
            id,
            def: ColumnDef {
                name,
                ..preset.column.clone()
            },
            confidence: preset.confidence.to_string(),
            sample_value: String::new(),
        };
        self.draft.columns.push(spec);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 删列。
    pub fn remove_column(&mut self, id: u64, cx: &mut Context<Self>) {
        self.draft.columns.retain(|c| c.id != id);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 调整列顺序（↑/↓）：列序 = 生成顺序与结果表列序，多列场景需要能改。
    /// 目标位置越界（已在首/尾）时是 no-op，由调用方控制按钮显隐即可。
    pub fn move_column(&mut self, id: u64, up: bool, cx: &mut Context<Self>) {
        let index = match self.draft.columns.iter().position(|c| c.id == id) {
            Some(index) => index,
            None => return,
        };
        let target = if up {
            // 首列不能再上移：no-op（不要 `index - 1` 下溢）
            if index == 0 {
                return;
            }
            index - 1
        } else {
            index + 1
        };
        if target >= self.draft.columns.len() {
            return;
        }
        self.draft.columns.swap(index, target);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 复制列（深拷贝配置，新 id，名字加「副本」后缀，插在原列后面）。
    /// 用于快速造相似字段：复制后再微调参数，比重新加列 + 重配快。
    pub fn duplicate_column(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.draft.columns.iter().position(|c| c.id == id) else {
            return;
        };
        let mut copy = self.draft.columns[index].clone();
        copy.id = self.next_id;
        self.next_id += 1;
        copy.def.name = format!("{} 副本", copy.def.name);
        self.draft.columns.insert(index + 1, copy);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 直接换生成器（取规格默认参数）——详情 tab 的生成器菜单。
    pub fn set_generator(&mut self, id: u64, name: &str, cx: &mut Context<Self>) {
        let Some(config) = generator_catalog::default_of(name) else {
            return;
        };
        self.remember_generator(name);
        if let Some(column) = self.draft.columns.iter_mut().find(|c| c.id == id) {
            column.def.generator = config;
            column.confidence = "manual".to_string();
            column.sample_value.clear();
            self.results.clear();
            self.current = 0;
            self.landed = None;
        }
        cx.notify();
    }

    /// 本会话点过的生成器（最近在前；生成器菜单的「最近使用」，见 B7）。
    pub fn recent_generators(&self) -> &[String] {
        &self.recent_generators
    }

    /// 某列的**推荐生成器**（目录名）——生成器菜单里标「推荐」（B7）。
    ///
    /// 与导入结构走同一条推理（`ColumnMapper::infer`：列名 + 类型 → 生成器），
    /// 但**只用来标记菜单项**，不写回配置：写回仍然只有用户点击（或导入那条路径）。
    pub fn recommended_generator(&self, name: &str, data_type: &ColumnDataType) -> String {
        generator_catalog::spec_of(&ColumnMapper::infer(name, data_type).generator)
            .name
            .to_string()
    }

    /// 记一笔「刚用过哪个生成器」（去重后放最前，只留 [`RECENT_GENERATORS`] 条）。
    fn remember_generator(&mut self, name: &str) {
        self.recent_generators.retain(|recent| recent != name);
        self.recent_generators.insert(0, name.to_string());
        self.recent_generators.truncate(RECENT_GENERATORS);
    }

    /// 打开「搜索生成器」对话框（143 项按名称 / 中文标签 / 分类过滤）。
    ///
    /// 用 `List` 组件而不是手搜：搜索框 / 虚拟化 / 上下键 / 回车与点击确认 / 空态全是组件的，
    /// 且搜索是**同步**的（目录全在内存），不会闪 loading。
    pub fn open_generator_search(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        // 当前生成器从自身状态取：此刻面板正在被更新，不能再 `read` 自己
        let current = self
            .draft
            .columns
            .iter()
            .find(|column| column.id == id)
            .map(|column| generator_catalog::spec_of(&column.def.generator).name);
        let delegate = GeneratorSearchDelegate::new(cx.entity(), id, current);
        // `searchable` 在 `ListState` 上（搜索框是状态的一部分），占位文案在元素上
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title("搜索生成器")
                .child(div().w_full().h(rems(ui::SEARCH_LIST_HEIGHT)).child(
                    List::new(&list).search_placeholder("按名称 / 中文标签 / 分类搜索（143 项）"),
                ))
        });
    }

    /// 应用列编辑（列名 / 类型 / 生成器 / 参数 / 空值率 / 唯一）——详情 tab 的「应用」。
    pub fn apply_column(&mut self, edited: MockColumnSpec, cx: &mut Context<Self>) {
        if let Some(slot) = self.draft.columns.iter_mut().find(|c| c.id == edited.id) {
            *slot = edited;
            self.results.clear();
            self.current = 0;
            self.landed = None;
        }
        cx.notify();
    }

    /// 按列名 + 类型重跑智能映射（「恢复智能默认」）。
    pub fn reset_column_mapping(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(slot) = self.draft.columns.iter_mut().find(|c| c.id == id) {
            let mapped = ColumnMapper::infer(&slot.def.name, &slot.def.data_type);
            slot.def.generator = mapped.generator;
            slot.def.nullable_ratio = 0.0;
            slot.def.unique = false;
            slot.confidence = mapped.confidence;
            slot.sample_value = mapped.sample_value;
            self.results.clear();
            self.current = 0;
            self.landed = None;
        }
        cx.notify();
    }

    /// 从输入框同步目标表名 / 行数 / 种子（生成与出口前调用）。
    fn sync_inputs(&mut self, cx: &App) -> Result<(), String> {
        if let Some(input) = self.table_input.as_ref() {
            self.draft.table_name = validate_table_name(&input.read(cx).value())?;
        } else {
            self.draft.table_name = validate_table_name(&self.draft.table_name)?;
        }
        if let Some(input) = self.rows_input.as_ref() {
            self.draft.options.rows = parse_rows(&input.read(cx).value())?;
        }
        if let Some(input) = self.seed_input.as_ref() {
            self.draft.options.seed = parse_seed(&input.read(cx).value())?;
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

    /// 「另存为」的默认文件名：取**当前结果表**的名字（场景模板下与草稿无关）。
    ///
    /// 没结果时回退到草稿的目标表名：按钮在无结果时本来就点不开（`has_result` 拦着），
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
        if let Some(input) = self.rows_input.clone() {
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
    fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.table_input.is_none() {
            let initial = self.draft.table_name.clone();
            let state = cx.new(|cx| InputState::new(window, cx).placeholder("目标表名"));
            state.update(cx, |s, cx| s.set_value(initial, window, cx));
            self.table_input = Some(state);
        }
        if let (Some(pending), Some(input)) = (self.table_pending.take(), self.table_input.clone())
        {
            input.update(cx, |s, cx| s.set_value(pending, window, cx));
        }
        if self.rows_input.is_none() {
            let initial = self.draft.options.rows.to_string();
            let state = cx.new(|cx| InputState::new(window, cx).placeholder("行数"));
            state.update(cx, |s, cx| s.set_value(initial, window, cx));
            self.rows_input = Some(state);
        }
        if self.seed_input.is_none() {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder("随机种子"));
            self.seed_input = Some(state);
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
        if self.column_filter.is_none() {
            let state =
                cx.new(|cx| InputState::new(window, cx).placeholder("筛选列（列名 / 生成器）"));
            // 搜索词一变就重画：输入框自己会重绘，但列卡片是面板画的
            self.column_filter_sub =
                Some(cx.subscribe(&state, |_panel, _input, ev: &InputEvent, cx| {
                    if matches!(ev, InputEvent::Change) {
                        cx.notify();
                    }
                }));
            self.column_filter = Some(state);
        }
        // 清单搜索框（工具栏下方 / 清单顶部；搜索词一变就重画，与 `column_filter` 同一订阅模式）
        if self.search_input.is_none() {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder("搜索表名"));
            self.search_input = Some(state);
        }
    }
}
