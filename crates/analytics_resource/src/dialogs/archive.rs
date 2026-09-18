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

use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::model::KeepVersions;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use crate::resource_view::GroupOption;
use crate::ui;

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

    let tags = cx.new(|cx| InputState::new(window, cx).placeholder("标签，逗号分隔（可空）"));
    let alias = cx.new(|cx| InputState::new(window, cx).placeholder("别名（可空）"));
    let keep_versions = cx.new(|cx| InputState::new(window, cx).placeholder("跟随设置（-1 全留）"));
    ArchiveDialogInputs {
        name,
        alias,
        tags,
        keep_versions,
        group_id: Rc::new(std::cell::RefCell::new(seed.group_id.clone())),
    }
}

/// 校验 + 取值；`None` = 不通过（对话框保持打开，内联提示已由渲染期给出）。
pub fn submit_archive(
    inputs: &ArchiveDialogInputs,
    cx: &App,
) -> Option<ArchiveDialogResult> {
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
    })
}

/// 打开归档确认对话框（宿主入口）。
///
/// `on_submit` 只在**校验通过**时回调一次，负责构造 `ArchiveRequest` 并落库（对话框不碰存储）。
pub fn open_archive_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: ArchiveDialogSeed,
    on_submit: impl Fn(ArchiveDialogResult, &mut App) + 'static,
) {
    let inputs = build_inputs(window, cx, &seed);
    open_archive_dialog_with(window, cx, seed, inputs, on_submit);
}

/// 同上，但用调用方建好的输入实体（窗口测试从这条缝进来，复核"同一批实体 + 真渲染"）。
pub fn open_archive_dialog_with(
    window: &mut Window,
    cx: &mut App,
    seed: ArchiveDialogSeed,
    inputs: ArchiveDialogInputs,
    on_submit: impl Fn(ArchiveDialogResult, &mut App) + 'static,
) {
    let ArchiveDialogInputs {
        name,
        alias,
        tags,
        keep_versions,
        group_id,
    } = inputs;
    let on_submit = Rc::new(on_submit);
    // 分组名单只在开窗时取一份：分组是单层的，一份列表就是这个对话框的生命周期里真实可选项。
    let groups = seed.groups.clone();

    // 按钮文案把"会发生什么"写进动作里：有冲突时不是"归档"而是"改名归档"。
    let ok_text = if seed.conflict.is_some() {
        "改名归档"
    } else {
        "归档"
    };

    let submit: Rc<dyn Fn(&mut App) -> bool> = {
        let name = name.clone();
        let alias = alias.clone();
        let tags = tags.clone();
        let keep_versions = keep_versions.clone();
        let group_id = group_id.clone();
        let on_submit = on_submit.clone();
        Rc::new(move |cx: &mut App| -> bool {
            let inputs = ArchiveDialogInputs {
                name: name.clone(),
                alias: alias.clone(),
                tags: tags.clone(),
                keep_versions: keep_versions.clone(),
                group_id: group_id.clone(),
            };
            let Some(result) = submit_archive(&inputs, cx) else {
                return false;
            };
            on_submit(result, cx);
            true
        })
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        // 提示每帧从当前值推导：用户改回去，提示自然消失（无标志位可残留）。
        let name_hint = name_hint(&name.read(cx).value());
        let keep = parse_keep_versions(&keep_versions.read(cx).value());
        let keep_hint = keep.err();
        // 写了 `-1` 时把它的含义摆出来：这是个哨兵值，不说明就等于让用户猜。
        let keep_note = match keep {
            Ok(Some(KeepVersions::All)) => Some("全部保留：不裁剪任何历史内容副本".to_string()),
            Ok(Some(keep)) => Some(format!("本次归档：{}", keep.label())),
            _ => None,
        };
        // builder 是 `Fn`（每帧重建）：两条提交路径各拿一份**本帧的**副本——
        // 内层 `move` 闭包只能拿走副本，外层不能再持有它们（否则 builder 就不是 `Fn` 了）。
        let submit_for_button = submit.clone();
        let submit_for_ok = submit.clone();

        let form = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(field_label(theme, "显示名"))
            .child(Input::new(&name))
            .when_some(name_hint, |this, hint| {
                this.child(hint_line(hint, theme.colors.danger))
            })
            .child(field_label(theme, "别名"))
            .child(Input::new(&alias))
            .child(field_label(theme, "分组"))
            .child(group_picker(&group_id, &groups))
            .child(field_label(theme, "标签"))
            .child(Input::new(&tags))
            .child(field_label(theme, "保留历史内容"))
            .child(Input::new(&keep_versions))
            .when_some(keep_hint, |this, hint| {
                this.child(hint_line(hint, theme.colors.danger))
            })
            .when_some(keep_note, |this, note| {
                this.child(hint_line(&note, theme.colors.muted_foreground))
            });

        dialog
            .title("归档为存档")
            .w(cx.theme().font_size * ui::ARCHIVE_DIALOG_WIDTH)
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_3()
                    .child(readonly_block(theme, &seed))
                    .when_some(seed.conflict.as_ref(), |this, conflict| {
                        this.child(hint_line(
                            &format!(
                                "{} 已被占用：本次将归档为 {}（不覆盖）",
                                conflict.taken_rel_path, conflict.resolved_rel_path
                            ),
                            theme.colors.warning,
                        ))
                    })
                    .child(form),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("archive-dialog-cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "archive-dialog-cancel".to_string())
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("archive-dialog-ok")
                            .with_variant(ButtonVariant::Primary)
                            .small()
                            .debug_selector(|| "archive-dialog-ok".to_string())
                            .label(ok_text)
                            .on_click(move |_, window, cx| {
                                // 校验不过：不关窗（内联提示已出现，让用户接着改）。
                                if !submit_for_button(cx) {
                                    return;
                                }
                                window.close_dialog(cx);
                            }),
                    ),
            )
            // Enter 确认 / Esc 取消（对话框 key context 自带绑定）。
            .on_ok(move |_, _window, cx| submit_for_ok(cx))
            .on_cancel(|_, _, _| true)
    });
}

/// 只读信息区：来源 / 目标位置 / 来源连接。
///
/// 与表单区的形状刻意不同（标签在上是"要你填"，标签在左是"告诉你"），
/// 用户扫一眼就知道哪几行动不了。
fn readonly_block(theme: &Theme, seed: &ArchiveDialogSeed) -> Div {
    let mut rows = div().v_flex().w_full().gap_2();
    for (label, value) in [
        ("来源", seed.source_label.clone()),
        ("目标位置", seed.rel_path.clone()),
        (
            "来源连接",
            seed.source_connection
                .clone()
                .unwrap_or_else(|| "未记录".to_string()),
        ),
    ] {
        rows = rows.child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .text_xs()
                .child(
                    div()
                        .w(rems(ui::DETAIL_LABEL_WIDTH))
                        .flex_none()
                        .text_color(theme.colors.muted_foreground)
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .child(value),
                ),
        );
    }
    rows
}

/// 分组选择：一个下拉按钮（未分组 / 各分组），当前项打勾。
///
/// 为什么选择结果住在 `Rc<RefCell<_>>` 而不是实体：下拉菜单不是输入框，没有现成的实体可挂；
/// 而开窗 builder 是 `Fn`（每帧重建），状态不能放在闭包里——选项点击后调 `window.refresh()`
/// 让对话框重画一帧，按钮文案就跟着变。
fn group_picker(
    group_id: &Rc<std::cell::RefCell<Option<String>>>,
    groups: &[GroupOption],
) -> Div {
    let current = group_id.borrow().clone();
    // 名单里找不到的 id（分组刚被别处删了）按“未分组”显示：不把悬空 id 当选项摆出来。
    let label = current
        .as_ref()
        .and_then(|id| groups.iter().find(|group| &group.id == id))
        .map(|group| group.name.clone())
        .unwrap_or_else(|| "未分组".to_string());

    let items = groups.to_vec();
    let is_none = current.is_none();
    let button = Button::new("archive-group")
        .ghost()
        .small()
        .label(format!("{label} ▾"))
        .dropdown_menu({
            let target = group_id.clone();
            move |menu, _window, _cx| {
                let mut menu = menu.item(PopupMenuItem::label("分组"));
                let target_none = target.clone();
                menu = menu.item(
                    PopupMenuItem::new("未分组")
                        .checked(is_none)
                        .on_click(move |_, window, _cx| {
                            *target_none.borrow_mut() = None;
                            window.refresh();
                        }),
                );
                for group in &items {
                    let id = group.id.clone();
                    let checked = current.as_deref() == Some(id.as_str());
                    let target = target.clone();
                    let target_id = id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(group.name.clone())
                            .checked(checked)
                            .on_click(move |_, window, _cx| {
                                *target.borrow_mut() = Some(target_id.clone());
                                window.refresh();
                            }),
                    );
                }
                menu
            }
        });

    div().w_full().child(button)
}

/// 字段名（弱化色，与它下面的输入框一组）。
fn field_label(theme: &Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 一行提示（校验用 `danger`，结果告知用 `warning`）。
fn hint_line(text: &str, color: gpui_kit::Hsla) -> Div {
    div()
        .w_full()
        .text_xs()
        .text_color(color)
        .child(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::{KEEP_VERSIONS_MAX, name_hint, parse_keep_versions, parse_tags};
    use crate::model::KeepVersions;

    #[test]
    fn tags_split_on_commas_and_spaces_keeping_order() {
        assert_eq!(
            parse_tags(" 报表, 月度  季度，报表 "),
            vec!["报表", "月度", "季度"],
            "去空、去重保序（中文逗号也算分隔符）"
        );
        assert!(parse_tags("   ").is_empty(), "全空白 = 没有标签");
        assert!(parse_tags("").is_empty());
    }

    #[test]
    fn keep_versions_empty_means_following_settings() {
        assert_eq!(parse_keep_versions(""), Ok(None));
        assert_eq!(parse_keep_versions("  "), Ok(None));
        assert_eq!(parse_keep_versions("3"), Ok(Some(KeepVersions::Keep(3))));
        assert_eq!(
            parse_keep_versions("0"),
            Ok(Some(KeepVersions::MetadataOnly)),
            "0 = 只留元数据"
        );
        assert_eq!(
            parse_keep_versions(&KEEP_VERSIONS_MAX.to_string()),
            Ok(Some(KeepVersions::Keep(KEEP_VERSIONS_MAX)))
        );
        assert!(parse_keep_versions("abc").is_err());
        assert!(
            parse_keep_versions(&(KEEP_VERSIONS_MAX + 1).to_string()).is_err(),
            "超出上限挡在对话框，不留给服务层裁剪"
        );
    }

    /// `-1` = 全部保留（与设置项同一个哨兵）；其余负数不当全留静默吃掉。
    #[test]
    fn minus_one_means_keep_everything() {
        assert_eq!(parse_keep_versions("-1"), Ok(Some(KeepVersions::All)));
        assert!(parse_keep_versions("-2").is_err());
        assert!(parse_keep_versions("-").is_err());
    }

    /// 设置项字面量 ↔ 领域口径的往返（宿主在两个 crate 之间就是靠这一对转的）。
    #[test]
    fn keep_versions_roundtrips_the_setting_value() {
        for (setting, expected) in [
            (-1, KeepVersions::All),
            (0, KeepVersions::MetadataOnly),
            (5, KeepVersions::Keep(5)),
        ] {
            let value = KeepVersions::from_setting(setting);
            assert_eq!(value, expected);
            assert_eq!(value.to_setting(), setting, "落盘值不能被改变");
        }
        // 全留 = 不裁剪（不是“保留 0 份”）；只留元数据 = 保留 0 份。
        assert_eq!(KeepVersions::All.limit(), None);
        assert_eq!(KeepVersions::MetadataOnly.limit(), Some(0));
    }

    #[test]
    fn name_hint_only_fires_on_blank() {
        assert!(name_hint("月报").is_none());
        assert_eq!(name_hint("  "), Some("显示名不能为空"));
    }
}
