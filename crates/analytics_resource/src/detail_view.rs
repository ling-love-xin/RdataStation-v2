//! 存档详情（M6 自持视图）：右侧属性面板的内容层。
//!
//! 与面板同一纪律：**纯渲染 + 纯数据**——取值全部来自宿主推来的 [`ArchiveDetail`]（已格式化），
//! render 期零 I/O、零计算。
//!
//! 本批只做**只读信息区**（头部 / 基本信息 / 来源 / 版本 / 标签与分组）。危险区与
//! 版本历史等**动作按钮**随对话框批接入（需要宿主回调，见开发方案 Phase 1）；此处不摆按钮，
//! 避免出现"点了没反应"的入口。

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use crate::model::{ArchiveKind, ArchiveStatus};
use crate::resource_view::{BadgeTone, ResourcesHost, badge_tone, strength_badge};
use crate::ui;

/// 指纹展示长度（前 12 位：足够比对，又不至于把面板撑爆）。
pub const HASH_PREVIEW_LEN: usize = 12;

/// 一条存档的详情快照（**宿主已格式化**：大小 / 时间 / 标签等都已是人读文案）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDetail {
    pub id: String,
    pub name: String,
    pub alias: Option<String>,
    pub kind: ArchiveKind,
    pub version: i32,
    pub status: ArchiveStatus,
    /// 归档后只读（`kind = analysis` 也可能为 false：表本身不可加只读属性）。
    pub readonly: bool,
    /// 大小（文件型）或规模（分析表型，如 "12,480 行 × 18 列"）；旧行可能为空。
    pub size_label: String,
    pub modified_label: String,
    pub archived_label: String,
    /// 来源草稿相对路径（归档凭证的"出处"）。
    pub promoted_from: Option<String>,
    pub source_connection_id: Option<String>,
    pub source_table: Option<String>,
    /// 内容指纹（完整值；展示时截断）。
    pub content_hash: Option<String>,
    /// 本体在 `resources/` 下的相对路径（`None` = 旧行没有登记）。
    ///
    /// 不在面板上展示（原型 §3.1 没有这一行），但**取回**要用它推默认工作副本名
    /// （显示名与扩展名是两回事：显示名可以改成中文，扩展名不行）。
    pub payload_rel_path: Option<String>,
    /// 版本历史摘要，如 "3 个历史版本 · 最近副本完整"。
    pub history_label: String,
    pub tags: Vec<String>,
    pub group: Option<String>,
}

/// 指纹缩略：前 [`HASH_PREVIEW_LEN`] 位；无指纹（旧行 / `table_ref`）给破折号而不是空白。
pub fn short_hash(hash: Option<&str>) -> String {
    match hash {
        Some(value) if !value.is_empty() => value.chars().take(HASH_PREVIEW_LEN).collect(),
        _ => "—".to_string(),
    }
}

/// 分区标题 → （标签, 值）行。
///
/// 空值**不产生行**：面板里一排"（无）"除了占地方没有信息量；只有"缺失/异常"这类
/// **需要用户处理**的状态才显式出现。
pub fn detail_rows(detail: &ArchiveDetail) -> Vec<(String, Vec<(&'static str, String)>)> {
    let mut sections = Vec::new();

    // 1) 基本信息
    let mut basic = vec![
        ("种类", kind_label(detail.kind).to_string()),
        ("版本", format!("v{}", detail.version)),
        (
            "只读",
            if detail.readonly {
                "归档后不可写，需取回编辑".to_string()
            } else {
                "可写（非文件型本体不带只读属性）".to_string()
            },
        ),
    ];
    if !detail.size_label.is_empty() {
        basic.push(("大小", detail.size_label.clone()));
    }
    if !detail.modified_label.is_empty() {
        basic.push(("修改时间", detail.modified_label.clone()));
    }
    if !detail.archived_label.is_empty() {
        basic.push(("归档时间", detail.archived_label.clone()));
    }
    sections.push(("基本信息".to_string(), basic));

    // 2) 来源（归档凭证的"出处"：回答这结论是用什么数据得出的）
    let mut source = Vec::new();
    if let Some(from) = detail.promoted_from.as_deref() {
        source.push(("来源草稿", from.to_string()));
    }
    if let Some(conn) = detail.source_connection_id.as_deref() {
        source.push(("来源连接", conn.to_string()));
    }
    if let Some(table) = detail.source_table.as_deref() {
        source.push(("来源表", table.to_string()));
    }
    source.push(("内容指纹", short_hash(detail.content_hash.as_deref())));
    sections.push(("来源".to_string(), source));

    // 3) 版本（无历史版本时不出现空分区——与其它分区同一口径：空值不产生行）
    if !detail.history_label.is_empty() {
        sections.push((
            "版本".to_string(),
            vec![("历史", detail.history_label.clone())],
        ));
    }

    // 4) 组织（标签 / 分组只在有内容时出现）
    let mut org = Vec::new();
    if !detail.tags.is_empty() {
        org.push(("标签", detail.tags.join("、")));
    }
    if let Some(group) = detail.group.as_deref() {
        org.push(("分组", group.to_string()));
    }
    if !org.is_empty() {
        sections.push(("组织".to_string(), org));
    }

    sections
}

/// 种类中文名（面板与详情共用一处，避免两处各写一份）。
pub fn kind_label(kind: ArchiveKind) -> &'static str {
    match kind {
        ArchiveKind::File => "受管文件",
        ArchiveKind::Analysis => "分析表",
        ArchiveKind::TableRef => "远端引用",
    }
}

/// 异常提示行：只有需要用户处理的状态才出现（缺失 / 内容已变 / 引用可能失效）。
pub fn alert_line(detail: &ArchiveDetail) -> Option<String> {
    match (detail.status, detail.kind) {
        (ArchiveStatus::Missing, _) => {
            Some("本体缺失：可能被手工删除或移动，可用索引修复还原或删除记录".to_string())
        }
        (ArchiveStatus::ContentChanged, _) => {
            Some("内容已变：本体被绕过只读改过，可接受当前内容（生成新版本）或从历史还原".to_string())
        }
        // 引用型复现强度最弱：常态就提示，不等它失效。
        (ArchiveStatus::Normal, ArchiveKind::TableRef) => {
            Some("这是引用：源表可能已变更或被删除，使用前建议立即校验".to_string())
        }
        _ => None,
    }
}

/// 详情面板的动作接线（`None` = 纯只读渲染：crate 单测与无宿主场景）。
///
/// 动作一律经宿主端口（与列表右键菜单同一套）：面板不认识服务层，也不自己取数。
pub struct DetailActions {
    pub host: std::rc::Rc<dyn ResourcesHost>,
    /// 项目只读：写类动作一律禁用（与右键菜单同一判据）。
    pub read_only: bool,
}

/// 渲染详情面板内容（只读信息区 + 动作区）。
pub fn render_detail(detail: &ArchiveDetail, actions: Option<DetailActions>, cx: &App) -> Div {
    let (foreground, muted, border, tone_color) = {
        let colors = cx.theme().colors;
        (colors.foreground, colors.muted_foreground, colors.border, colors)
    };
    let badge = strength_badge(detail.kind, detail.status);
    let badge_color = match badge_tone(detail.kind, detail.status) {
        BadgeTone::Success => tone_color.success,
        BadgeTone::Info => tone_color.info,
        BadgeTone::Warning => tone_color.warning,
        BadgeTone::Danger => tone_color.danger,
    };
    let alert = alert_line(detail);

    let mut body = div().v_flex().w_full().gap_2().p_2();

    // 头部：名称 + 别名 + 版本 + 强度徽标
    let mut header = div()
        .v_flex()
        .w_full()
        .gap_0p5()
        .child(
            div()
                .h_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_ellipsis()
                        .text_color(foreground)
                        .child(detail.name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("v{}", detail.version)),
                )
                .child(div().text_xs().text_color(badge_color).child(badge)),
        );
    if let Some(alias) = detail.alias.as_deref() {
        header = header.child(div().text_xs().text_color(muted).child(alias.to_string()));
    }
    if detail.status == ArchiveStatus::Missing {
        // 缺失行不给"看起来正常"的头部：名称同步转弱。
        header = header.child(
            div()
                .text_xs()
                .text_color(tone_color.danger)
                .child("本体缺失"),
        );
    }
    body = body.child(header);

    // 异常提示条
    if let Some(alert) = alert {
        body = body.child(
            div()
                .w_full()
                .px_2()
                .py_1()
                .text_xs()
                .border_1()
                .border_color(tone_color.warning)
                .text_color(tone_color.warning)
                .child(alert),
        );
    }

    // 分区
    for (title, rows) in detail_rows(detail) {
        let mut section = div().v_flex().w_full().gap_1().child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child(title),
        );
        for (label, value) in rows {
            section = section.child(
                div()
                    .h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(div().w(rems(ui::DETAIL_LABEL_WIDTH)).flex_none().text_xs().text_color(muted).child(label))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(foreground)
                            .child(value),
                    ),
            );
        }
        body = body.child(section);
    }

    // 动作区：目前只有「取回（检出）…」——它是只读存档**唯一的编辑入口**，也是用户看完
    // 归档凭证后最可能的下一步。其余动作各自被挡着（打开只读 = P1.6、移入回收站 = P0.8、
    // 标签 / 版本历史 = Phase 2/3），**不提前摆点不动的入口**。
    if let Some(actions) = actions {
        let blocked = actions.read_only
            || matches!(
                detail.status,
                ArchiveStatus::Missing | ArchiveStatus::ContentChanged
            );
        // 禁用时给的理由要**指向出口**（去哪儿处理），不是一句"不可用"。
        let hint = if actions.read_only {
            "项目处于只读模式"
        } else if blocked {
            "本体异常，先在状态行「修复…」处理"
        } else {
            "复制一份可写的工作副本，本体不动"
        };
        let host = actions.host.clone();
        // 闭包是 `Fn`（每帧重建），且它比 `detail` 活得久——拷一份带走。
        let detail_for_click = detail.clone();
        body = body.child(
            div()
                .v_flex()
                .w_full()
                .gap_1()
                .border_t(px(1.0))
                .border_color(border)
                .pt_2()
                .child(
                    Button::new("archive-detail-checkout")
                        .ghost()
                        .label("取回（检出）…")
                        .disabled(blocked)
                        .on_click(move |_, window, cx| {
                            host.request_checkout(&detail_for_click, window, cx)
                        }),
                )
                .child(div().text_xs().text_color(muted).child(hint)),
        );
    }

    div()
        .v_flex()
        .w_full()
        .min_h_0()
        .border_l(px(1.0))
        .border_color(border)
        .bg(cx.theme().colors.background)
        .child(body)
}

#[cfg(test)]
mod tests {
    // 安全模式：测试模块不通配导入。
    use super::{ArchiveDetail, alert_line, detail_rows, kind_label, short_hash};
    use crate::model::{ArchiveKind, ArchiveStatus};

    fn detail(status: ArchiveStatus, kind: ArchiveKind) -> ArchiveDetail {
        ArchiveDetail {
            id: "ar_1".to_string(),
            name: "dau_report".to_string(),
            alias: None,
            kind,
            version: 2,
            status,
            readonly: true,
            size_label: "1.2 KB".to_string(),
            modified_label: "3 天前".to_string(),
            archived_label: "3 天前".to_string(),
            promoted_from: Some("scratchpad/dau.sql".to_string()),
            source_connection_id: Some("conn_1".to_string()),
            source_table: None,
            content_hash: Some("0123456789abcdef0123".to_string()),
            payload_rel_path: Some("dau.sql".to_string()),
            history_label: "1 个历史版本".to_string(),
            tags: Vec::new(),
            group: None,
        }
    }

    #[test]
    fn short_hash_truncates_and_tolerates_missing() {
        assert_eq!(short_hash(Some("0123456789abcdef0123")), "0123456789ab");
        assert_eq!(short_hash(None), "—");
        assert_eq!(short_hash(Some("")), "—");
    }

    #[test]
    fn rows_skip_empty_values_but_keep_fingerprint_placeholder() {
        let rows = detail_rows(&detail(ArchiveStatus::Normal, ArchiveKind::File));
        let basic = rows.iter().find(|(title, _)| title == "基本信息").expect("基本信息");
        // 只读说明必须出现（用户最常问"为什么不能编辑"）。
        assert!(basic.1.iter().any(|(label, value)| *label == "只读" && value.contains("取回")));

        let source = rows.iter().find(|(title, _)| title == "来源").expect("来源");
        assert!(source.1.iter().any(|(label, _)| *label == "来源草稿"));
        // 没有来源表 → 不产生该行；指纹则恒有（无值给破折号）。
        assert!(!source.1.iter().any(|(label, _)| *label == "来源表"));
        assert!(source.1.iter().any(|(label, _)| *label == "内容指纹"));
    }

    #[test]
    fn organization_section_is_hidden_when_empty() {
        let rows = detail_rows(&detail(ArchiveStatus::Normal, ArchiveKind::File));
        assert!(
            !rows.iter().any(|(title, _)| title == "组织"),
            "无标签无分组时不应出现空分区"
        );

        let mut with_tags = detail(ArchiveStatus::Normal, ArchiveKind::File);
        with_tags.tags = vec!["报表".to_string(), "月度".to_string()];
        with_tags.group = Some("报表".to_string());
        let rows = detail_rows(&with_tags);
        let org = rows.iter().find(|(title, _)| title == "组织").expect("组织");
        assert!(org.1.iter().any(|(_, value)| value == "报表、月度"));
    }

    #[test]
    fn alerts_only_for_actionable_states() {
        assert!(alert_line(&detail(ArchiveStatus::Normal, ArchiveKind::File)).is_none());
        assert!(alert_line(&detail(ArchiveStatus::ContentChanged, ArchiveKind::File))
            .expect("内容已变应提示")
            .contains("内容已变"));
        assert!(alert_line(&detail(ArchiveStatus::Missing, ArchiveKind::File))
            .expect("缺失应提示")
            .contains("本体缺失"));
        // 引用型常态就要提示（复现强度最弱，不等失效）。
        assert!(alert_line(&detail(ArchiveStatus::Normal, ArchiveKind::TableRef))
            .expect("引用应提示")
            .contains("源表可能已变更"));
    }

    #[test]
    fn kind_labels_are_shared_wording() {
        assert_eq!(kind_label(ArchiveKind::File), "受管文件");
        assert_eq!(kind_label(ArchiveKind::Analysis), "分析表");
        assert_eq!(kind_label(ArchiveKind::TableRef), "远端引用");
    }
}
