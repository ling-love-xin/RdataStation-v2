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
    /// 工具栏「列结构」开关：结果面板左栏（列名 / 类型 / 隐藏切换）开合
    ToggleColumnSidebar,
    /// 工具栏「值详情」开关：结果面板右栏（选中格完整值 + 元数据）开合
    ToggleValueSidebar,
    /// 左栏列结构列表的「单击一行」：切换该列隐藏 / 显示（宿主 update 网格 + 面板）
    ToggleColumnVisibility(usize),
    /// 呈现方式切换（DBeaver 左栏「显示方式」：网格 / 文本）。显式目标值——
    /// 网格 / 文本两个入口（左栏按钮组、工具栏按钮）都要能精确切到目标，
    /// 翻转语义在「已在该呈现」时会把用户点反。
    SetPresentation(Presentation),
    /// 右栏标签页切换（值详情 / 列统计 / 分组 / 元数据）
    SetRightTab(RightTab),
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

/// 结果面板右栏的标签页（DBeaver Data Editor 右侧辅助区）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightTab {
    /// 数值查看器：选中格完整值（默认）
    #[default]
    Value,
    /// 计算：选中列的 count / null / min / max / avg / sum（纯函数，不引 polars）
    Stats,
    /// 分组：按一列本地 groupby 计数（纯函数；DBeaver 大表口径是下发 GROUP BY）
    Group,
    /// 元数据：列名 / 类型 / 来源表 / 注释 / 可空 / 默认值
    Metadata,
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
