//! 结果面板 ↔ 编辑器宿主的动作端口（Shared 事件队列，彻底解耦）
//!
//! 结果面板（[`crate::panel::ResultPanel`]）渲染期只做纯读 + 入队：工具栏 / 标签条 /
//! 卡片上的按钮一律 [`ResultActionQueue::push`]（不直接调宿主方法）；编辑器宿主在
//! 事件路径 `drain` 后按变体分派到既有动作方法（执行 / 提交 / 导出 / 分析 / 绑定等
//! 仍归编辑器，它们依赖执行链与 engine 端口）。
//!
//! 为什么不用注入闭包：结果面板要在不反向依赖 editor 的前提下自洽（lib.rs 的依赖
//! 方向硬约束 `editor → result_panel`），闭包会把宿主方法名焊进面板渲染；队列把
//! 「动作语义」（枚举）与「动作实现」（宿主）分开，面板可独立复用与测试。
//!
//! 网格内部交互（右键菜单 / 行编辑 / 列统计 / 值预览 / 加载更多）**不走队列**——
//! [`crate::grid::ResultGridDelegate`] 已有跨 crate 安全的注入钩子
//! （`Rc<dyn Fn(..., &mut App)>`），那套机制保持原样，不重复发明。

use std::cell::RefCell;
use std::rc::Rc;

use crate::export::ExportFormat;

/// 结果面板发出的面板级动作请求（编辑器宿主 drain 后分派）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultRequest {
    /// 结果集标签条切选中（下标 = 标签顺序）
    SelectSet(usize),
    /// 对比栏标签条切选中（下标 = 标签顺序；点当前选中 = 取消对比）
    SelectCompare(usize),
    /// 工具栏「并排对比」开关（开 / 关）
    ToggleCompareMode,
    /// 工具栏「转置视图」开关（开 / 关）
    ToggleTranspose,
    /// 转置视图「上一行」导航
    TransposePrev,
    /// 转置视图「下一行」导航
    TransposeNext,
    /// 工具栏筛选框「✕」清除（清空输入框 + 立即应用空词；`window` 由宿主分派时提供）
    ClearFilter,
    /// 下发源库开关翻转后的重查（开着时应用筛选会重查数据库）
    PushdownFilter { value: String },
    /// 批量提交待提交变更
    CommitPending,
    /// 批量撤销待提交变更
    RollbackPending,
    /// 分析菜单项（对已抓行跑聚合 SQL）
    Analysis { sql: String },
    /// 分析菜单「自定义」项（弹输入框，模板预填）
    AnalysisCustom,
    /// 复制当前结果（TSV）
    CopyTsv,
    /// 刷新（重跑当前结果 SQL）
    Refresh,
    /// 取下一段
    FetchMore,
    /// 导出（格式 × 范围）
    Export { format: ExportFormat, all: bool },
    /// 错误卡片「复制错误」
    CopyError,
    /// 错误卡片「定位到行」
    LocateError,
    /// 下发源库开关（只翻转状态，不重查——与 [`Self::PushdownFilter`] 区别开）
    SetPushdown { open: bool },
    /// 工具栏「辅助栏」开关：结果面板右侧辅助栏（列浏览器 / 值详情 / 计算 / 分组）开合
    ToggleValueSidebar,
    /// 左栏列浏览器清单行眼睛：切换该列隐藏 / 显示（宿主 update 网格 + 面板）
    ToggleColumnVisibility(usize),
    /// 列浏览器清单单击选中一列（数据列序）：写清单选中态 + 网格列高亮联动
    SelectColumn(usize),
    /// 列浏览器清单双击定位：网格滚到该列首行并选中
    LocateColumn(usize),
    /// 列浏览器搜索框「✕」清除（清空输入框 + 搜索词；`window` 由宿主分派时提供）
    ClearColumnSearch,
    /// 呈现方式切换（DBeaver 左栏「显示方式」：网格 / 文本）。显式目标值——
    /// 网格 / 文本两个入口（左栏按钮组、工具栏按钮）都要能精确切到目标，
    /// 翻转语义在「已在该呈现」时会把用户点反。
    SetPresentation(Presentation),
    /// 右栏标签页切换（列浏览器 / 值详情 / 计算 / 分组；对齐 DBeaver 右侧 metadata /
    /// valueviewer / aggregate / grouping 同栏页签体系）
    SetRightTab(RightTab),
    /// 【#28-B3】在指定视图行下方插入一条本地空行（`None` = 追加到末尾区段）。
    /// 宿主经网格 `RowRef` 翻译身份后落到 `LocalRowSet`；只改叠加层，不下发 INSERT。
    InsertRowBelow(Option<usize>),
    /// 【#28-B3】复制指定视图行（视图行号）为一条本地行，携带源行展示值快照。
    DuplicateRow(usize),
    /// 【#28-B3】删除指定视图行：底层行只打删除标记（可撤销恢复）；本地行直接移出叠加层。
    DeleteRow(usize),
    /// 【#28-B3】撤销（组合语义：格级 `EditHistory` 非空先走格级，栈空再走行级 `RowHistory`）
    UndoAction,
    /// 【#28-B3】重做（组合语义与 [`Self::UndoAction`] 对称）
    RedoAction,
    /// 【C1】重命名第 `index` 份结果集标签。
    /// `Some(name)` = 覆盖自动标签（空白串宿主侧归一为恢复 `结果 N`）；`None` = 恢复自动标签。
    /// 会话级，不落盘。
    RenameSet(usize, Option<String>),
    /// 【C1】切换第 `index` 份结果集的钉住态（钉住不被 FIFO 淘汰；会话级）。
    TogglePin(usize),
    /// 【C1】进入第 `index` 份结果集的重命名态（面板弹输入框；输入确认后再发
    /// [`Self::RenameSet`]）。单独走一条事件路径是因为 render 闭包拿不到 `&mut App`，
    /// 不能在 render 里直接改面板状态。
    BeginRename(usize),
    /// 【C1】提交重命名（读面板输入框当前值，发 RenameSet 回 store；空串恢复序号）
    CommitRename,
    /// 【C1】取消重命名（不发请求，面板退出 renaming 态）
    CancelRename,
    /// 【B1】面板级临时开关：列头直接显示注释（覆盖全局偏好）
    SetShowColumnComments { open: bool },
}

/// 结果集呈现方式（DBeaver Data Editor 的显示方式：网格 / 文本）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Presentation {
    /// 默认网格（gpui-kit DataTable）
    #[default]
    Grid,
    /// 文本视图：当前结果（可见列 + 可见行）渲染为 TSV 纯文本，可选中复制
    Text,
}

/// 结果面板右侧辅助栏的标签页（对齐 DBeaver `resultset/panel/` 右侧面板组：
/// metadata / valueviewer / aggregate / grouping 同栏页签切换；「引用」页留 backlog，
/// 见 result-panel.md §8.12）。顺序：列浏览器 → 值详情 → 计算 → 分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightTab {
    /// 列浏览器：搜索框 + 列清单 + 底部元数据明细（默认；对齐 DBeaver metadata 面板）
    #[default]
    Browser,
    /// 数值查看器：选中格完整值
    Value,
    /// 计算：选中列的 count / null / min / max / avg / sum（纯函数，不引 polars）
    Stats,
    /// 分组：按一列本地 groupby 计数（纯函数；DBeaver 大表口径是下发 GROUP BY）
    Group,
}

/// 结果动作队列（`Rc<RefCell<Vec<_>>>` 共享句柄）
///
/// 编辑器宿主创建（一次），同一份句柄给两块：
/// - 结果面板（渲染闭包 clone 捕获，按钮 `push`）
/// - 宿主自身（事件路径 `drain` 分派）
#[derive(Clone, Debug, Default)]
pub struct ResultActionQueue {
    inner: Rc<RefCell<Vec<ResultRequest>>>,
}

impl ResultActionQueue {
    /// 新建空队列
    pub fn new() -> Self {
        Self::default()
    }

    /// 入队一条动作（渲染期调用；纯写，无副作用）
    pub fn push(&self, request: ResultRequest) {
        self.inner.borrow_mut().push(request);
    }

    /// 取走全部在途动作（事件路径消费；取空后队列恢复空）
    pub fn drain(&self) -> Vec<ResultRequest> {
        std::mem::take(&mut *self.inner.borrow_mut())
    }

    /// 是否为空（渲染期判「有没有在途请求」用不到；主要给测试断言）
    pub fn is_empty(&self) -> bool {
        self.inner.borrow().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::ExportFormat;

    /// 队列先入先出：push 顺序 = drain 顺序
    #[gpui_kit::test]
    fn queue_preserves_push_order(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let queue = ResultActionQueue::new();
        assert!(queue.is_empty());
        queue.push(ResultRequest::FetchMore);
        queue.push(ResultRequest::ToggleTranspose);
        queue.push(ResultRequest::Export {
            format: ExportFormat::Csv,
            all: true,
        });
        assert!(!queue.is_empty());
        let drained = queue.drain();
        assert_eq!(
            drained,
            vec![
                ResultRequest::FetchMore,
                ResultRequest::ToggleTranspose,
                ResultRequest::Export {
                    format: ExportFormat::Csv,
                    all: true,
                },
            ]
        );
        // drain 后队列清空
        assert!(queue.is_empty());
        assert!(queue.drain().is_empty());
    }

    /// 多次 drain 之间可继续 push（宿主分派一批后，新动作能进下一批）
    #[gpui_kit::test]
    fn queue_accepts_push_after_drain(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let queue = ResultActionQueue::new();
        queue.push(ResultRequest::CopyTsv);
        let first = queue.drain();
        assert_eq!(first, vec![ResultRequest::CopyTsv]);
        queue.push(ResultRequest::Refresh);
        let second = queue.drain();
        assert_eq!(second, vec![ResultRequest::Refresh]);
    }
}
