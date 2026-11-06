//! 归档确认对话框（P1.4）：把"这次归档会变成什么"在动手前一次说清。
//!
//! 归档是**不可逆**的动作（文件从工作区搬进 `resources/` 并变只读，开发方案 R2），
//! 所以对话框要摆出：来源是什么、落到哪、来源连接是谁（归档凭证的关键一环）、
//! 目标被占用时怎么处理。字段与默认值取自原型 §4.1。
//!
//! 两条刻意的克制：
//!
//! 1. **目标位置只读**：目标相对路径由来源决定，不给输入框——手打相对路径就是绕过
//!    `PayloadStore::resolve` 的越界拒绝（模块硬约束 2 的入口守卫必须留在服务层）；
//! 2. **冲突不静默覆盖**：目标已被占用时，宿主在**开窗前**查好"改名后的落点"并作为
//!    [`ArchiveConflict`] 传进来，对话框原样摆出——用户看到的是确定的行为，不是"可能会重命名"。

pub(crate) use std::rc::Rc;

pub(crate) use gpui_kit::base::StyledExt;
pub(crate) use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
pub(crate) use gpui_kit::component::dialog::DialogFooter;
pub(crate) use gpui_kit::component::input::{Input, InputState};
pub(crate) use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, WindowExt as _};
pub(crate) use gpui_kit::prelude::FluentBuilder as _;
pub(crate) use gpui_kit::*;

pub(crate) use crate::model::{KeepVersions, PayloadMode};
pub(crate) use crate::resource_view::GroupOption;
pub(crate) use crate::ui;
pub(crate) use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
pub(crate) use gpui_kit::component::radio::Radio;

/// 「保留历史内容」输入的上限。
///
/// 100 份内容副本已是"基本不裁剪"；再加位数多半是手滑多打了一个 0，
/// 而每多一份都是整份文件副本，不值得让它悄悄生效（要真的全留就填 `-1`）。
pub const KEEP_VERSIONS_MAX: u32 = 100;

/// 打开归档对话框所需的全部信息（**宿主在事件路径备好**）。
///
/// 这里只放"要展示给用户看的"，不放 `ArchiveRequest` 本身：请求的构造（kind / binding /
/// rel_path 的最终落点）归宿主，对话框只回传用户能改的那几个字段
/// （见 [`ArchiveDialogResult`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDialogSeed {
    /// 来源一句话（如"草稿箱：reports/a.sql" / "本地文件：D:\x\a.sql"），只读展示。
    pub source_label: String,
    /// 目标相对路径（`resources/` 下），只读展示。
    pub rel_path: String,
    /// 显示名默认值（文件名去扩展名）。
    pub name: String,
    /// 来源连接（自动带出；`None` 显示"未记录"——**不猜**）。
    pub source_connection: Option<String>,
    /// 目标已占用时的"将改名到"提示（宿主查好；`None` = 无冲突）。
    pub conflict: Option<ArchiveConflict>,
    /// 可选分组的**现有名单**（宿主从面板快照的字典拿；空名单时只给「未分组」）。
    ///
    /// 不做「新建分组…」子项：那要“先建组再归档”的两步提交（或让服务层按名字找建），
    /// 而面板的行菜单 / 分组头右键已经有建组入口。
    pub groups: Vec<GroupOption>,
    /// 预选分组（`None` = 未分组）：默认分组设置项给的初值。
    pub group_id: Option<String>,
    /// 标签输入框的**继承预填**（草稿箱归档时带草稿条目标签名；用户可改可删）。
    pub initial_tags: Vec<String>,
    /// 是否允许选「引用」模式（§12.1.1 ①）：File 档为真；分析快照固定复制、不提供引用选项。
    pub allow_reference: bool,
    /// 归档方式初值（默认复制；宿主可按来源预设）。
    pub default_mode: PayloadMode,
    /// 源文件绝对路径（引用态「源位置」行展示用；复制态不读它）。
    ///
    /// 与 `source_label` 的区别：后者是"草稿箱：reports/a.sql"这类一句话来路描述，
    /// 不承担路径展示；引用本体留原位，表单要给出可核对的绝对路径。
    pub source_abs_path: Option<String>,
}

/// 目标占用：不静默覆盖，也不让用户手改路径——由宿主给出确定的改名落点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveConflict {
    /// 被占用的目标相对路径（展示用，带 `resources/` 前缀）。
    pub taken_rel_path: String,
    /// 宿主定好的改名落点（展示用，同样带前缀）。
    pub resolved_rel_path: String,
}

/// 对话框结果：**只含用户能改的字段**（路径 / 来源 / kind 由宿主补）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDialogResult {
    pub name: String,
    /// 别名（可空；空串 = 不设）。
    pub alias: Option<String>,
    /// 逗号 / 空格分隔的标签（已去空、去重、保序）。
    pub tags: Vec<String>,
    /// 归入的分组（`None` = 未分组）。
    pub group_id: Option<String>,
    /// `None` = 跟随设置默认（输入框留空即此意；`-1` = 全部保留）。
    pub keep_versions: Option<KeepVersions>,
    /// 用户选定的归档粒度（复制 / 引用）。
    pub payload_mode: PayloadMode,
}

/// 表单的输入实体（开窗前建好；测试可复核同一批实体）。
///
/// `Clone`：宿主与测试都要在开窗后继续持有它们（开窗会 `move` 一份进 builder）。
#[derive(Clone)]
pub struct ArchiveDialogInputs {
    pub name: Entity<InputState>,
    pub alias: Entity<InputState>,
    pub tags: Entity<InputState>,
    pub keep_versions: Entity<InputState>,
    /// 分组选择：下拉菜单不是 `InputState`，选择结果住在共享单元格里（开窗是 `Fn` 重建，
    /// 状态不能在闭包里）。`None` = 未分组。
    pub group_id: Rc<std::cell::RefCell<Option<String>>>,
    /// 归档方式选择：与分组同一纪律——状态住共享单元格，builder 每帧从它读。
    pub mode: Rc<std::cell::RefCell<PayloadMode>>,
}

/// 标签输入解析：`,` / `，` / 空白分隔；去空、**去重保序**（顺序是用户敲的，别打乱）。
pub fn parse_tags(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in text.split([',', '，', ' ', '\t', '\n']) {
        let tag = part.trim();
        if tag.is_empty() || out.iter().any(|seen| seen == tag) {
            continue;
        }
        out.push(tag.to_string());
    }
    out
}

/// 「保留历史内容」解析：**空 = 跟随设置**（`None`），数字 = 本次覆盖，`-1` = 全部保留。
///
/// 错误给的是**可直接当提示用**的一句话（校验与提示同源，不会出现"提示说非法、提交却过了"）。
pub fn parse_keep_versions(text: &str) -> Result<Option<KeepVersions>, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let Ok(value) = text.parse::<i64>() else {
        return Err("请填数字（留空 = 跟随设置）");
    };
    if value < 0 {
        // 负数只有 `-1` 有意义（与设置项同一个哨兵）；其余负数（如 `-2`）是误敲，
        // 不当作全留静默吃掉。
        if value == -1 {
            return Ok(Some(KeepVersions::All));
        }
        return Err("-1 = 全部保留（留空 = 跟随设置）");
    }
    if value > i64::from(KEEP_VERSIONS_MAX) {
        return Err("最多 100 份（留空 = 跟随设置）");
    }
    Ok(Some(KeepVersions::from_setting(value)))
}

/// 显示名的校验提示（`None` = 合法）。
pub fn name_hint(name: &str) -> Option<&'static str> {
    name.trim().is_empty().then_some("显示名不能为空")
}

/// 建表单实体并按 seed 预填（宿主与测试都从这里拿到同一批输入）。
pub fn build_inputs(
    window: &mut Window,
    cx: &mut App,
    seed: &ArchiveDialogSeed,
) -> ArchiveDialogInputs {
    let name = cx.new(|cx| InputState::new(window, cx).placeholder("显示名"));
    // 预填走 `set_value`：它**不发** `InputEvent::Change`，而本对话框的校验是每帧从值推导，
    // 因此无需额外同步条件（这正是选"推导"而非"标志位"的收益）。
    let name_value = seed.name.clone();
    name.update(cx, |state, cx| state.set_value(name_value, window, cx));

    // 标签预填 = 归档继承（草稿条目标签名）；同样走 `set_value`（不发 Change，校验仍从值推导）。
    let tags = cx.new(|cx| InputState::new(window, cx).placeholder("标签，逗号分隔（可空）"));
    if !seed.initial_tags.is_empty() {
        let tags_value = seed.initial_tags.join(", ");
        tags.update(cx, |state, cx| state.set_value(tags_value, window, cx));
    }
    let alias = cx.new(|cx| InputState::new(window, cx).placeholder("别名（可空）"));
    let keep_versions = cx.new(|cx| InputState::new(window, cx).placeholder("跟随设置（-1 全留）"));
    ArchiveDialogInputs {
        name,
        alias,
        tags,
        keep_versions,
        group_id: Rc::new(std::cell::RefCell::new(seed.group_id.clone())),
        mode: Rc::new(std::cell::RefCell::new(seed.default_mode)),
    }
}

/// 校验 + 取值；`None` = 不通过（对话框保持打开，内联提示已由渲染期给出）。
pub fn submit_archive(inputs: &ArchiveDialogInputs, cx: &App) -> Option<ArchiveDialogResult> {
    let name = inputs.name.read(cx).value().trim().to_string();
    if name_hint(&name).is_some() {
        return None;
    }
    let keep = parse_keep_versions(&inputs.keep_versions.read(cx).value()).ok()?;
    // 别名：全空白 = 不设（空串入库会让详情面板显示一个空行）。
    let alias = inputs.alias.read(cx).value().trim().to_string();
    Some(ArchiveDialogResult {
        name,
        alias: (!alias.is_empty()).then_some(alias),
        tags: parse_tags(&inputs.tags.read(cx).value()),
        group_id: inputs.group_id.borrow().clone(),
        keep_versions: keep,
        payload_mode: *inputs.mode.borrow(),
    })
}

mod fields;
mod open;
#[cfg(test)]
mod tests;

pub use self::open::{open_archive_dialog, open_archive_dialog_with};
