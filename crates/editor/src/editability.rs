//! 结果集单元格编辑能力判据（T2a-1，纯函数）
//!
//! 原型 §2.4 的口径：无主键 / 只读 / 通道不允许写 → 编辑入口不亮。
//! 判据顺序与既有闸门一致（通道闸 → 项目闸 → 只读 → 行标识 → 列存在），
//! 且只报**第一条**过不去的——原因在界面上展示时，用户改掉最上面那条就够。
//!
//! 这里是"能不能编辑这一格"的**唯一**判据：网格双击、工具栏入口、提交按钮
//! 全走 [`cell_editable`]，不许各写一份判断（判断分叉 = 界面行为分叉）。

use crate::channel::ExecChannel;
use crate::export::RowKey;
use crate::model::ReadOnly;
use crate::project::ProjectState;

/// 编辑被拦的原因（只报第一条；文案给界面展示）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditGate {
    /// 当前执行通道不允许写源库（加速 / 联邦档是只读副本）
    ChannelReadOnly,
    /// 项目为只读模式（标题栏的锁）
    ProjectReadOnly,
    /// 文档为只读（查看历史快照 / 分析资源锁定 / 大文件预览这类查看态）
    EditorReadOnly,
    /// 连接只读（连接策略只读：可查不可写）
    ConnectionReadOnly,
    /// 这份结果没有行定位依据（主键 / 唯一键）：写回会定位不到行、甚至改错行
    NoRowKey,
    /// 列越界（列下标超出结果列数）
    ColumnMissing,
}

impl EditGate {
    /// 界面文案（结果工具栏 / 双击后的提示都读它）
    pub fn reason(self) -> &'static str {
        match self {
            Self::ChannelReadOnly => "当前通道为只读副本，写回需切回源库通道",
            Self::ProjectReadOnly => "项目为只读模式，不能写回数据库",
            Self::EditorReadOnly => "当前文档为只读（查看态），不能编辑结果",
            Self::ConnectionReadOnly => "连接为只读，写语句被拦",
            Self::NoRowKey => "这份结果没有主键/唯一键，无法定位要更新的行",
            Self::ColumnMissing => "列不存在",
        }
    }
}

/// 单元格编辑判据：一次返回 `Ok(())` 或第一条过不去的 `Err(EditGate)`
///
/// 参数都是面板手里现成的快照（渲染路径可调，不做 I/O）：
/// - `channel`：本文档的当前执行通道（写回源库的前提是源库通道）
/// - `project`：项目态快照（标题栏锁）
/// - `read_only`：只读的两个独立维度（文档只读 / 连接只读，见 `model::ReadOnly`）
/// - `key`：这份结果的行定位依据（`None` = 探测不到主键/唯一键 → 整表只读）
/// - `columns_len`：结果列数（判列越界）
/// - `column`：目标列下标（0 = 第一数据列）
pub fn cell_editable(
    channel: ExecChannel,
    project: ProjectState,
    read_only: ReadOnly,
    key: Option<&RowKey>,
    columns_len: usize,
    column: usize,
) -> Result<(), EditGate> {
    if !channel.allows_source_writes() {
        return Err(EditGate::ChannelReadOnly);
    }
    if project.read_only {
        return Err(EditGate::ProjectReadOnly);
    }
    if read_only.editor {
        return Err(EditGate::EditorReadOnly);
    }
    if read_only.connection {
        return Err(EditGate::ConnectionReadOnly);
    }
    if key.is_none() {
        return Err(EditGate::NoRowKey);
    }
    if column >= columns_len {
        return Err(EditGate::ColumnMissing);
    }
    Ok(())
}

/// 这份结果**整体**能不能编辑（入口级判据：工具栏「提交」/ 双击入口的开关）
///
/// 与 [`cell_editable`] 只差最后一步：不判具体列，行标识有就行。
pub fn result_editable(
    channel: ExecChannel,
    project: ProjectState,
    read_only: ReadOnly,
    key: Option<&RowKey>,
) -> Result<(), EditGate> {
    if !channel.allows_source_writes() {
        return Err(EditGate::ChannelReadOnly);
    }
    if project.read_only {
        return Err(EditGate::ProjectReadOnly);
    }
    if read_only.editor {
        return Err(EditGate::EditorReadOnly);
    }
    if read_only.connection {
        return Err(EditGate::ConnectionReadOnly);
    }
    if key.is_none() {
        return Err(EditGate::NoRowKey);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::RowKey;

    fn gates(channel: ExecChannel, project: bool, read_only: (bool, bool)) -> (ExecChannel, ProjectState, ReadOnly) {
        (
            channel,
            ProjectState { read_only: project },
            ReadOnly {
                editor: read_only.0,
                connection: read_only.1,
            },
        )
    }

    /// 判据矩阵：每条只改一个变量，验证「只报第一条」的优先级
    #[test]
    fn matrix_reports_first_failing_gate_only() {
        let key = RowKey::single("id");
        let (channel, project, read_only) = gates(ExecChannel::Source, false, (false, false));

        // 全部放行
        assert_eq!(
            cell_editable(channel, project, read_only, Some(&key), 3, 1),
            Ok(())
        );

        // 通道优先：即使项目也只读，也只报通道
        let (ch_accel, prj_ro, ro) = gates(ExecChannel::Accelerated, true, (true, true));
        assert_eq!(
            cell_editable(ch_accel, prj_ro, ro, Some(&key), 3, 1),
            Err(EditGate::ChannelReadOnly)
        );

        // 项目只读次之（通道放行时）
        let (ch, prj_ro, ro) = gates(ExecChannel::Source, true, (false, false));
        assert_eq!(
            cell_editable(ch, prj_ro, ro, Some(&key), 3, 1),
            Err(EditGate::ProjectReadOnly)
        );

        // 文档只读再次之
        let (ch, prj, ro) = gates(ExecChannel::Source, false, (true, false));
        assert_eq!(
            cell_editable(ch, prj, ro, Some(&key), 3, 1),
            Err(EditGate::EditorReadOnly)
        );

        // 连接只读再次之
        let (ch, prj, ro) = gates(ExecChannel::Source, false, (false, true));
        assert_eq!(
            cell_editable(ch, prj, ro, Some(&key), 3, 1),
            Err(EditGate::ConnectionReadOnly)
        );

        // 没有行标识（通道/只读都放行时）
        let (ch, prj, ro) = gates(ExecChannel::Source, false, (false, false));
        assert_eq!(
            cell_editable(ch, prj, ro, None, 3, 1),
            Err(EditGate::NoRowKey)
        );

        // 列越界最后
        assert_eq!(
            cell_editable(channel, project, read_only, Some(&key), 3, 3),
            Err(EditGate::ColumnMissing)
        );
    }

    #[test]
    fn result_editable_ignores_column_dimension() {
        let key = RowKey::single("id");
        let (channel, project, read_only) = gates(ExecChannel::Source, false, (false, false));
        assert_eq!(result_editable(channel, project, read_only, Some(&key)), Ok(()));
        assert_eq!(
            result_editable(channel, project, read_only, None),
            Err(EditGate::NoRowKey)
        );
        assert_eq!(
            result_editable(ExecChannel::Federated, project, read_only, Some(&key)),
            Err(EditGate::ChannelReadOnly)
        );
    }

    #[test]
    fn gate_reasons_are_truthful() {
        // 文案不能是空串（界面要拿它当提示条/禁用原因）
        for gate in [
            EditGate::ChannelReadOnly,
            EditGate::ProjectReadOnly,
            EditGate::EditorReadOnly,
            EditGate::ConnectionReadOnly,
            EditGate::NoRowKey,
            EditGate::ColumnMissing,
        ] {
            assert!(!gate.reason().is_empty());
        }
    }
}
