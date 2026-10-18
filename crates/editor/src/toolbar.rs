//! 工具栏分组自定义模型（T2c-5）
//!
//! ## 组 = 分隔符界定的连续段（原型 ⑭）
//!
//! 布局是一串有序槽位：`Item`（带可见性）或 `Separator`（竖线分隔符）。连续的 `Item`
//! 段构成一个组；分隔符**归前一段尾**（原型 `tbSegs` 的语义），段尾分隔符在自定义面板里
//! 显示为「┃ 竖线分隔符」行，可以隐藏 / 移动。
//!
//! ## 固定项（不进模型）
//!
//! - **模式指示器**：原型 ⑯ 明确「不参与 `tbOrder` / `moveToolbar` / `moveGroup`」，
//!   永远在最左，不参与排序 / 可见性 / 分组。
//! - **右对齐簇（执行位置 + 连接）**：T2c-5 里固定在最右（保持右对齐语义），不参与
//!   分组移动——原型把「通道·连接组」列为可移动组的例子，但那要连同右对齐语义一起想清楚
//!   （`tool-spacer` 是固定占位），放 T2c-6 随会话持久化时一并放开。
//!
//! ## 与执行簇的关系（原型 ⑬）
//!
//! 执行簇默认只显示「执行 + 全部运行」两个按钮；执行当前语句 / 执行选区 / 批量执行默认
//! 隐藏，可在 `···` 面板开启。组折叠时显示「首组件 + ▾」，▾ 收纳组内其余成员——这正是
//! 当前 `render_exec_group` 的下拉形态，所以「折叠」与「展开」是同一组的两副面孔。

use gpui_kit::assets::IconName;

use crate::model::EditorMode;

/// 模式指示器图标（原型 ⑯：工具栏最左「图标 + 模式名 + ▾」）
///
/// 模式图标是**工具栏概念**（放这里而不是 `EditorMode`：模型层不依赖图标资产）。
pub fn mode_icon(mode: EditorMode) -> IconName {
    match mode {
        EditorMode::General => IconName::FileCode,
        EditorMode::Sql => IconName::Database,
        EditorMode::Analysis => IconName::Notebook,
    }
}

/// 工具栏项的唯一业务键（ElementId 用键不用下标）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolbarItemId {
    /// 执行（主按钮：选区优先 → 当前语句；原型 `runBtn`，`fixed: true` 不可隐藏）
    ExecRun,
    /// 全部运行（原型 `runAllBtn`）
    ExecAll,
    /// 执行当前语句（原型 `runCurBtn`，默认隐藏）
    ExecCurrent,
    /// 执行选区（原型 `runSelBtn`，默认隐藏）
    ExecSelection,
    /// 批量执行（原型 `runBatchBtn`，默认隐藏）
    ExecBatch,
    /// 【原型 §2.2 分析组】执行计划（EXPLAIN）：常驻按钮（2026-10-11 提升常驻，不再进"更多"）
    Explain,
    /// 【原型 §2.2 分析组】方言转换：常驻按钮（目标方言列表；源方言已知排除自身）
    Translate,
    /// 【原型 §2.2 事务/模板】模板 / 代码片段：常驻按钮（无模板时置灰）
    Template,
    /// 【原型 §2.2 事务/模板】事务提交方式（自动 ⇄ 手动）：常驻开关（模式要一直看得见）
    TxAutoCommit,
    /// 格式化（文档级）
    Format,
    /// 【B8 余项】历史（原型 §2.2 的 `⟲ 历史`）：打开右 Dock 历史面板；文档级常驻
    History,
}

impl ToolbarItemId {
    /// 目录（面板渲染与断言共用一份；与布局无关，布局顺序看 `ToolbarLayout`）
    pub const ALL: &'static [Self] = &[
        Self::ExecRun,
        Self::ExecAll,
        Self::ExecCurrent,
        Self::ExecSelection,
        Self::ExecBatch,
        Self::Explain,
        Self::Translate,
        Self::Template,
        Self::TxAutoCommit,
        Self::Format,
        Self::History,
    ];

    /// 面板里的显示名（与工具栏按钮文案一致）
    pub fn label(self) -> &'static str {
        match self {
            Self::ExecRun => "执行",
            Self::ExecAll => "全部运行",
            Self::ExecCurrent => "执行当前语句",
            Self::ExecSelection => "执行选区",
            Self::ExecBatch => "批量执行",
            Self::Explain => "执行计划",
            Self::Translate => "方言转换",
            Self::Template => "模板",
            Self::TxAutoCommit => "自动提交",
            Self::Format => "格式化",
            Self::History => "历史",
        }
    }

    /// 面板三栏左列的图标（图标资产未注册时静默空白，不影响渲染）
    pub fn icon(self) -> IconName {
        match self {
            Self::ExecRun => IconName::Play,
            Self::ExecAll => IconName::SquareTerminal,
            Self::ExecCurrent => IconName::Play,
            Self::ExecSelection => IconName::Square,
            Self::ExecBatch => IconName::Menu,
            Self::Explain => IconName::Braces,
            Self::Translate => IconName::ArrowRightLeft,
            Self::Template => IconName::FilePlus,
            Self::TxAutoCommit => IconName::ArrowUpDown,
            Self::Format => IconName::Replace,
            Self::History => IconName::Timeline,
        }
    }

    /// 默认可见性（原型 §2.2：执行簇默认「执行 + 全部运行」；分析组 / 事务开关 /
    /// 文档级「格式化 · 历史」常驻——2026-10-11 修订，密度靠 ⋯ 自定义不靠隐藏）
    pub fn visible_by_default(self) -> bool {
        matches!(
            self,
            Self::ExecRun
                | Self::ExecAll
                | Self::Explain
                | Self::Translate
                | Self::Template
                | Self::TxAutoCommit
                | Self::Format
                | Self::History
        )
    }

    /// 是否**不可隐藏**（原型 `runBtn` `fixed: true`：执行主按钮永远在）
    pub fn fixed_visible(self) -> bool {
        matches!(self, Self::ExecRun)
    }
}

/// 布局槽位：一项 或 一条竖线分隔符
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolbarSlot {
    Item { id: ToolbarItemId, visible: bool },
    Separator,
}

/// 投影出的一个组（= 分隔符界定的连续段）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarGroup {
    /// 组头（组内第一个项；折叠状态的键，原型 `TB_FOLD[seg.head]`）
    pub head: ToolbarItemId,
    /// 组内全部项（含隐藏，按布局顺序）
    pub items: Vec<ToolbarItemId>,
    /// 本段尾部是否有竖线分隔符（有则在面板里显示「┃ 竖线分隔符」行）
    pub tail_separator: bool,
}

impl ToolbarGroup {
    /// 组内**可见**的项（工具栏渲染 / 折叠 ▾ 菜单都按它来）
    pub fn visible_items(&self, layout: &ToolbarLayout) -> Vec<ToolbarItemId> {
        self.items
            .iter()
            .copied()
            .filter(|id| layout.is_visible(*id))
            .collect()
    }
}

/// 工具栏布局（T2c-5 的权威状态；render 是纯读路径，改动走事件路径）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarLayout {
    /// 左区有序槽位（可排序 / 可折叠；右对齐簇不在此）
    left: Vec<ToolbarSlot>,
    /// 折叠的组：按组头 item 记（原型 `TB_FOLD`）
    folded: Vec<ToolbarItemId>,
}

impl Default for ToolbarLayout {
    fn default() -> Self {
        Self::default_layout()
    }
}

impl ToolbarLayout {
    /// 默认布局（原型 ⑬「重置恢复两按钮布局」的基准）：
    ///
    /// `[执行, 全部运行, 执行当前语句, 执行选区, 批量执行] │ [执行计划, 方言转换, 模板] │
    /// [自动提交, 格式化, 历史]`
    ///
    /// 执行簇默认只显示前两个；其余三项隐藏。分析组（执行计划 / 方言转换 / 模板）与
    /// 事务开关、文档级「格式化 · 历史」常驻（原型 §2.2 + 2026-10-11 修订：密度靠 ⋯
    /// 自定义不靠隐藏），各组之间各有一条竖线分隔符。
    pub fn default_layout() -> Self {
        Self {
            left: vec![
                ToolbarSlot::Item {
                    id: ToolbarItemId::ExecRun,
                    visible: true,
                },
                ToolbarSlot::Item {
                    id: ToolbarItemId::ExecAll,
                    visible: true,
                },
                ToolbarSlot::Item {
                    id: ToolbarItemId::ExecCurrent,
                    visible: false,
                },
                ToolbarSlot::Item {
                    id: ToolbarItemId::ExecSelection,
                    visible: false,
                },
                ToolbarSlot::Item {
                    id: ToolbarItemId::ExecBatch,
                    visible: false,
                },
                ToolbarSlot::Separator,
                ToolbarSlot::Item {
                    id: ToolbarItemId::Explain,
                    visible: true,
                },
                ToolbarSlot::Item {
                    id: ToolbarItemId::Translate,
                    visible: true,
                },
                ToolbarSlot::Item {
                    id: ToolbarItemId::Template,
                    visible: true,
                },
                ToolbarSlot::Separator,
                ToolbarSlot::Item {
                    id: ToolbarItemId::TxAutoCommit,
                    visible: true,
                },
                ToolbarSlot::Item {
                    id: ToolbarItemId::Format,
                    visible: true,
                },
                ToolbarSlot::Item {
                    id: ToolbarItemId::History,
                    visible: true,
                },
            ],
            folded: Vec::new(),
        }
    }

    /// 投影组列表（组 = 连续项段；分隔符归前一段尾；前导分隔符忽略）
    ///
    /// 纯函数：render 与面板共用同一份投影，测试直接断言它。
    pub fn groups(&self) -> Vec<ToolbarGroup> {
        let mut groups: Vec<ToolbarGroup> = Vec::new();
        let mut current: Vec<ToolbarItemId> = Vec::new();
        for slot in &self.left {
            match slot {
                ToolbarSlot::Item { id, .. } => current.push(*id),
                ToolbarSlot::Separator => {
                    if !current.is_empty() {
                        groups.push(ToolbarGroup {
                            head: current[0],
                            items: std::mem::take(&mut current),
                            tail_separator: true,
                        });
                    }
                }
            }
        }
        if !current.is_empty() {
            groups.push(ToolbarGroup {
                head: current[0],
                items: current,
                tail_separator: false,
            });
        }
        groups
    }

    /// 某组头对应的组（面板 / 折叠操作按组头定位）
    pub fn group_of(&self, head: ToolbarItemId) -> Option<ToolbarGroup> {
        self.groups().into_iter().find(|g| g.head == head)
    }

    /// 某项当前是否可见（隐藏项不渲染，但仍在组里、可在面板重开）
    pub fn is_visible(&self, id: ToolbarItemId) -> bool {
        self.left
            .iter()
            .find_map(|slot| match slot {
                ToolbarSlot::Item {
                    id: slot_id,
                    visible,
                } if *slot_id == id => Some(*visible),
                _ => None,
            })
            .unwrap_or(true)
    }

    /// 某组当前是否折叠（折叠 = 只显示首组件 + ▾）
    pub fn is_collapsed(&self, head: ToolbarItemId) -> bool {
        self.folded.contains(&head)
    }

    /// 切换可见性；返回是否真的变了（`false` = 该项不可隐藏 / 本就不在该状态）
    ///
    /// 原型 `applyTbVis` 对 `runBtn` 直接 return（不可隐藏）：`fixed_visible` 项恒保持可见。
    pub fn set_visible(&mut self, id: ToolbarItemId, on: bool) -> bool {
        if id.fixed_visible() && !on {
            return false;
        }
        let mut changed = false;
        for slot in &mut self.left {
            if let ToolbarSlot::Item {
                id: slot_id,
                visible,
            } = slot
            {
                if *slot_id == id && *visible != on {
                    *visible = on;
                    changed = true;
                }
            }
        }
        changed
    }

    /// 切换折叠状态；返回折叠后是否为「折叠」（`true` = 刚折叠上）
    pub fn toggle_collapsed(&mut self, head: ToolbarItemId) -> bool {
        if let Some(pos) = self.folded.iter().position(|h| *h == head) {
            self.folded.remove(pos);
            false
        } else {
            self.folded.push(head);
            true
        }
    }

    /// 组内成员数量（折叠 ▾ 菜单只在成员 ≥ 2 时有意义）
    pub fn group_member_count(&self, head: ToolbarItemId) -> usize {
        self.group_of(head).map(|g| g.items.len()).unwrap_or(0)
    }

    /// 移动单个项（可跨组；原型 `moveToolbar`：跨分隔符后自然形成新组）
    ///
    /// `dir`：`MoveDir::Left` = 与左邻槽位交换（原型 `up`）。
    /// 返回是否真的移动了（`false` = 已在边缘）。
    pub fn move_item(&mut self, id: ToolbarItemId, dir: MoveDir) -> bool {
        let pos = self.position_of(id).expect("目录里的项必在布局里");
        let (a, b) = match dir {
            MoveDir::Left if pos > 0 => (pos - 1, pos),
            MoveDir::Right if pos + 1 < self.left.len() => (pos, pos + 1),
            _ => return false,
        };
        self.left.swap(a, b);
        true
    }

    /// 整体移动一个组（原型 `moveGroup`：与相邻组整体交换）
    ///
    /// 组的左边界是它头一个成员，右边界是它最后一个成员（含段尾分隔符）。
    /// `MoveDir::Left` = 整组左移（与左邻组交换）；`MoveDir::Right` = 与右邻组交换。
    /// 实现：把本组的**整段槽位**摘出来，插到相邻组的外侧——两组整体换位。
    pub fn move_group(&mut self, head: ToolbarItemId, dir: MoveDir) -> bool {
        let groups = self.groups();
        let Some(index) = groups.iter().position(|g| g.head == head) else {
            return false;
        };
        let group = &groups[index];
        // 组的槽位区间：第一个成员起，到段尾分隔符止（没有分隔符就到最后一个成员）
        let start = self.position_of(group.items[0]).expect("组头必在布局里");
        let mut end = start + group.items.len() - 1;
        if group.tail_separator && end + 1 < self.left.len() {
            end += 1;
        }
        match dir {
            MoveDir::Left if index > 0 => {
                // 左邻组：把本组整段摘出，插到它开头之前
                let prev_start = self
                    .position_of(groups[index - 1].items[0])
                    .expect("左邻组必在布局里");
                let slots: Vec<ToolbarSlot> = self.left.drain(start..=end).collect();
                self.left.splice(prev_start..prev_start, slots);
                true
            }
            MoveDir::Right if index + 1 < groups.len() => {
                // 右邻组：把本组整段摘出，插到它末尾之后
                let next = &groups[index + 1];
                let next_start = self.position_of(next.items[0]).expect("右邻组必在布局里");
                let mut next_end = next_start + next.items.len() - 1;
                if next.tail_separator && next_end + 1 < self.left.len() {
                    next_end += 1;
                }
                let next_len = next_end - next_start + 1;
                let slots: Vec<ToolbarSlot> = self.left.drain(start..=end).collect();
                // 摘出后右邻组左移了 group_len：它在 [start .. start+next_len-1]
                self.left.splice(start + next_len..start + next_len, slots);
                true
            }
            _ => false,
        }
    }

    /// 是否有「新增」的竖线分隔符（默认布局之外、由用户在 `···` 面板加的）
    pub fn has_extra_separator(&self) -> bool {
        self.left
            .iter()
            .filter(|s| matches!(s, ToolbarSlot::Separator))
            .count()
            > 2
    }

    /// 在左区末尾加一条竖线分隔符（原型 `addSepBtn`：分隔符界定组边界；加在末尾后
    /// 用 [`Self::move_separator`] 移到想切开的位置——切开一个组 = 把分隔符移到该组内
    /// 两个成员之间）
    pub fn insert_separator(&mut self) {
        self.left.push(ToolbarSlot::Separator);
    }

    /// 移动一条竖线分隔符（面板的「┃」行 ↑↓）：与相邻槽位交换，跨过一项即改变组边界
    ///
    /// `index` 是它在 `left` 里的槽位下标（来自 [`Self::separator_positions`]）。
    /// 返回是否真的移动了（`false` = 已在边缘 / 该下标不是分隔符）。
    pub fn move_separator(&mut self, index: usize, dir: MoveDir) -> bool {
        if index >= self.left.len() || !matches!(self.left[index], ToolbarSlot::Separator) {
            return false;
        }
        let (a, b) = match dir {
            MoveDir::Left if index > 0 => (index - 1, index),
            MoveDir::Right if index + 1 < self.left.len() => (index, index + 1),
            _ => return false,
        };
        self.left.swap(a, b);
        true
    }

    /// 移除某条竖线分隔符；返回是否真的移除了
    pub fn remove_separator(&mut self, index: usize) -> bool {
        if index >= self.left.len() {
            return false;
        }
        if matches!(self.left[index], ToolbarSlot::Separator) {
            self.left.remove(index);
            true
        } else {
            false
        }
    }

    /// 全部竖线分隔符的位置（面板的「┃ 竖线分隔符」行定位用）
    pub fn separator_positions(&self) -> Vec<usize> {
        self.left
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| {
                if matches!(slot, ToolbarSlot::Separator) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    /// 重置默认（原型 `resetTb`）：清折叠、可见性回默认、删掉新增的分隔符
    pub fn reset(&mut self) {
        *self = Self::default_layout();
    }

    /// 【T2c-6】编码为稳定字符串（会话持久化用：存进 `editor_contexts` 的 TEXT 列）
    ///
    /// 格式（人可读、可手改）：槽位用 `,` 连接——可见项写 id、隐藏项写 `~id`、分隔符写 `|`；
    /// 折叠的组头若不为空，追加 `;` + 逗号连接的组头列表。不依赖 serde，往返见 [`Self::decode`]。
    pub fn encode(&self) -> String {
        let mut out: Vec<String> = Vec::new();
        for slot in &self.left {
            match slot {
                ToolbarSlot::Item { id, visible } => {
                    let tag = if *visible { "" } else { "~" };
                    out.push(format!("{tag}{id:?}"));
                }
                ToolbarSlot::Separator => out.push("|".to_string()),
            }
        }
        if !self.folded.is_empty() {
            // 折叠段直接拼接（不能当槽位 join，否则 `,;` 会把解码的槽位段尾缀空 token）
            let heads: Vec<String> = self.folded.iter().map(|id| format!("{id:?}")).collect();
            return format!("{};{}", out.join(","), heads.join(","));
        }
        out.join(",")
    }

    /// 【T2c-6】从 [`Self::encode`] 的字符串解码；非法 / 缺项 / 重复一律回 `None`（调用方保持默认）
    ///
    /// 校验：目录里 11 项必须**恰好出现一次**（顺序任意）、`folded` 里只留真实组头、
    /// 未知 id / 空串 / 重复项都视为坏数据。分隔符位置宽松（前导 / 尾部 / 连续都接受，
    /// 与 [`Self::groups`] 忽略空段的投影一致）；旧会话没这列 → `None`（不覆盖默认）。
    pub fn decode(code: &str) -> Option<Self> {
        let (slots_part, folded_part) = match code.split_once(';') {
            Some((left, right)) => (left, Some(right)),
            None => (code, None),
        };
        if slots_part.is_empty() {
            return None;
        }

        let mut left: Vec<ToolbarSlot> = Vec::new();
        let mut seen: Vec<ToolbarItemId> = Vec::new();
        for token in slots_part.split(',') {
            if token == "|" {
                left.push(ToolbarSlot::Separator);
                continue;
            }
            let (hidden, name) = match token.strip_prefix('~') {
                Some(rest) => (true, rest),
                None => (false, token),
            };
            let id = ToolbarItemId::ALL
                .iter()
                .copied()
                .find(|item| format!("{item:?}") == name)?;
            if seen.contains(&id) {
                return None; // 重复项
            }
            seen.push(id);
            left.push(ToolbarSlot::Item {
                id,
                visible: !hidden,
            });
        }
        if seen.len() != ToolbarItemId::ALL.len() {
            return None; // 缺项（目录必须全在，布局才有意义）
        }

        let mut folded: Vec<ToolbarItemId> = Vec::new();
        if let Some(part) = folded_part {
            // 折叠键是组头：先按已解码的布局投影组，只收真实组头（脏键不产生布局差异）
            let layout = Self {
                left: left.clone(),
                folded: Vec::new(),
            };
            let heads: Vec<ToolbarItemId> =
                layout.groups().into_iter().map(|g| g.head).collect();
            for name in part.split(',').filter(|s| !s.is_empty()) {
                let id = ToolbarItemId::ALL
                    .iter()
                    .copied()
                    .find(|item| format!("{item:?}") == name)?;
                if heads.contains(&id) && !folded.contains(&id) {
                    folded.push(id);
                }
            }
        }

        Some(Self { left, folded })
    }

    fn position_of(&self, id: ToolbarItemId) -> Option<usize> {
        self.left.iter().position(|slot| match slot {
            ToolbarSlot::Item { id: slot_id, .. } => *slot_id == id,
            ToolbarSlot::Separator => false,
        })
    }
}

/// 移动方向（面板 ↑ = 左移 / 上移，↓ = 右移 / 下移）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDir {
    Left,
    Right,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模式指示器图标：三模式各配一个（⑯），与菜单顺序一致
    #[test]
    fn mode_icons_cover_all_three_modes() {
        assert_eq!(
            mode_icon(EditorMode::General),
            IconName::FileCode,
            "通用 = 脚本/文件"
        );
        assert_eq!(mode_icon(EditorMode::Sql), IconName::Database, "SQL = 数据库");
        assert_eq!(
            mode_icon(EditorMode::Analysis),
            IconName::Notebook,
            "分析 = 笔记"
        );
    }

    /// 默认布局：三组、执行簇两按钮可见、分析组 / 事务开关 / 文档级常驻
    #[test]
    fn default_layout_is_exec_plus_all_visible() {
        let layout = ToolbarLayout::default_layout();
        assert!(layout.is_visible(ToolbarItemId::ExecRun));
        assert!(layout.is_visible(ToolbarItemId::ExecAll));
        assert!(!layout.is_visible(ToolbarItemId::ExecCurrent));
        assert!(!layout.is_visible(ToolbarItemId::ExecSelection));
        assert!(!layout.is_visible(ToolbarItemId::ExecBatch));
        // 原型 §2.2：分析组 / 事务开关 / 文档级常驻（2026-10-11 修订，不再进"更多"）
        assert!(layout.is_visible(ToolbarItemId::Explain));
        assert!(layout.is_visible(ToolbarItemId::Translate));
        assert!(layout.is_visible(ToolbarItemId::Template));
        assert!(layout.is_visible(ToolbarItemId::TxAutoCommit));
        assert!(layout.is_visible(ToolbarItemId::Format));
        assert!(layout.is_visible(ToolbarItemId::History));

        let groups = layout.groups();
        assert_eq!(groups.len(), 3, "执行簇 │ 分析组 │ 事务+文档级");
        assert_eq!(groups[0].head, ToolbarItemId::ExecRun);
        assert_eq!(
            groups[0].items,
            vec![
                ToolbarItemId::ExecRun,
                ToolbarItemId::ExecAll,
                ToolbarItemId::ExecCurrent,
                ToolbarItemId::ExecSelection,
                ToolbarItemId::ExecBatch,
            ]
        );
        assert!(groups[0].tail_separator, "执行簇与分析组之间有一条分隔符");
        assert_eq!(groups[1].head, ToolbarItemId::Explain);
        assert_eq!(
            groups[1].items,
            vec![
                ToolbarItemId::Explain,
                ToolbarItemId::Translate,
                ToolbarItemId::Template,
            ],
            "分析组「执行计划 · 方言转换 · 模板」"
        );
        assert!(groups[1].tail_separator);
        assert_eq!(groups[2].head, ToolbarItemId::TxAutoCommit);
        assert_eq!(
            groups[2].items,
            vec![
                ToolbarItemId::TxAutoCommit,
                ToolbarItemId::Format,
                ToolbarItemId::History,
            ],
            "事务开关 + 文档级「格式化 · 历史」"
        );
        assert!(!groups[2].tail_separator, "末组没有尾分隔符");
    }

    /// 组 = 分隔符界定的连续段：跨分隔符移动后形成新组
    #[test]
    fn moving_an_item_across_a_separator_reshapes_groups() {
        let mut layout = ToolbarLayout::default_layout();
        // 把「格式化」左移到分析组末尾（越过一条分隔符）：格式化与事务/历史拆开
        // 默认位置（组2 中间）：Format 在 TxAutoCommit 之后，先左移一次换位，再跨分隔符
        assert!(layout.move_item(ToolbarItemId::Format, MoveDir::Left)); // TxAutoCommit ↔ Format
        assert!(layout.move_item(ToolbarItemId::Format, MoveDir::Left)); // 跨过分隔符 → 分析组尾
        let groups = layout.groups();
        assert_eq!(groups.len(), 3, "格式化并入分析组，事务+历史自成一段");
        assert_eq!(groups[0].head, ToolbarItemId::ExecRun);
        assert_eq!(
            groups[1].items.last(),
            Some(&ToolbarItemId::Format),
            "格式化成为分析组的末成员"
        );
        assert_eq!(groups[2].head, ToolbarItemId::TxAutoCommit, "事务+历史独立成组");
    }

    /// 组整体移动：执行簇与格式化所在组交换位置
    #[test]
    fn moving_a_group_swaps_with_its_neighbor() {
        let mut layout = ToolbarLayout::default_layout();
        assert!(layout.move_group(ToolbarItemId::Explain, MoveDir::Left));
        let groups = layout.groups();
        assert_eq!(groups[0].head, ToolbarItemId::Explain, "分析组移到最前");
        assert_eq!(groups[1].head, ToolbarItemId::ExecRun, "执行簇紧随其后");
        assert_eq!(groups[2].head, ToolbarItemId::TxAutoCommit);
    }

    /// 组边缘移动被拒绝（不 panic）
    #[test]
    fn moving_off_the_edge_is_refused() {
        let mut layout = ToolbarLayout::default_layout();
        assert!(
            !layout.move_group(ToolbarItemId::ExecRun, MoveDir::Left),
            "首组不能再左移"
        );
        assert!(
            !layout.move_group(ToolbarItemId::TxAutoCommit, MoveDir::Right),
            "末组不能再右移"
        );
        assert!(
            !layout.move_item(ToolbarItemId::ExecRun, MoveDir::Left),
            "首项不能再左移"
        );
        assert!(
            !layout.move_item(ToolbarItemId::History, MoveDir::Right),
            "末项不能再右移"
        );
    }

    /// 折叠以组头为键；折叠 / 展开往返
    #[test]
    fn collapse_is_keyed_by_group_head() {
        let mut layout = ToolbarLayout::default_layout();
        assert!(!layout.is_collapsed(ToolbarItemId::ExecRun));
        assert!(
            layout.toggle_collapsed(ToolbarItemId::ExecRun),
            "第一次切换 = 折叠上"
        );
        assert!(layout.is_collapsed(ToolbarItemId::ExecRun));
        assert!(
            !layout.toggle_collapsed(ToolbarItemId::ExecRun),
            "再切一次 = 展开"
        );
        assert!(!layout.is_collapsed(ToolbarItemId::ExecRun));
    }

    /// 执行主按钮不可隐藏（原型 `fixed: true`）
    #[test]
    fn exec_run_cannot_be_hidden() {
        let mut layout = ToolbarLayout::default_layout();
        assert!(
            !layout.set_visible(ToolbarItemId::ExecRun, false),
            "执行不可隐藏"
        );
        assert!(layout.is_visible(ToolbarItemId::ExecRun));
        assert!(
            layout.set_visible(ToolbarItemId::ExecAll, false),
            "其余项可以隐藏"
        );
        assert!(!layout.is_visible(ToolbarItemId::ExecAll));
    }

    /// 隐藏项仍在组里（可在面板重开），不参与工具栏渲染
    #[test]
    fn hidden_items_stay_in_the_group() {
        let mut layout = ToolbarLayout::default_layout();
        layout.set_visible(ToolbarItemId::ExecAll, false);
        let groups = layout.groups();
        assert!(
            groups[0].items.contains(&ToolbarItemId::ExecAll),
            "隐藏项仍是组成员"
        );
        assert_eq!(
            groups[0].visible_items(&layout),
            vec![ToolbarItemId::ExecRun],
            "工具栏只渲染可见成员"
        );
    }

    /// 新增 / 移除竖线分隔符；重置默认清掉新增的
    #[test]
    fn separators_can_be_added_removed_and_reset() {
        let mut layout = ToolbarLayout::default_layout();
        assert!(!layout.has_extra_separator());
        layout.insert_separator();
        assert!(layout.has_extra_separator());
        assert_eq!(layout.groups().len(), 3, "末尾分隔符不新建组（归前一段尾）");

        // 移除新增的那条（位置 = 最后一个分隔符）
        let positions = layout.separator_positions();
        let last = *positions.last().expect("有一条新分隔符");
        assert!(layout.remove_separator(last));
        assert!(!layout.has_extra_separator());

        // 移除会改变槽位下标：再插入一条后重置，应回到默认
        layout.insert_separator();
        layout.reset();
        assert_eq!(layout, ToolbarLayout::default_layout());
    }

    /// 移动竖线分隔符：跨过一项即改变组边界（把末尾新增的分隔符移进执行簇 → 切开两组）
    #[test]
    fn moving_a_separator_into_a_group_splits_it() {
        let mut layout = ToolbarLayout::default_layout();
        layout.insert_separator();

        // 新增的分隔符在末尾：一路左移到执行簇内部（下标 1，紧挨执行主按钮）。
        // 起始下标动态取（布局加项不改这里——见 `default_layout` 的注释）
        let mut index = *layout
            .separator_positions()
            .last()
            .expect("有一条新分隔符");
        while index > 1 {
            assert!(layout.move_separator(index, MoveDir::Left), "第 {index} 步应可左移");
            index -= 1;
        }

        // 执行簇被切成了两段：「执行」单独一组，「全部运行…」在后一组
        let groups = layout.groups();
        assert_eq!(groups.len(), 4, "分隔符切出第 4 段");
        assert_eq!(groups[0].head, ToolbarItemId::ExecRun);
        assert_eq!(groups[1].head, ToolbarItemId::ExecAll, "分隔符在 执行/全部运行 之间");
    }

    /// 重置默认恢复「执行 + 全部运行」两按钮布局并清折叠
    #[test]
    fn reset_restores_the_default_layout() {
        let mut layout = ToolbarLayout::default_layout();
        layout.set_visible(ToolbarItemId::Format, false);
        layout.toggle_collapsed(ToolbarItemId::ExecRun);
        layout.insert_separator();
        layout.move_item(ToolbarItemId::History, MoveDir::Left);

        layout.reset();
        assert_eq!(layout, ToolbarLayout::default_layout());
        assert!(layout.is_visible(ToolbarItemId::Format));
        assert!(!layout.is_collapsed(ToolbarItemId::ExecRun));
        assert!(!layout.has_extra_separator());
    }

    /// 目录全覆盖：默认布局里每个目录项都出现一次
    #[test]
    fn every_catalog_item_appears_exactly_once_in_default() {
        let layout = ToolbarLayout::default_layout();
        for id in ToolbarItemId::ALL {
            let count = layout
                .left
                .iter()
                .filter(|slot| matches!(slot, ToolbarSlot::Item { id: sid, .. } if *sid == *id))
                .count();
            assert_eq!(count, 1, "{:?} 应在默认布局恰出现一次", id);
        }
    }

    /// 【T2c-6】默认布局编码 / 解码往返：不变
    #[test]
    fn default_layout_round_trips_through_its_code() {
        let layout = ToolbarLayout::default_layout();
        let code = layout.encode();
        assert_eq!(
            code,
            "ExecRun,ExecAll,~ExecCurrent,~ExecSelection,~ExecBatch,|,Explain,Translate,Template,|,TxAutoCommit,Format,History",
            "默认布局的编码应稳定可读"
        );
        let back = ToolbarLayout::decode(&code).expect("默认布局必可解码");
        assert_eq!(back, layout);
    }

    /// 【T2c-6】改过的布局（隐藏 / 折叠 / 移动 / 加分隔符）编码解码往返：不变
    #[test]
    fn a_customized_layout_round_trips_through_its_code() {
        let mut layout = ToolbarLayout::default_layout();
        layout.set_visible(ToolbarItemId::Format, false);
        layout.toggle_collapsed(ToolbarItemId::ExecRun);
        layout.move_item(ToolbarItemId::History, MoveDir::Left);
        layout.insert_separator();

        let code = layout.encode();
        let back = ToolbarLayout::decode(&code).expect("自定布局必可解码");
        assert_eq!(back, layout);
        assert!(!back.is_visible(ToolbarItemId::Format));
        assert!(back.is_collapsed(ToolbarItemId::ExecRun));
    }

    /// 【T2c-6】坏数据解码回 `None`（缺项 / 重复 / 未知 id / 空串）：调用方保持默认
    #[test]
    fn decode_rejects_bad_data() {
        // 缺一项
        assert_eq!(
            ToolbarLayout::decode(
                "ExecRun,ExecAll,ExecCurrent,ExecSelection,ExecBatch,|,Explain,Translate,Template,|,TxAutoCommit,Format"
            ),
            None,
            "缺项：历史不在"
        );
        // 重复项
        assert_eq!(
            ToolbarLayout::decode(
                "ExecRun,ExecRun,ExecAll,ExecCurrent,ExecSelection,ExecBatch,|,Explain,Translate,Template,|,TxAutoCommit,Format,History"
            ),
            None,
            "重复项"
        );
        // 未知 id
        assert_eq!(
            ToolbarLayout::decode(
                "ExecRun,ExecAll,ExecCurrent,ExecSelection,ExecBatch,|,Explain,Translate,Template,|,TxAutoCommit,Format,Nope"
            ),
            None,
            "未知 id"
        );
        // 空串
        assert_eq!(ToolbarLayout::decode(""), None, "空串");
        // 只分隔符（没有项）
        assert_eq!(ToolbarLayout::decode("|"), None, "没有项不成布局");
    }

    /// 【T2c-6】折叠段只收真实组头：脏键解码后不产生折叠（不报错、不丢布局）
    #[test]
    fn decode_drops_folded_keys_that_are_not_group_heads() {
        let code = "ExecRun,ExecAll,ExecCurrent,ExecSelection,ExecBatch,|,Explain,Translate,Template,|,TxAutoCommit,Format,History;TxAutoCommit";
        let layout = ToolbarLayout::decode(code).expect("折叠脏键不影响布局解码");
        // TxAutoCommit 是末组组头：折叠应保留
        assert!(layout.is_collapsed(ToolbarItemId::TxAutoCommit));

        // ExecCurrent 是组内成员不是组头：折叠键被丢
        let code = "ExecRun,ExecAll,ExecCurrent,ExecSelection,ExecBatch,|,Explain,Translate,Template,|,TxAutoCommit,Format,History;ExecCurrent";
        let layout = ToolbarLayout::decode(code).expect("折叠脏键不影响布局解码");
        assert!(!layout.is_collapsed(ToolbarItemId::ExecCurrent));
        assert_eq!(layout.folded, Vec::new());
    }
}
