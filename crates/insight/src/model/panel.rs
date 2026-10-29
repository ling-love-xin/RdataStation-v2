//! 洞察模型 · 面板内容状态与数据（自 model.rs 拆分，纯位移）

use super::*;
/// 面板内容状态（四态：空 / 加载 / 错误 / 数据）
///
/// `Data` 不带载荷：**载荷在 [`PanelData`] 里按 Tab 分开存**。
/// 两者分开的理由：Tab 条是「同一个目标的多个视角」，切 Tab 不该把别的视角取到的
/// 数据丢掉（单格子设计下，切到「表」再切回「多列」就会丢表单与结果）。
#[derive(Debug, Clone, PartialEq)]
pub enum InsightPanelState {
    /// 无目标，或目标当前 Tab 尚无内容（后者由视图补期次提示）
    Empty,
    /// 计算中（视图渲染骨架）
    Loading,
    Error {
        message: String,
        /// 是否值得让用户重试（临时表失效 / 连接断开可重试；语法类错误不可）
        retryable: bool,
    },
    /// 至少有一份可取的内容（具体是哪个 Tab 的，看 [`PanelData`]）
    Data,
}

/// 数据态里装的是什么：按 Tab 分开存（各 Tab 的取数时机不同，互不覆盖）。
///
/// 四种目标（列 / 表 / 多列 / 结构）各有自己的视图模型；`None` = 这个 Tab 还没取过数。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PanelData {
    pub column: Option<ColumnProfileView>,
    pub table: Option<TableProfileView>,
    pub multi: Option<MultiColumnView>,
    pub schema: Option<crate::schema_view::SchemaReportView>,
    pub history: Option<HistoryView>,
    /// 取样来源解析出的**分析临时表名**（源目标才有；接缝侧取样后回填）。
    ///
    /// 面板自己要「接着分析哪张临时表」（保存快照 / 多列 / 下钻）时读它——
    /// 源目标的 `InsightTarget` 里没有临时表名（那是取样之后才知道的事）。
    pub source_sample: Option<String>,
}

impl PanelData {
    /// 当前该分析哪张临时表：源目标读解析出的样本表，其余目标读目标自带的。
    pub fn sample_table<'a>(&'a self, target: &'a InsightTarget) -> Option<&'a str> {
        match target.source() {
            Some(_) => self.source_sample.as_deref(),
            None => {
                let table = target.temp_table();
                (!table.is_empty()).then_some(table)
            }
        }
    }
    pub fn as_column(&self) -> Option<&ColumnProfileView> {
        self.column.as_ref()
    }

    pub fn as_table(&self) -> Option<&TableProfileView> {
        self.table.as_ref()
    }

    pub fn as_multi(&self) -> Option<&MultiColumnView> {
        self.multi.as_ref()
    }

    pub fn as_schema(&self) -> Option<&crate::schema_view::SchemaReportView> {
        self.schema.as_ref()
    }

    pub fn as_history(&self) -> Option<&HistoryView> {
        self.history.as_ref()
    }

    /// 三个 Tab 都还没取过数
    pub fn is_empty(&self) -> bool {
        self.column.is_none()
            && self.table.is_none()
            && self.multi.is_none()
            && self.schema.is_none()
    }

    // 写入用链式构造：新数据只换它自己那一格，别的 Tab 的载荷不动
    pub fn with_column(mut self, profile: ColumnProfileView) -> Self {
        self.column = Some(profile);
        self
    }

    pub fn with_table(mut self, profile: TableProfileView) -> Self {
        self.table = Some(profile);
        self
    }

    pub fn with_multi(mut self, view: MultiColumnView) -> Self {
        self.multi = Some(view);
        self
    }

    pub fn with_schema(mut self, view: crate::schema_view::SchemaReportView) -> Self {
        self.schema = Some(view);
        self
    }

    pub fn with_history(mut self, view: HistoryView) -> Self {
        self.history = Some(view);
        self
    }
}

impl InsightPanelState {
    /// 正在计算
    pub fn is_loading(&self) -> bool {
        matches!(self, InsightPanelState::Loading)
    }

    /// 出错（含不可重试）
    pub fn is_error(&self) -> bool {
        matches!(self, InsightPanelState::Error { .. })
    }

    /// 已出数
    pub fn is_data(&self) -> bool {
        matches!(self, InsightPanelState::Data)
    }
}