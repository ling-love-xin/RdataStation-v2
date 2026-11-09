//! Quick Open 浮层（独立视图实体）：输入 + 结果列表 + 键盘通道 + 元数据异步搜索。
//!
//! 宿主只做三件事：
//! 1. 把浮层挂进 overlay（`render_quick_open` 返回本实体的元素）；
//! 2. 注入动作端口 [`QuickOpenHost`]（执行一条结果要什么副作用，只有宿主知道）；
//! 3. 开关时置 `Shared::quick_open`（标题栏入口 / `Ctrl+P`），并调 [`QuickOpenPalette::on_opened`]。
//!
//! 这样键盘 / 焦点 / 防抖 / 回填都能用**简化宿主**在窗口测试里驱动（不必构造 `WorkbenchView`），
//! 也是将来把本模块升级成独立 crate 时的现成边界。
//!
//! 状态归属：
//! - `Shared::quick_open` = 开关（全应用一份：标题栏、设置页互斥、命令都读它）；
//! - 本实体 = 输入 / 结果 / 选中锚点 / 防抖与回填任务 / 元数据命中缓存。
//!
//! 子模块（同一类型跨文件拆 `impl` 块，字段只在本文件声明）：
//! - [`query`]：输入 → 查询（`refresh` 权威重算点）；
//! - [`search`]：元数据搜索的防抖发送 / 批次守卫 / 回填泵；
//! - [`files`]：草稿箱文件源（清单请求与快照）；
//! - [`selection`]：业务键选中 / ↑↓ 漫游 / 宿主 → 列表镜像；
//! - [`render`]：实体懒装配与整块渲染。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::input::InputState;
use gpui_kit::component::list::ListState;
use gpui_kit::*;

use crate::panels::Shared;
use crate::quick_open::delegate::QuickOpenDelegate;
use crate::quick_open::model::{self, Action};

mod files;
mod query;
mod render;
mod search;
mod selection;

use files::FilesSnapshot;

/// 宿主动作端口：执行一条结果（副作用语义在宿主一侧）。
pub(crate) trait QuickOpenHost: 'static {
    /// `keep_open` = Shift+↵ / Ctrl+点击（保留面板）。
    fn execute(&self, action: Action, keep_open: bool, window: &mut Window, cx: &mut App);
}

/// Quick Open 浮层。
pub(crate) struct QuickOpenPalette {
    shared: Shared,
    host: Rc<dyn QuickOpenHost>,
    /// 输入框（首帧渲染时创建：需要 `&mut Window`）。
    input: Option<Entity<InputState>>,
    _input_sub: Option<Subscription>,
    /// 结果列表（首帧渲染时创建；行 / 选中 / 空态归组件）。
    list: Option<Entity<ListState<QuickOpenDelegate>>>,
    /// 打开后待办：清输入 / 重置选中 / 聚焦 / 刷新（实体可能是上一帧才创建的）。
    pending_open: bool,
    /// 打开时「聚焦输入框」的待办标记。
    focus_pending: bool,
    /// 打开时锁定的初始模式：`Some(Mode::Goto)` 由 `Ctrl+Shift+O` 置位，本档内文本前缀
    /// 不再切换模式；`None` = 默认档（文本前缀 `>` / `#` 照常解析）。
    mode_override: Option<model::Mode>,
    /// 选中的行业务键（跨重算跟随的权威；组件的选中索引只是渲染锚点）。
    selected_key: Option<String>,
    /// 元数据命中（宿主缓存；后台回填后在 render 重建成行）。
    meta: Vec<model::MetaObject>,
    /// 文件源清单（每次打开重取一次；搜索是本地过滤，不跟输入走）。
    files: Option<FilesSnapshot>,
    /// 已为哪个项目根发过清单请求（避免每敲一个字都重发）。
    files_requested_for: Option<PathBuf>,
    /// 有清单请求在途。
    files_pending: bool,
    /// 已发给后台的搜索词（**只在词变时发**；回填时据此丢弃过期批次）。
    sent_query: Option<String>,
    /// 元数据搜索中（组头显示「搜索中…」）。
    searching: bool,
    /// 回填后有新数据、待 render 重建行（泵线程拿不到 `Window`）。
    rows_dirty: bool,
    /// 防抖任务句柄（替换即取消上一枚）。
    debounce: Option<Task<()>>,
    /// 结果回填泵（打开时启动；关闭期间降频空转）。
    pump: Option<Task<()>>,
    /// S1-① 批次序号：每次发搜索 +1，回填时丢弃旧批次（同词重跑被
    /// `sent_query` 拦截，但防抖窗口内开关/目标重排后重发同词时，序号是唯一权威）。
    search_seq: u64,
}

impl QuickOpenPalette {
    pub(crate) fn new(shared: Shared, host: Rc<dyn QuickOpenHost>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            shared,
            host,
            input: None,
            _input_sub: None,
            list: None,
            pending_open: false,
            focus_pending: false,
            mode_override: None,
            selected_key: None,
            meta: Vec::new(),
            files: None,
            files_requested_for: None,
            files_pending: false,
            sent_query: None,
            searching: false,
            rows_dirty: false,
            debounce: None,
            pump: None,
            search_seq: 0,
        };
        this.ensure_pump(cx);
        this
    }

    /// 打开边沿：宿主置位 `Shared::quick_open` 后调它。
    ///
    /// `mode` 锁定本次打开的档位：`Default`（`Ctrl+P`，文本前缀可切命令 / 全文）或
    /// `Goto`（`Ctrl+Shift+O`，本档内前缀不再切换）。真正的清输入 / 落选中 / 聚焦在下一帧
    /// render 里做（输入框可能还没创建）。
    pub(crate) fn on_opened(&mut self, mode: model::Mode, cx: &mut Context<Self>) {
        self.pending_open = true;
        self.focus_pending = true;
        self.mode_override = Some(mode);
        self.selected_key = None;
        // 文件清单每次打开重取一次（面板关着的时候草稿可能被外部改过）；
        // 在途时不置位——否则一次打开会发两份清单，后到的那份白算。
        if !self.files_pending {
            self.files_requested_for = None;
        }
        cx.notify();
    }

    /// 关闭（Esc / 点遮罩 / 执行完）：只改共享开关，宿主与状态栏读同一份。
    pub(super) fn close(&mut self, cx: &mut Context<Self>) {
        self.shared.quick_open.set(false);
        cx.notify();
    }

    /// 委托回传的确认（键盘 ↵：此时不在列表更新栈帧内，可回读列表取动作）→ 交给宿主执行；
    /// 不保留则自行关闭。
    pub(crate) fn confirm(&mut self, keep_open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(action) = self.selected_action(cx) else {
            return;
        };
        self.finish_confirm(action, keep_open, window, cx);
    }

    /// 鼠标点击行的确认路径：动作已由委托在列表更新栈帧内解析好传入。
    ///
    /// 不能复用 [`Self::confirm`]：那条路径会回读列表实体取动作，而鼠标确认正发生在
    /// 列表实体被 `update` 的栈帧里，重入读取会触发「实体正在被更新」panic。
    pub(crate) fn confirm_with(
        &mut self,
        action: Action,
        keep_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_confirm(action, keep_open, window, cx);
    }

    /// 执行动作并按「不保留则关面板」收尾。关闭是浮层自己的行为（与 Esc / 点遮罩同一组），
    /// 不依赖宿主实现——宿主可能只想记录/转发动作。
    fn finish_confirm(
        &mut self,
        action: Action,
        keep_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.host.execute(action, keep_open, window, cx);
        if !keep_open {
            self.close(cx);
        }
    }

    fn selected_action(&self, cx: &App) -> Option<Action> {
        let key = self.selected_key.as_deref()?;
        Some(self.list.as_ref()?.read(cx).delegate().action_of(key)?)
    }

    /// `⌥↵`：默认档 = 在导航树中定位选中行（只有元数据命中行可定位）；
    /// Goto 档主动作已是回树定位，`⌥↵` 改走次级属性动作。
    ///
    /// 与 `confirm` 分开而不是把它塞进 `Row.action`：同一行有两个动作（默认档 ↵ 属性 /
    /// ⌥↵ 定位；Goto 档 ↵ 定位 / ⌥↵ 属性），挤进一个字段执行侧就得猜“用户按的是哪个键”。
    pub(super) fn confirm_locate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.selected_key.clone() else {
            return;
        };
        // Goto 档：次级动作（属性面板）；无次级动作的行不动选中也不关面板。
        if self.mode_override == Some(model::Mode::Goto) {
            let Some(action) = self
                .list
                .as_ref()
                .and_then(|list| list.read(cx).delegate().secondary_action_of(&key))
            else {
                return;
            };
            self.host.execute(action, false, window, cx);
            self.close(cx);
            return;
        }
        let Some(object) = self
            .list
            .as_ref()
            .and_then(|list| list.read(cx).delegate().locate_of(&key))
        else {
            // 命令 / 连接 / 文件行不支持定位：不动选中也不关面板（当没按）。
            // 行上那条「⌥↵ 定位」提示本就只在可定位的行出现。
            return;
        };
        self.host
            .execute(Action::RevealMetadata(object), false, window, cx);
        self.close(cx);
    }

    /// 上一帧遗留的打开待办（清输入 → 刷新 → 聚焦）。
    ///
    /// 「打开」是**权威的刷新边沿**，不等输入框的 `Change`：
    /// 空输入也要出默认分组（连接 / 文件 / 命令）——否则打开后结果区是空的，
    /// 看起来像「搜不到」；而且文件清单的请求就挂在这一跳上。
    pub(super) fn consume_pending_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut opened = false;
        if self.pending_open {
            self.pending_open = false;
            opened = true;
            self.meta.clear();
            self.sent_query = None;
            self.searching = false;
            if let Some(input) = self.input.clone() {
                input.update(cx, |state, cx| state.set_value("", window, cx));
            }
        }
        if opened || self.rows_dirty {
            self.rows_dirty = false;
            self.refresh(window, cx);
        }
        if self.focus_pending {
            self.focus_pending = false;
            if let Some(input) = self.input.clone() {
                let handle = input.read(cx).focus_handle(cx);
                handle.focus(window, cx);
            }
        }
    }

    /// 测试只读访问器：输入框当前文本。
    #[cfg(test)]
    pub(crate) fn query_text(&self, cx: &App) -> String {
        self.input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// 测试只读访问器：结果行数。
    #[cfg(test)]
    pub(crate) fn row_count(&self, cx: &App) -> usize {
        self.list
            .as_ref()
            .map(|list| list.read(cx).delegate().row_count())
            .unwrap_or(0)
    }

    /// 测试只读访问器：结果行的业务键（按渲染顺序）。
    #[cfg(test)]
    pub(crate) fn row_keys(&self, cx: &App) -> Vec<String> {
        self.list
            .as_ref()
            .map(|list| list.read(cx).delegate().keys())
            .unwrap_or_default()
    }

    /// 测试只读访问器：选中的行业务键。
    #[cfg(test)]
    pub(crate) fn selected_key(&self) -> Option<String> {
        self.selected_key.clone()
    }

    /// 测试只读访问器：当前锁定模式（`None` = 默认档，文本前缀解析）。
    #[cfg(test)]
    pub(crate) fn current_mode(&self) -> Option<model::Mode> {
        self.mode_override
    }

    /// 测试只读访问器：是否处于「元数据搜索中」。
    #[cfg(test)]
    pub(crate) fn searching(&self) -> bool {
        self.searching
    }

    /// 测试只读访问器：已发给后台的搜索词（`None` = 一次都没发）。
    #[cfg(test)]
    pub(crate) fn sent_query(&self) -> Option<String> {
        self.sent_query.clone()
    }
}
