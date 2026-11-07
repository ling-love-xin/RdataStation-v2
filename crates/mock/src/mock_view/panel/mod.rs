// ==================== 配置面板（右 Dock） ====================

// 继承父模块（mock_view）的全部 use 绑定
use super::*;
use crate::history as core_history;

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
// 历史/模板子模块（impl MockPanel 的历史记录 / 模板保存与应用方法）
mod history;
// 出口/辅助子模块（impl MockPanel 的持久化/导出/状态提示等辅助方法）
mod sinks;
// 预览/取样子模块（impl MockPanel 的预览/排序/取样刷新方法）
mod preview;
// 动作子模块（impl MockPanel 的生成/详情/追加等动作方法）
mod actions;
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
    Listed(core_history::HistorySnapshot),
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


    /// 项目已切换：作废与旧项目绑定的生成结果与出口反馈（`cleared` = 宿主刚清掉的临时表数）。
    ///
    /// 为什么必须作废：宿主在切项目时删掉了本进程的 mock 临时表（内存库是进程级单例，
    /// 不切项目就会一直在），`gen_info` 里的临时表名已经指向不存在的表——留着它，
    /// 用户下一次点「导出 / 落库」只会拿到一个难懂的「表不存在」。
    ///


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
