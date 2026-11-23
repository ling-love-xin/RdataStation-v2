//! 来源选择器（宿主侧通用内核）：从三分法三根里选一个数据目标。
//!
//! 洞察 / 联邦 / 结果集二次分析共用同一套「来源浏览 + 单选」，差异只在
//! **物化策略**（选中后如何进 DuckDB：洞察→取样来源、联邦→ATTACH/本地加速、
//! 二次分析→联邦 + 结果集直连），由调用方注入确认回调。
//!
//! ## 形态（`prototypes/entry/01-source-picker.html` + `06-object-tree.html`）
//!
//! tab 不是五个叶子，而是**三个根**（`group::SourceGroup`）：
//! - **数据库**：驱动 + 连接，SQL 下推；连接在，表就在；
//! - **DuckDB**：本地 SQL，装临时产物（结果集 / Mock / 插件 / 会话表）；
//! - **文件**：reader 函数读本机持久物（草稿箱 / 资产库 / 外部文件）。
//!
//! 叶子枚举（5 个）保持不动——分组是视图层的纯函数运算（`group::grouped`），
//! `PickerConfig.kinds` / `confirm_prefix` 都不因此改动。
//!
//! ## 选中模型
//!
//! 树里选的是**节点**，确认时要的是「哪一类目标 + 定位」，所以状态只留一个
//! `picked: Option<Picked>`。这样 `current_tab()` 由**选中的东西**决定，而不是由
//! 「当前停在哪一屏」决定——用户选了草稿再切到数据库 tab，确认按钮仍然说的是草稿
//! （老实现按 tab 分派，这里会答错）。
//!
//! ## 文件地图
//!
//! | 文件 | 职责 |
//! | --- | --- |
//! | `mod.rs` | 对外契约：`SourceKind` / `Picked` / `PickerState` / `PickerConfig` |
//! | `dialog.rs` | 模态外壳：根 tab、三分法主体、底部栏 |
//! | `load.rs` | 全部 I/O：内省、草稿枚举、资产库表、按层懒加载 |
//! | `db.rs` / `rows.rs` | 数据 → 行（深度预算、折叠规则） |
//! | `render.rs` | 行 → 元素（三个根共用） |
//! | `node.rs` / `state.rs` / `search.rs` / `group.rs` | 身份、树态、搜索档位、分组 |
//!
//! 归属：留在 workbench（三方入口全在宿主侧）。未来出现第二个非 workbench 直接消费者
//! （独立应用 / editor 就地内嵌）时，整体搬成独立 crate 即可（纯搬文件）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{App, Window};

use workbench_shell::model::ConnectionItem;

use crate::panels::Shared;

mod db;
mod dialog;
mod group;
mod load;
mod node;
mod render;
mod rows;
mod search;
mod state;

pub use db::{catalog_level, db_rows, schema_rows, table_rows, CatalogLevel, ConnBrief, DbCache};
pub use dialog::open_source_picker;
pub use group::{group_of, grouped, SourceGroup};
pub use node::{ArchiveScope, NodeId, NodeKind, Row, TempScope, PICKER_MAX_DEPTH};
pub use render::{render_rows, RowAction};
pub use rows::{
    analysis_table_rows, draft_rows, external_row, insight_sample_rows, is_persistent_table,
    persistent_table_label, persistent_table_rows, result_set_rows, root_rows,
    split_persistent_tables,
};
/// 取数（面板与对话框共用同一份 I/O，见 `components::data_objects`）
pub(crate) use load::{analysis_tables_of, list_connection_tables_in};
pub use search::{rank, search, HitRank, SearchOutcome, SEARCH_LIMIT};
pub use state::{TreeState, RECENT_CAP};

/// 来源种类（6 个叶子；分组见 `group::group_of`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// ① 已连接数据库的表
    Connected,
    /// ② 草稿箱里 duckdb 支持的文件
    Scratchpad,
    /// ③ 资产库（原「资源分析」）——`{项目}/.RSmeta/analytics.duckdb` 的内部表
    Analysis,
    /// ③-b 全局分析表——`{system}/analytics.duckdb` 里的表（只读；跨项目共用）
    ///
    /// 为什么不并入 `Analysis`：两段的**写入面**不同（全局是单写者、项目是本项目副本），
    /// 树上必须分两行，用户得一眼看出自己动的是哪一份（`ArchiveScope`）。
    AnalysisGlobal,
    /// ④ 外部文件（rfd 系统对话框）
    External,
    /// ⑤ 结果集临时表（仅结果集二次分析启用；洞察 / 联邦裁剪掉，当前未实装）
    ResultSet,
    /// ⑥ 洞察产物（`tmp_i_`，进程单例）：二次分析的「再分析」档
    InsightSample,
}

impl SourceKind {
    /// 叶子文案（调试与日志用；tab 上是根标题，不是这个）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "已连接数据库",
            Self::Scratchpad => "草稿箱",
            Self::Analysis => "资产库",
            Self::AnalysisGlobal => "全局分析表",
            Self::External => "外部文件",
            Self::ResultSet => "结果集",
            Self::InsightSample => "洞察产物",
        }
    }
}

/// 表候选（带归属定位，确认时拼限定名）。
#[derive(Debug, Clone, PartialEq)]
pub struct TableCandidate {
    pub name: String,
    pub catalog: String,
    /// 空串 = 该驱动无独立 schema 层（对齐导航侧口径）
    pub schema: String,
}

/// 选中的目标。树里选的是节点，这里落成「哪一类 + 定位」。
#[derive(Debug, Clone, PartialEq)]
pub enum Picked {
    /// 库表 `(连接 id, 表候选)`
    Db(String, TableCandidate),
    /// 草稿文件 `(相对路径, 绝对路径)`
    Draft(String, PathBuf),
    /// 资产库分析表（当前只接本项目副本）
    Analysis(String),
    /// 全局分析表（`{system}/analytics.duckdb`，只读；取样走全局会话）
    AnalysisGlobal(String),
    /// 外部文件绝对路径
    External(PathBuf),
    /// 结果集临时表（`tmp_q_`，本机单例）：二次分析的入口
    ResultSet(String),
    /// 洞察产物（`tmp_i_`，本机单例）：再分析的入口（样本表不是终点）
    InsightSample(String),
}

impl Picked {
    /// 这个目标属于哪个叶子（`current_tab()` 的真值来源）。
    pub fn kind(&self) -> SourceKind {
        match self {
            Self::Db(..) => SourceKind::Connected,
            Self::Draft(..) => SourceKind::Scratchpad,
            Self::Analysis(..) => SourceKind::Analysis,
            Self::AnalysisGlobal(..) => SourceKind::AnalysisGlobal,
            Self::External(..) => SourceKind::External,
            Self::ResultSet(..) => SourceKind::ResultSet,
            Self::InsightSample(..) => SourceKind::InsightSample,
        }
    }

    /// 确认按钮上的一句话描述。
    pub fn label(&self) -> String {
        match self {
            Self::Db(_, t) if t.schema.is_empty() => t.name.clone(),
            Self::Db(_, t) => format!("{} · {}", t.schema, t.name),
            Self::Draft(rel, _) => rel.clone(),
            Self::Analysis(t) => t.clone(),
            // 全局表加前缀：按钮上必须能看出「这是全局那份」
            Self::AnalysisGlobal(t) => format!("全局 · {t}"),
            Self::ResultSet(t) => t.clone(),
            Self::InsightSample(t) => t.clone(),
            Self::External(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string()),
        }
    }
}

/// 来源选择状态（模态渲染闭包每次重建 UI，状态持久在这里）。
pub struct PickerState {
    /// 当前激活的根（tab）
    group: SourceGroup,
    /// 本次启用的叶子（来自 `PickerConfig.kinds`）
    kinds: &'static [SourceKind],
    // ① 数据库（懒加载树：每个分支各自展开，缓存按 NodeId 记）
    conns: Vec<ConnectionItem>,
    /// 各层已加载的结果（空结果也记，见 `db.rs` 模块文档末段）
    db_cache: DbCache,
    // ② 草稿箱
    drafts: Vec<(String, PathBuf)>, // (相对路径, 绝对路径)
    // ③ 资产库
    analysis_tables: Vec<String>,
    /// ③-b 全局分析表（`{system}/analytics.duckdb`；只读，跨项目共用）
    global_tables: Vec<String>,
    // ⑤ 结果集（进程单例里的 `tmp_q_` 表）
    result_sets: Vec<String>,
    /// ⑥ 洞察产物（进程单例里的 `tmp_i_` 表）
    insight_samples: Vec<String>,
    /// 本次启用多选（`PickerConfig.multi`）：为真时每点一次是「加/减一个来源」，
    /// 为假时每点一次整体替换（单选，绝大多数入口的现状）。
    multi: bool,
    /// 选中的目标清单（**唯一真值**）：`(所在行 id, 目标)`。
    ///
    /// 存行 id 是为了让渲染能标出「已选」（树自身只有单选态）；删掉某一项时
    /// 行与目标一起消失，不会出现「选中态还在、确认时却拿不到」的错位。
    picked: Vec<(NodeId, Picked)>,
    /// 底部提示（加载失败 / 源不可用 / 物化失败回显）
    note: Option<String>,
    /// 树态：展开 / 搜索词 / 选中 / 最近使用（纯内存，随对话框关闭丢弃，见 `state.rs`）
    tree: TreeState,
}

impl PickerState {
    fn new(
        kinds: &'static [SourceKind],
        multi: bool,
        conns: Vec<ConnectionItem>,
        drafts: Vec<(String, PathBuf)>,
        analysis: Vec<String>,
        global_tables: Vec<String>,
        result_sets: Vec<String>,
        insight_samples: Vec<String>,
    ) -> Self {
        let group = grouped(kinds)
            .first()
            .map(|(g, _)| *g)
            .unwrap_or(SourceGroup::Database);
        Self {
            group,
            kinds,
            multi,
            conns,
            db_cache: DbCache::new(),
            drafts,
            analysis_tables: analysis,
            global_tables,
            result_sets,
            insight_samples,
            picked: Vec::new(),
            note: None,
            tree: TreeState::new(),
        }
    }

    /// 点选一个目标（单选替换 / 多选加减由 `self.multi` 决定）。
    ///
    /// 行 id 与目标一起入表：渲染据此标「已选」，删项时两者同时消失。
    pub fn toggle(&mut self, id: NodeId, picked: Picked) {
        if !self.multi {
            self.picked.clear();
            self.picked.push((id.clone(), picked));
            self.tree.select(Some(id));
            return;
        }
        if let Some(pos) = self.picked.iter().position(|(cur, _)| cur == &id) {
            self.picked.remove(pos);
        } else {
            self.picked.push((id.clone(), picked));
        }
        // 树自身只有单选态：多选时把最后点的行点亮即可，不做多行高亮（见上注释）
        self.tree.select(Some(id));
    }

    /// 已选行 id（渲染标「已选」用）。
    pub fn picked_ids(&self) -> Vec<NodeId> {
        self.picked.iter().map(|(id, _)| id.clone()).collect()
    }

    /// 本次是否多选。
    pub fn is_multi(&self) -> bool {
        self.multi
    }

    /// 当前来源是否已选好目标（确认按钮可用性）。
    pub fn has_target(&self) -> bool {
        !self.picked.is_empty()
    }

    /// 当前选中目标的一句话描述（确认按钮文案用）。
    ///
    /// 多选下按钮必须能看出「要一次分析几个来源」，否则用户只会看到第一个名字，
    /// 以为后面的没选上。
    pub fn selected_label(&self) -> Option<String> {
        match self.picked.len() {
            0 => None,
            1 => self.picked.first().map(|(_, p)| p.label()),
            n => Some(format!("{n} 个来源")),
        }
    }

    /// 写底部提示（供物化策略失败时回显）。
    pub fn set_note(&mut self, note: Option<String>) {
        self.note = note;
    }

    // —— 选中目标访问器（物化策略确认时读取；返回克隆避免借用逃逸）——

    /// 当前来源（物化策略按来源分派出口）。
    ///
    /// 由**选中的目标**决定，不由激活的 tab 决定：选了草稿再切到数据库 tab，
    /// 确认时仍然按草稿导出（否则会拿错出口）。
    pub fn current_tab(&self) -> SourceKind {
        self.picked
            .first()
            .map(|(_, p)| p.kind())
            .or_else(|| {
                grouped(self.kinds)
                    .first()
                    .and_then(|(_, m)| m.first().copied())
            })
            .unwrap_or(SourceKind::Connected)
    }

    /// 已选目标的克隆清单（多选消费方按顺序逐个出口）。
    pub fn picked_all(&self) -> Vec<Picked> {
        self.picked.iter().map(|(_, p)| p.clone()).collect()
    }

    /// 某个变体的第一个命中（各 `picked_*` 访问器的共同实现）。
    fn find_map<T>(&self, f: impl Fn(&Picked) -> Option<T>) -> Option<T> {
        self.picked.iter().find_map(|(_, p)| f(p))
    }

    /// ① 已连接库选中的表（`(连接 id, 表候选)`）。
    pub fn picked_table(&self) -> Option<(String, TableCandidate)> {
        self.find_map(|p| match p {
            Picked::Db(conn, table) => Some((conn.clone(), table.clone())),
            _ => None,
        })
    }

    /// ② 草稿箱选中的文件 `(相对路径, 绝对路径)`。
    pub fn picked_draft(&self) -> Option<(String, PathBuf)> {
        self.find_map(|p| match p {
            Picked::Draft(rel, abs) => Some((rel.clone(), abs.clone())),
            _ => None,
        })
    }

    /// ③ 资产库选中的表名。
    pub fn picked_analysis(&self) -> Option<String> {
        self.find_map(|p| match p {
            Picked::Analysis(t) => Some(t.clone()),
            _ => None,
        })
    }

    /// ③-b 全局分析表选中的表名（裸名，SQL 由 `SampleSource::on_global` 拼别名）。
    pub fn picked_analysis_global(&self) -> Option<String> {
        self.find_map(|p| match p {
            Picked::AnalysisGlobal(t) => Some(t.clone()),
            _ => None,
        })
    }

    /// ④ 外部文件选中的绝对路径。
    pub fn picked_external(&self) -> Option<PathBuf> {
        self.find_map(|p| match p {
            Picked::External(p) => Some(p.clone()),
            _ => None,
        })
    }

    /// ⑤ 结果集选中的临时表名（`tmp_q_`）。
    pub fn picked_result_set(&self) -> Option<String> {
        self.find_map(|p| match p {
            Picked::ResultSet(t) => Some(t.clone()),
            _ => None,
        })
    }

    /// ⑥ 洞察产物选中的临时表名（`tmp_i_`）。
    pub fn picked_insight_sample(&self) -> Option<String> {
        self.find_map(|p| match p {
            Picked::InsightSample(t) => Some(t.clone()),
            _ => None,
        })
    }
}

/// 对话框配置：标题 + 启用的叶子（顺序决定根与组内顺序）+ 确认按钮动作前缀。
#[derive(Debug, Clone, Copy)]
pub struct PickerConfig {
    pub title: &'static str,
    pub kinds: &'static [SourceKind],
    /// 确认按钮前缀（洞察="分析"，联邦="联邦"，二次分析="分析"）。
    pub confirm_prefix: &'static str,
    /// 是否允许一次选多个来源（洞察多源 / 联邦）。单选入口填 `false`。
    pub multi: bool,
}

/// 物化策略（消费方注入）：读 `PickerState` 里选中的目标，执行出口动作。
/// 失败时写 `state.set_note` + `window.refresh()` 回显，不关对话框。
pub type ConfirmStrategy = Rc<dyn Fn(&Shared, &Rc<RefCell<PickerState>>, &mut Window, &mut App)>;

#[cfg(test)]
mod tests {
    // 刻意不用 `use super::*`：会把 gpui 的 `test` 属性宏带进作用域，
    // `#[test]` 会解析到它自己（recursion limit reached，见 gpui-kit-dev「窗口测试」）
    use super::{Picked, PickerState, SourceKind, TableCandidate};
    use super::node::{ArchiveScope, NodeId};
    use std::path::PathBuf;

    /// 出口派发必须看「选中的东西」，不是「停在哪一屏」：
    /// 用户选了草稿再切到数据库 tab，确认时仍然按草稿导出。
    #[test]
    fn picked_kind_is_the_dispatch_source() {
        assert_eq!(
            Picked::Draft("数据/a.parquet".into(), PathBuf::from("C:/p/a.parquet")).kind(),
            SourceKind::Scratchpad
        );
        assert_eq!(
            Picked::Analysis("orders".into()).kind(),
            SourceKind::Analysis
        );
        // 全局表必须自成一档：断言「不会被派发成项目资产库」，
        // 否则会拿项目单例的出口去读全局表（读不到，还以为是数据没了）
        assert_eq!(
            Picked::AnalysisGlobal("g_orders".into()).kind(),
            SourceKind::AnalysisGlobal
        );
        assert_eq!(
            Picked::External(PathBuf::from("C:/data/x.csv")).kind(),
            SourceKind::External
        );
        // 洞察产物自成一档：它是「再分析」入口，不是结果集（两者虽同在一台单例里）
        assert_eq!(
            Picked::InsightSample("tmp_i_x".into()).kind(),
            SourceKind::InsightSample
        );
    }

    /// 洞察产物的叶子文案要与结果集区分开（两者都来自同一台内存单例）。
    #[test]
    fn insight_sample_kind_has_its_own_label() {
        assert_eq!(SourceKind::InsightSample.label(), "洞察产物");
        assert_eq!(SourceKind::ResultSet.label(), "结果集");
    }

    /// 单选：每点一次整体替换（老行为，不能因多选改造而变）。
    /// 多选：点同一个行是「取消」，点别的行是「再添一个」。
    #[test]
    fn toggle_is_replace_in_single_mode_and_add_remove_in_multi_mode() {
        let kinds: &'static [SourceKind] = &[SourceKind::Analysis];
        let mut single =
            PickerState::new(kinds, false, vec![], vec![], vec![], vec![], vec![], vec![]);
        single.toggle(
            NodeId::AnalysisTable {
                scope: ArchiveScope::Project,
                table: "a".into(),
            },
            Picked::Analysis("a".into()),
        );
        single.toggle(NodeId::Path("b".into()), Picked::Analysis("b".into()));
        assert_eq!(single.picked_all(), vec![Picked::Analysis("b".into())]);
        assert_eq!(single.selected_label().as_deref(), Some("b"));

        let mut multi = PickerState::new(kinds, true, vec![], vec![], vec![], vec![], vec![], vec![]);
        let a = NodeId::Path("a".into());
        let b = NodeId::Path("b".into());
        multi.toggle(a.clone(), Picked::Analysis("a".into()));
        multi.toggle(b, Picked::Analysis("b".into()));
        assert_eq!(multi.picked_all().len(), 2);
        // 多选下按钮文案必须报数量，否则只看得到第一个名字
        assert_eq!(multi.selected_label().as_deref(), Some("2 个来源"));
        // 再点一次 a = 取消它，只留 b
        multi.toggle(a, Picked::Analysis("a".into()));
        assert_eq!(multi.picked_all(), vec![Picked::Analysis("b".into())]);
    }

    /// 同一张表在两份分析库里时，按钮文案要让用户看出是哪一份。
    #[test]
    fn global_label_is_distinguishable_from_the_project_copy() {
        assert_eq!(Picked::Analysis("orders".into()).label(), "orders");
        assert_eq!(
            Picked::AnalysisGlobal("orders".into()).label(),
            "全局 · orders"
        );
        assert_eq!(SourceKind::AnalysisGlobal.label(), "全局分析表");
    }

    /// 确认按钮文案：有 schema 时折进标签，与树上的表行标签一致
    /// （同一张表在树里和按钮上不能长得不一样）。
    #[test]
    fn db_label_matches_the_row_label() {
        let table = TableCandidate {
            name: "orders".into(),
            catalog: "analytics".into(),
            schema: "sales".into(),
        };
        assert_eq!(
            Picked::Db("c1".into(), table.clone()).label(),
            "sales · orders"
        );

        let no_schema = TableCandidate {
            name: "orders".into(),
            catalog: "shop".into(),
            schema: String::new(),
        };
        assert_eq!(Picked::Db("c1".into(), no_schema).label(), "orders");
    }

    /// 资产库正名（裁决 8）：叶子文案不再叫「资源分析」。
    #[test]
    fn analysis_kind_is_renamed_to_archive() {
        assert_eq!(SourceKind::Analysis.label(), "资产库");
    }
}
