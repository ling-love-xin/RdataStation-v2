//! 动作可用性判据：**一处判定，所有入口共用**。
//!
//! 同一条存档的动作在四个地方出现（行右键菜单 / `F2` 快捷键 / 详情头部名字与别名 /
//! 详情动作区），四个入口各写一遍判据就会漂：实测出现过「右键置灰但 `F2` 能开窗」
//! 「全局行详情能点名字改名」这类偏差——用户碰得到，测试却各测各的。
//!
//! 本模块只做**纯判定**（不碰 UI、不查库）：调用方把当前上下文装成 [`ActionContext`]，
//! 再问 `can_*`。文案（禁用理由）留在各入口自己手里——同一件事在不同位置需要不同的
//! 说法（菜单不给长句，按钮下方要给出口）。

use super::{ArchiveStatus, RowScope};

/// 一次动作判定的全部输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionContext {
    /// 实例归属：项目行可操作，全局行只读（只能分发到本项目）。
    pub scope: RowScope,
    /// 本体状态（缺失 = 本体被删/被移走，只剩登记）。
    pub status: ArchiveStatus,
    /// 本体是否登记了路径（File 档 = `resources/` 下相对路径；分析表 / 引用型恒 `false`）。
    pub has_payload: bool,
    /// **项目**是否只读（不是行的 `readonly` 列：那是归档只读，两者不是一件事）。
    pub read_only: bool,
    /// 本次动作作用几行（多选时是整选集）。
    pub multi_count: usize,
}

impl ActionContext {
    /// 项目行（只有 `ArchiveRow`、没有详情时的降级构造；`has_payload` 给 `false`——
    /// 需要本体的动作会因此判为不可用，这是安全的一侧）。
    pub fn from_row(scope: RowScope, status: ArchiveStatus, read_only: bool, multi_count: usize) -> Self {
        Self {
            scope,
            status,
            has_payload: false,
            read_only,
            multi_count,
        }
    }

    /// 单行、单选、非只读项目的常态上下文（测试与简单调用点用）。
    pub fn project_row() -> Self {
        Self {
            scope: RowScope::Project,
            status: ArchiveStatus::Normal,
            has_payload: true,
            read_only: false,
            multi_count: 1,
        }
    }

    fn single(&self) -> bool {
        self.multi_count <= 1
    }

    fn project(&self) -> bool {
        self.scope == RowScope::Project
    }
}

/// 打开本体（只读）：本体必须登记了路径且还在，且只开一条（多选时打开哪一条都是猜）。
pub fn can_open(ctx: &ActionContext) -> bool {
    ctx.single() && ctx.status != ArchiveStatus::Missing && ctx.has_payload
}

/// 取回（检出）：往草稿箱写工作副本——只对项目行、本体正常、项目非只读。
///
/// 内容已变（`ContentChanged`）也挡：那是"本体被绕过只读改过"，先让用户在状态行
/// 接受当前内容或从历史还原，再取回（否则取走的是来路不明的内容）。
pub fn can_checkout(ctx: &ActionContext) -> bool {
    ctx.project() && ctx.status == ArchiveStatus::Normal && !ctx.read_only
}

/// 升级到全局：本质是一次发布——项目行、本体在位、单选、项目非只读。
///
/// 全局行必须挡（否则能把全局表再"升级"一次，撞同名冲突）；这一点旧判据漏了。
pub fn can_promote(ctx: &ActionContext) -> bool {
    ctx.project() && can_open(ctx) && ctx.single() && !ctx.read_only
}

/// 分发到本项目：全局行、本体在位（表被删了但登记还在时 COPY 必失败）、单选、项目非只读。
///
/// 注意**不**要求 `has_payload`：分析表型全局行的本体在全局 DuckDB 里，`file_rel_path`
/// 本就是空——要求它会把最常分发的那一类挡掉。
pub fn can_distribute(ctx: &ActionContext) -> bool {
    ctx.scope == RowScope::Global && ctx.status != ArchiveStatus::Missing && ctx.single() && !ctx.read_only
}

/// 改显示名 / 别名：只写登记、不碰本体，但**本体缺失的行不给**（原型 §3.1：那一行先走索引修复，
/// 改名只会让人以为问题解决了）。
///
/// 全局行必须挡：全局登记不归本项目管（旧判据只在菜单里挡，`F2` 与详情头部都漏了）。
pub fn can_rename(ctx: &ActionContext) -> bool {
    ctx.project() && ctx.single() && ctx.status != ArchiveStatus::Missing && !ctx.read_only
}

/// 改标签：只写组织归属，不需要本体；全局行没有项目标签归属。
pub fn can_edit_tags(ctx: &ActionContext) -> bool {
    ctx.project() && !ctx.read_only
}

/// 移动到分组：全局行没有项目分组归属；只读项目写不进去。
pub fn can_move_to_group(ctx: &ActionContext) -> bool {
    ctx.project() && !ctx.read_only
}

/// 移入回收站（列表 / 多选）：项目行都可以——本体缺失的行也能删，那正是清理脏登记的手段。
pub fn can_delete_row(ctx: &ActionContext) -> bool {
    ctx.project()
}

/// 移入回收站（详情危险区）：比列表多两道——本体必须还在、项目非只读。
///
/// 与列表不同是有意的：详情下面那句提示要说清"本体去哪了"，而缺失行没有本体可送进
/// 回收站（该先走索引修复）。
pub fn can_delete_from_detail(ctx: &ActionContext) -> bool {
    ctx.project() && ctx.status != ArchiveStatus::Missing && ctx.has_payload && !ctx.read_only
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_rows_can_only_distribute() {
        let mut ctx = ActionContext::project_row();
        ctx.scope = RowScope::Global;
        ctx.has_payload = false; // 分析表型全局行
        assert!(can_distribute(&ctx), "分析表型全局行也要能分发");
        assert!(!can_promote(&ctx), "全局行不能再升级一次");
        assert!(!can_rename(&ctx), "全局登记不归本项目管");
        assert!(!can_edit_tags(&ctx));
        assert!(!can_move_to_group(&ctx));
        assert!(!can_delete_row(&ctx));
    }

    #[test]
    fn missing_global_row_is_not_distributable() {
        let mut ctx = ActionContext::project_row();
        ctx.scope = RowScope::Global;
        ctx.status = ArchiveStatus::Missing;
        assert!(
            !can_distribute(&ctx),
            "本体缺失时 COPY 必失败，入口就该置灰"
        );
    }

    #[test]
    fn read_only_project_blocks_every_writing_action() {
        let mut ctx = ActionContext::project_row();
        ctx.read_only = true;
        assert!(!can_checkout(&ctx));
        assert!(!can_promote(&ctx));
        assert!(!can_rename(&ctx));
        assert!(!can_edit_tags(&ctx));
        assert!(!can_move_to_group(&ctx));
        assert!(!can_delete_from_detail(&ctx));
        assert!(can_open(&ctx), "读本体不写库，只读项目也能打开");
    }

    #[test]
    fn multi_selection_blocks_single_row_actions() {
        let mut ctx = ActionContext::project_row();
        ctx.multi_count = 2;
        assert!(!can_promote(&ctx));
        assert!(!can_open(&ctx), "多选时打开哪一条都是猜");
        assert!(!can_rename(&ctx));
        assert!(can_edit_tags(&ctx), "打标签是唯一按整选集生效的组织动作");
        assert!(can_delete_row(&ctx), "删除是多选唯一可用的破坏性项");
    }

    #[test]
    fn missing_project_row_keeps_only_the_cleanup_paths() {
        let mut ctx = ActionContext::project_row();
        ctx.status = ArchiveStatus::Missing;
        assert!(!can_rename(&ctx), "本体缺失先走索引修复，改名只会掩盖问题");
        assert!(!can_open(&ctx));
        assert!(!can_delete_from_detail(&ctx), "详情要先走索引修复");
        assert!(can_delete_row(&ctx), "列表删记录正是清理脏登记的手段");
    }
}
