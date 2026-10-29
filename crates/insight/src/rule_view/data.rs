//! 规则管理对话框 · 视图模型与数据组装（自 rule_view.rs 拆分，纯位移）

use super::*;
/// 分组展示顺序：**覆盖优先级从高到低**（与加载顺序相反）。
///
/// 与加载顺序相反是为了让用户先看到「谁赢」——项目规则压全局、全局压内置，
/// 按加载顺序排版会让人以为内置规则最优先。
pub const GROUP_ORDER: [RuleScope; 3] = [RuleScope::Project, RuleScope::Global, RuleScope::Builtin];

// ==================== 视图模型（纯数据，可脱窗口单测） ====================

/// 规则行的异常状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleRowStatus {
    Ok,
    /// 文件在、解析失败（`error` 是 TOML 报错原文，含行列信息）
    Invalid {
        error: String,
    },
    /// 索引里有记录、磁盘上文件已不在
    Missing,
}

impl RuleRowStatus {
    pub fn is_broken(&self) -> bool {
        !matches!(self, RuleRowStatus::Ok)
    }
}

/// 一条规则的展示行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRowView {
    pub id: String,
    /// 展示名（索引里的 `name` 为空时退回 `id`，不让用户看空白行）
    pub name: String,
    pub category: String,
    pub version: String,
    /// 当前启停状态（用户决策，落索引表）
    pub enabled: bool,
    pub status: RuleRowStatus,
    /// 可在系统编辑器中打开的文件（内置规则内嵌、文件已丢失时为 `None`）
    pub file: Option<PathBuf>,
}

/// 一个作用域分组
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleGroupView {
    pub scope: RuleScope,
    /// 分组标题（「项目规则」/「全局规则」/「内置规则」）
    pub title: &'static str,
    /// 该层落点说明：目录路径，或内置层的「应用内嵌」
    pub location: String,
    /// 该层目录尚不存在（内置层恒为 false）——界面据此给「新建」入口
    pub dir_missing: bool,
    pub rows: Vec<RuleRowView>,
}

impl RuleGroupView {
    /// 空态提示（按层给不同的话：空的含义不一样）
    pub fn empty_hint(&self) -> &'static str {
        match self.scope {
            RuleScope::Project => "还没有项目规则。项目规则随项目走，可提交到版本库",
            RuleScope::Global => "跨项目复用的规则放在这里，所有项目可见",
            RuleScope::Builtin => "内置规则缺失：安装包不完整",
        }
    }
}

/// 对话框的完整数据（三组 + 底部统计 + 信任门横幅）
///
/// 统计数字取自**未过滤**的全量数据：搜索词不该让「共 20 条」变成「共 2 条」，
/// 那会让人误以为规则被删了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesData {
    pub groups: Vec<RuleGroupView>,
    pub total: usize,
    pub disabled: usize,
    /// 异常条数（解析失败 + 文件丢失）
    pub broken: usize,
    /// 项目层有规则但**未信任**因而未装配（信任门横幅；`None` = 没这回事）
    pub pending: Option<PendingRulesView>,
}

/// 未信任的项目规则（信任门横幅的数据）。
///
/// 这是「你正在决定什么」的全部信息：多少条、叫什么、目录在哪、其中几条本来就解析不过。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRulesView {
    pub ids: Vec<String>,
    /// 其中解析失败的条数（未信任也看得见「这份规则本身就有毛病」）
    pub invalid: usize,
    pub dir: String,
    /// 用户此前已明确选择「不加载」（此时横幅改说「你选择了不加载」）
    pub declined: bool,
}

impl PendingRulesView {
    pub fn count(&self) -> usize {
        self.ids.len()
    }

    /// 展示用的 id 列表：最多 6 条，多的部分用「等 N 条」收尾。
    pub fn summary(&self) -> String {
        const SHOWN: usize = 6;
        let shown: Vec<&str> = self.ids.iter().take(SHOWN).map(|id| id.as_str()).collect();
        let mut text = shown.join(" / ");
        if self.ids.len() > SHOWN {
            text.push_str(&format!(" 等 {} 条", self.ids.len()));
        }
        text
    }
}

impl Default for RulesData {
    fn default() -> Self {
        Self {
            groups: GROUP_ORDER
                .iter()
                .map(|&scope| RuleGroupView {
                    scope,
                    title: group_title(scope),
                    location: String::new(),
                    dir_missing: false,
                    rows: Vec::new(),
                })
                .collect(),
            total: 0,
            disabled: 0,
            broken: 0,
            pending: None,
        }
    }
}

impl RulesData {
    pub fn group(&self, scope: RuleScope) -> Option<&RuleGroupView> {
        self.groups.iter().find(|g| g.scope == scope)
    }

    /// 可见行数（过滤后调用；底部统计用）。
    pub fn visible_rows(&self) -> usize {
        self.groups.iter().map(|g| g.rows.len()).sum()
    }

    /// 按搜索词过滤（事件路径调用，render 只读结果）。
    ///
    /// 只过滤行、保留分组骨架：空分组仍显示该层的空态与落点路径，
    /// 「搜不到」与「这一层本来就没规则」因此不会混为一谈。
    pub fn filtered(&self, query: &str) -> Self {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return self.clone();
        }
        let mut out = self.clone();
        for group in &mut out.groups {
            group.rows.retain(|row| row_matches(row, &needle));
        }
        out
    }
}

/// 行是否命中搜索词：规则名 / id / 分类三者任一包含即可（大小写不敏感）
pub(crate) fn row_matches(row: &RuleRowView, needle_lower: &str) -> bool {
    row.name.to_lowercase().contains(needle_lower)
        || row.id.to_lowercase().contains(needle_lower)
        || row.category.to_lowercase().contains(needle_lower)
}

fn group_title(scope: RuleScope) -> &'static str {
    match scope {
        RuleScope::Project => "项目规则",
        RuleScope::Global => "全局规则",
        RuleScope::Builtin => "内置规则",
    }
}

// ==================== 取数结果的组装（纯函数） ====================

/// 组装 [`RulesData`] 所需的原始输入（都是取数阶段已拿到的所有权数据）。
#[derive(Debug, Clone, Default)]
pub struct RuleDataInput {
    /// 项目规则目录（无项目时 `None`）
    pub project_dir: Option<PathBuf>,
    /// 全局规则目录（系统目录不可用时 `None`）
    pub global_dir: Option<PathBuf>,
    pub project_rows: Vec<RuleIndexEntry>,
    pub global_rows: Vec<RuleIndexEntry>,
    /// 内置规则元信息（正文内嵌，界面只需要展示字段）
    pub builtin: Vec<RuleMeta>,
    /// 未信任而未装配的项目规则（见 [`PendingProjectRules`]）
    pub pending: Option<PendingProjectRules>,
    /// 用户此前已明确选择「不加载该项目规则」
    pub trust_declined: bool,
}

/// 内置层 id 集合：索引里出现这些 id 且没有来源文件时，是**抑制记录**而非规则文件。
fn builtin_id_set(builtin: &[RuleMeta]) -> HashSet<String> {
    builtin.iter().map(|m| m.id.clone()).collect()
}

/// 抑制记录判定：id 属于内置层，且**没有来源文件**。
///
/// `source_path` 为空是唯一的判据——同步器写入的抑制记录只带 id 与状态，
/// 而任何真实规则文件都必然有相对路径。反过来，用户完全可以写一个与内置规则同 id 的
/// 项目规则文件（那就是「覆盖」，不是抑制），这种行的 `source_path` 非空，不会被误判。
fn is_suppression(row: &RuleIndexEntry, builtin_ids: &HashSet<String>) -> bool {
    builtin_ids.contains(&row.rule_id) && row.source_path.is_empty()
}

/// 索引行 → 展示行。`dir` 是该层的规则目录（`source_path` 是相对路径）。
fn row_from_index(row: &RuleIndexEntry, dir: Option<&Path>) -> RuleRowView {
    let status = match row.load_status {
        RuleLoadStatus::Ok => RuleRowStatus::Ok,
        RuleLoadStatus::Invalid => RuleRowStatus::Invalid {
            error: row
                .load_error
                .clone()
                .unwrap_or_else(|| "解析失败（未记录错误详情）".to_string()),
        },
        RuleLoadStatus::Missing => RuleRowStatus::Missing,
    };
    // 文件已不在的报告行不给「打开」入口：点了必然失败，不如把状态说清楚
    let file = match (row.load_status, dir, row.source_path.is_empty()) {
        (RuleLoadStatus::Missing, _, _) | (_, None, _) | (_, _, true) => None,
        (_, Some(dir), false) => Some(dir.join(&row.source_path)),
    };
    RuleRowView {
        id: row.rule_id.clone(),
        name: if row.name.trim().is_empty() {
            row.rule_id.clone()
        } else {
            row.name.clone()
        },
        category: row.category.clone(),
        version: row.version.clone(),
        enabled: row.enabled,
        status,
        file,
    }
}

/// 目录是否存在（取数阶段判一次，界面不做 I/O 判定）。
fn dir_exists(dir: &Option<PathBuf>) -> bool {
    dir.as_ref().is_some_and(|d| d.is_dir())
}

/// 组装三组视图数据。
///
/// 顺序与优先级见 [`GROUP_ORDER`]；每层的 `enabled` 都读自索引，
/// 内置层没有索引行，其启停由**抑制记录**反推（`!suppressed`）。
pub fn build_rules_data(input: RuleDataInput) -> RulesData {
    let builtin_ids = builtin_id_set(&input.builtin);
    let suppressed: HashSet<&str> = input
        .project_rows
        .iter()
        .chain(input.global_rows.iter())
        .filter(|row| is_suppression(row, &builtin_ids) && !row.enabled)
        .map(|row| row.rule_id.as_str())
        .collect();

    let groups: Vec<RuleGroupView> = GROUP_ORDER
        .iter()
        .map(|&scope| {
            let (dir, rows): (&Option<PathBuf>, Vec<RuleRowView>) = match scope {
                RuleScope::Project => (
                    &input.project_dir,
                    input
                        .project_rows
                        .iter()
                        .filter(|row| !is_suppression(row, &builtin_ids))
                        .map(|row| row_from_index(row, input.project_dir.as_deref()))
                        .collect(),
                ),
                RuleScope::Global => (
                    &input.global_dir,
                    input
                        .global_rows
                        .iter()
                        .filter(|row| !is_suppression(row, &builtin_ids))
                        .map(|row| row_from_index(row, input.global_dir.as_deref()))
                        .collect(),
                ),
                RuleScope::Builtin => (
                    &None,
                    input
                        .builtin
                        .iter()
                        .map(|meta| RuleRowView {
                            id: meta.id.clone(),
                            name: if meta.name.trim().is_empty() {
                                meta.id.clone()
                            } else {
                                meta.name.clone()
                            },
                            category: meta.category.clone(),
                            version: meta.version.clone(),
                            enabled: !suppressed.contains(meta.id.as_str()),
                            status: RuleRowStatus::Ok,
                            // 正文内嵌在二进制里，没有可打开的文件
                            file: None,
                        })
                        .collect(),
                ),
            };
            RuleGroupView {
                scope,
                title: group_title(scope),
                location: match scope {
                    RuleScope::Builtin => "应用内嵌".to_string(),
                    _ => dir
                        .as_ref()
                        .map(|d| d.display().to_string())
                        .unwrap_or_else(|| "（不可用）".to_string()),
                },
                dir_missing: !matches!(scope, RuleScope::Builtin) && !dir_exists(dir),
                rows,
            }
        })
        .collect();

    let mut total = 0;
    let mut disabled = 0;
    let mut broken = 0;
    for group in &groups {
        for row in &group.rows {
            total += 1;
            if !row.enabled {
                disabled += 1;
            }
            if row.status.is_broken() {
                broken += 1;
            }
        }
    }

    RulesData {
        groups,
        total,
        disabled,
        broken,
        pending: input.pending.as_ref().map(|pending| PendingRulesView {
            ids: pending.ids.clone(),
            invalid: pending.invalid,
            dir: pending.dir.display().to_string(),
            declined: input.trust_declined,
        }),
    }
}