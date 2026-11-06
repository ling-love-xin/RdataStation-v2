//! 详情分区行与异常提示行（纯数据，不碰视图）。

use super::*;

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
            "实例",
            match detail.scope {
                RowScope::Project => "当前项目".to_string(),
                RowScope::Global => "全局实例（只读，可分发到本项目）".to_string(),
            },
        ),
        (
            "只读",
            if detail.readonly {
                "归档后不可写，需取回编辑".to_string()
            } else {
                "可写（非文件型本体不带只读属性）".to_string()
            },
        ),
        (
            "归档方式",
            match detail.payload_mode {
                PayloadMode::Copy => "复制存档（本体在 resources/ 下，只读）".to_string(),
                PayloadMode::Reference => "引用存档（本体留在源位置，快照指向源路径）".to_string(),
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
    // reference 模式的本体在源位置：把源绝对路径摆出来，方便判断引用对象是谁。
    if let Some(src) = detail.source_path.as_deref() {
        source.push(("源路径", src.to_string()));
    }
    source.push((HASH_LABEL, short_hash(detail.content_hash.as_deref())));
    sections.push(("来源".to_string(), source));

    // 3) 版本（**总是出现**：只有 v1 的存档也能打开版本历史看一眼——那个列表里
    //    至少有一行当前版本，而“没有历史”本身就是一条信息）
    let history = if detail.history_label.is_empty() {
        "只有当前版本".to_string()
    } else {
        detail.history_label.clone()
    };
    sections.push((VERSION_SECTION_TITLE.to_string(), vec![("历史", history)]));

    // 4) 组织（**只有分组**：标签是可交互的 chips，在 `render_tag_section` 里单独渲染——
    //    （标签, 值）两列是纯文本通道，装不下带 × 的控件）
    let mut org = Vec::new();
    if let Some(group) = detail.group.as_deref() {
        org.push(("分组", group.to_string()));
    }
    if !org.is_empty() {
        sections.push(("组织".to_string(), org));
    }

    sections
}

/// 异常提示行：只有需要用户处理的状态才出现（缺失 / 内容已变 / 引用可能失效）。
pub fn alert_line(detail: &ArchiveDetail) -> Option<String> {
    match (detail.status, detail.kind) {
        (ArchiveStatus::Missing, _) => {
            Some("本体缺失：可能被手工删除或移动，可用索引修复还原或删除记录".to_string())
        }
        // 两类“变了”的原因不同，文案不能合用：文件型是**字节**被改过，
        // 分析表看的是**定义与列结构**（行数变化不算，见 `analysis.rs` 的裁决 R4）。
        (ArchiveStatus::ContentChanged, ArchiveKind::Analysis) => Some(format!(
            "{}：可接受当前结构（生成新版本）或从历史还原",
            crate::analysis::STRUCTURE_CHANGED_NOTE
        )),
        (ArchiveStatus::ContentChanged, _) => Some(
            "内容已变：本体被绕过只读改过，可接受当前内容（生成新版本）或从历史还原".to_string(),
        ),
        // 引用型复现强度最弱：常态就提示，不等它失效。
        (ArchiveStatus::Normal, ArchiveKind::TableRef) => {
            Some("这是引用：源表可能已变更或被删除，使用前建议立即校验".to_string())
        }
        // 本地文件引用（§12.1.1 末段）：常态即说明本体归属与失效条件；与旧 TableRef 正交。
        (ArchiveStatus::Normal, _) if detail.payload_mode == PayloadMode::Reference => {
            Some("引用型：本体在源位置，源变更后快照失效".to_string())
        }
        _ => None,
    }
}
