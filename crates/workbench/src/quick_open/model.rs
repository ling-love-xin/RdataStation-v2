//! Quick Open 的**纯逻辑**：查询解析（模式前缀）、命令目录、行匹配与评分、查询词转义。
//!
//! 不依赖 GPUI / `Shared`，输入输出都是纯数据，因此可直接单测
//! （`cargo test -p rds-workbench --lib quick_open`）。
//! 渲染与副作用在 `quick_open/delegate.rs` 与 `view.rs`（宿主 render / 事件路径）。
//!
//! 规格：`docs/architecture/quick_open/quick-open-prototype-design.md`（§5 模式 / §6 元数据两档 / §8 匹配排序）。

use editor::model::EditorMode;

use crate::view::{LeftPanel, RightPanel};

/// 搜索模式：由输入的首字符决定（对齐 VSCode 的 Quick Open / Command Palette）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// 默认：连接 + 命令（+ 元数据 / 文件，Phase 1 接入）。
    Default,
    /// `>`：仅命令。
    Command,
    /// `#`：元数据全文档（注释 / 类型 / 定义）——Phase 1 接线 FTS 后才会有结果。
    FullText,
}

/// 异步元数据搜索的最小词长（本地源不受限；对齐导航搜索框的口径）。
pub(crate) const MIN_NEEDLE_LEN: usize = 2;

/// 解析后的查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Query {
    pub mode: Mode,
    /// 去掉前缀与首尾空白后的词（**保留大小写**：高亮要按原文定位）。
    pub needle: String,
}

impl Query {
    /// 词长是否够发异步元数据搜索（本地源始终可搜）。
    pub(crate) fn async_ready(&self) -> bool {
        self.mode != Mode::Command && self.needle.chars().count() >= MIN_NEEDLE_LEN
    }
}

/// 按首字符判定模式。只认**半角**符号（全角 `＞` / `＃` 当普通搜索词，避免输入法误触发）。
pub(crate) fn parse(raw: &str) -> Query {
    let t = raw.trim_start();
    if let Some(rest) = t.strip_prefix('>') {
        Query {
            mode: Mode::Command,
            needle: rest.trim().to_string(),
        }
    } else if let Some(rest) = t.strip_prefix('#') {
        Query {
            mode: Mode::FullText,
            needle: rest.trim().to_string(),
        }
    } else {
        Query {
            mode: Mode::Default,
            needle: t.trim().to_string(),
        }
    }
}

/// 行类别（决定类型标签；图标集不在第一批扩，缺则用文本标签）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowKind {
    Connection,
    Command,
}

impl RowKind {
    /// 行首的类型短标签（类型双通道：文本 + 位置，不只靠颜色）。
    pub(crate) fn label(self) -> &'static str {
        match self {
            RowKind::Connection => "连接",
            RowKind::Command => "命令",
        }
    }
}

/// 行被确认（↵ / 点击 / Ctrl+↵）时要执行的动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    /// 新建编辑器文档（查询 / 笔记 / 文件）。
    NewDocument(EditorMode),
    /// 打开左侧面板。
    OpenLeftPanel(LeftPanel),
    /// 打开右侧面板。
    OpenRightPanel(RightPanel),
    /// 打开设置页。
    OpenSettings,
    /// 完全隐藏两侧边栏。
    HideSidebars,
    /// 恢复两侧边栏（还原隐藏前的模式）。
    RestoreSidebars,
    /// 选中数据源连接（切到导航并选中；**不自动连接**）。
    SelectConnection(usize),
}

/// 一行结果。
#[derive(Debug, Clone)]
pub(crate) struct Row {
    /// 业务键（选中跨重算跟随；**不用下标**）。
    pub key: String,
    pub kind: RowKind,
    /// 主文本（不含高亮标记；高亮区间在渲染期按查询词现算）。
    pub title: String,
    /// 右侧次级信息（连接：驱动；命令：快捷键）。
    pub secondary: String,
    pub action: Action,
}

/// 一个分组（渲染为 `List` 的一个 section）。
#[derive(Debug, Clone)]
pub(crate) struct Group {
    pub title: &'static str,
    pub rows: Vec<Row>,
}

/// 命令表：Quick Open 也是命令面（`>` 前缀只留这一组）。
///
/// 这是**当前唯一权威**的命令清单（原先硬编码在 `view.rs`）；Phase 1 迁到命令注册后，
/// 各 Feature 通过宿主端口登记自己的命令。
pub(crate) fn command_rows() -> Vec<Row> {
    let mut out = Vec::new();
    let mut push = |label: &str, shortcut: &str, action: Action| {
        out.push(Row {
            key: format!("cmd:{label}"),
            kind: RowKind::Command,
            title: label.to_string(),
            secondary: shortcut.to_string(),
            action,
        });
    };
    // 新建入口放最前：Quick Open 是工作台的命令面（Ctrl+P），“新建”是最常敲的一条。
    push("新建查询", "Ctrl+N", Action::NewDocument(EditorMode::Sql));
    push("新建笔记", "", Action::NewDocument(EditorMode::Analysis));
    push("新建文件", "", Action::NewDocument(EditorMode::Text));
    push("打开草稿箱", "", Action::OpenLeftPanel(LeftPanel::Draft));
    push("打开数据库导航", "", Action::OpenLeftPanel(LeftPanel::Database));
    push("打开资产库", "", Action::OpenLeftPanel(LeftPanel::Resources));
    push("打开插件", "", Action::OpenLeftPanel(LeftPanel::Plugin));
    push("打开洞察", "", Action::OpenRightPanel(RightPanel::Insight));
    push("打开 Mock 生成", "", Action::OpenRightPanel(RightPanel::Mock));
    push("打开历史", "", Action::OpenRightPanel(RightPanel::History));
    push("打开设置", "Ctrl+,", Action::OpenSettings);
    push("完全隐藏侧边栏", "", Action::HideSidebars);
    push("恢复侧边栏", "", Action::RestoreSidebars);
    out
}

/// 组装结果分组：**空组不出现**（组头也不渲染）。
///
/// 元数据（跨连接名称档）与文件源在 Phase 1 接入——接的是后台索引搜索，
/// 本函数是纯函数，不做 I/O。
pub(crate) fn build_groups(q: &Query, connections: &[(String, String)]) -> Vec<Group> {
    match q.mode {
        Mode::Command => non_empty("命令", filter_ranked(command_rows(), &q.needle)).into_iter().collect(),
        // Phase 1：`metadata_fts` 接线后填全文档命中（注释 / 类型 / 定义 + snippet）。
        Mode::FullText => Vec::new(),
        Mode::Default => {
            let mut groups = Vec::new();
            groups.extend(non_empty(
                "连接",
                filter_ranked(connection_rows(connections), &q.needle),
            ));
            groups.extend(non_empty("命令", filter_ranked(command_rows(), &q.needle)));
            groups
        }
    }
}

fn non_empty(title: &'static str, rows: Vec<Row>) -> Option<Group> {
    if rows.is_empty() {
        None
    } else {
        Some(Group { title, rows })
    }
}

/// 连接行（下标即 `Shared::selected` 的位置）。
fn connection_rows(connections: &[(String, String)]) -> Vec<Row> {
    connections
        .iter()
        .enumerate()
        .map(|(ix, (name, driver))| Row {
            key: format!("conn:{ix}:{name}"),
            kind: RowKind::Connection,
            title: name.clone(),
            secondary: driver.clone(),
            action: Action::SelectConnection(ix),
        })
        .collect()
}

/// 过滤 + 排序：先按匹配档，再按命中长度、标题长度，最后按标题字典序（稳定）。
fn filter_ranked(rows: Vec<Row>, needle: &str) -> Vec<Row> {
    let mut hits: Vec<(Rank, Row)> = rows
        .into_iter()
        .filter_map(|row| rank(&row.title, needle).map(|r| (r, row)))
        .collect();
    hits.sort_by(|(a, ra), (b, rb)| {
        a.tier
            .cmp(&b.tier)
            .then(a.span_len.cmp(&b.span_len))
            .then_with(|| ra.title.len().cmp(&rb.title.len()))
            .then_with(|| ra.title.cmp(&rb.title))
    });
    hits.into_iter().map(|(_, row)| row).collect()
}

/// 匹配档（越小越优先）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Tier {
    /// 完全相同。
    Exact,
    /// 名称前缀。
    Prefix,
    /// 词首命中（`snake_case` / `camelCase` 分段首字母）。
    WordStart,
    /// 子串命中（同档内按位置靠前）。
    Substring,
}

/// 一次命中的排序信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rank {
    pub tier: Tier,
    /// 命中区间长度（同档内短者优先）。
    pub span_len: usize,
    /// 命中起点（同档内靠前者优先）。
    pub span_start: usize,
}

/// 纯排序信息（不含高亮区间计算）。
fn rank(title: &str, needle: &str) -> Option<Rank> {
    let span = match_span(title, needle)?;
    let tier = if needle.is_empty() {
        Tier::Substring
    } else if title.eq_ignore_ascii_case(needle) {
        Tier::Exact
    } else if span.0 == 0 {
        Tier::Prefix
    } else if is_word_start(title, span.0) {
        Tier::WordStart
    } else {
        Tier::Substring
    };
    Some(Rank {
        tier,
        span_len: span.1.saturating_sub(span.0),
        span_start: span.0,
    })
}

/// 命中区间（字节范围；空词返回 `(0, 0)`）。
pub(crate) fn match_span(title: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return Some((0, 0));
    }
    // 大小写折叠只按 ASCII 生效；折叠后长度变化（罕见 Unicode）时放弃定位，退化为「整串可搜」。
    let (hay, ndl) = (title.to_lowercase(), needle.to_lowercase());
    if hay.len() != title.len() || ndl.len() != needle.len() {
        return if title.contains(needle) {
            Some((0, 0))
        } else {
            None
        };
    }
    hay.find(&ndl).map(|pos| (pos, pos + ndl.len()))
}

/// 命中位置是否落在某个分段的词首（`order_items` 的 `items`、`orderItems` 的 `Items`）。
fn is_word_start(title: &str, pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    let prev = title[..pos].chars().next_back();
    let cur = title[pos..].chars().next();
    match (prev, cur) {
        (Some(p), Some(c)) => {
            p == '_' || p == '-' || p == ' ' || p == '.' || (p.is_lowercase() && c.is_uppercase())
        }
        _ => false,
    }
}

/// `LIKE` 通配符转义与 FTS5 `MATCH` 词清洗**不在这里**：
/// - `LIKE` 的转义已在写侧（`engine::persistence::MetadataCacheOps::like_escape`，带单测）；
/// - FTS 的词清洗归 Phase 1 接线的 `search_fts`（现状是 `format!("{}*", q)` 裸拼，
///   会被用户输入里的 `"` / `*` / `NEAR` 破坏语法）——修在它自己的 crate，避免两处各写一套。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_mode_from_first_char_only() {
        assert_eq!(parse("order").mode, Mode::Default);
        assert_eq!(parse("order").needle, "order");
        assert_eq!(parse(">新建").mode, Mode::Command);
        assert_eq!(parse(">新建").needle, "新建");
        assert_eq!(parse("> 新建 查询 ").needle, "新建 查询");
        assert_eq!(parse("#渠道").mode, Mode::FullText);
        // 前缀不是首字符 → 当普通词
        assert_eq!(parse("订单 > 明细").mode, Mode::Default);
        assert_eq!(parse("   >撤销").mode, Mode::Command);
        // 全角符号不当前缀（输入法误触发防护）
        assert_eq!(parse("＞撤销").mode, Mode::Default);
    }

    #[test]
    fn async_ready_requires_two_chars_and_non_command_mode() {
        assert!(!parse("o").async_ready());
        assert!(parse("or").async_ready());
        assert!(!parse("#渠").async_ready());
        assert!(parse("#渠道").async_ready());
        assert!(!parse(">新建").async_ready());
        assert!(!parse(">").async_ready());
        // 中文按字符计数，不按字节
        assert!(parse("订单").async_ready());
    }

    #[test]
    fn match_span_reports_byte_range_and_tolerates_non_ascii_case() {
        assert_eq!(match_span("order_items", "items"), Some((6, 11)));
        assert_eq!(match_span("OrderItems", "order"), Some((0, 5)));
        assert_eq!(match_span("订单明细", "明细"), Some((6, 12)));
        assert_eq!(match_span("orders", ""), Some((0, 0)));
        assert_eq!(match_span("orders", "zzz"), None);
        // 非 ASCII 大小写折叠后长度变化 → 放弃定位
        assert_eq!(match_span("İstanbul", "istanbul"), None);
    }

    #[test]
    fn rank_prefers_exact_then_prefix_then_word_start_then_substring() {
        let ranked = |needle: &str| {
            filter_ranked(
                ["order_items", "pre_order", "order", "order_id"]
                    .iter()
                    .map(|t| Row {
                        key: (*t).to_string(),
                        kind: RowKind::Command,
                        title: (*t).to_string(),
                        secondary: String::new(),
                        action: Action::OpenSettings,
                    })
                    .collect(),
                needle,
            )
            .into_iter()
            .map(|r| r.title)
            .collect::<Vec<_>>()
        };
        assert_eq!(
            ranked("order"),
            vec!["order", "order_id", "order_items", "pre_order"]
        );
        // 词首档：`items` 只命中 order_items（下划线之后）
        assert_eq!(ranked("items"), vec!["order_items"]);
        // 空词 → 全部保留（排序退化为长度 + 字典序）
        assert_eq!(ranked("").len(), 4);
    }

    #[test]
    fn build_groups_filters_and_drops_empty_groups() {
        let conns = vec![
            ("营销分析（生产）".to_string(), "postgres".to_string()),
            ("分析库".to_string(), "duckdb".to_string()),
        ];
        // 命令模式：只有命令组
        let cmd = build_groups(&parse(">设置"), &conns);
        assert_eq!(cmd.len(), 1);
        assert_eq!(cmd[0].title, "命令");
        assert!(cmd[0].rows.iter().all(|r| r.title.contains("设置")));

        // 默认模式：命中连接则不出现命令组
        let hit_conn = build_groups(&parse("分析库"), &conns);
        assert_eq!(hit_conn.len(), 1);
        assert_eq!(hit_conn[0].title, "连接");
        assert_eq!(hit_conn[0].rows[0].action, Action::SelectConnection(1));

        // `#` 全文档：Phase 1 前恒为空（渲染走空态说明）
        assert!(build_groups(&parse("#渠道"), &conns).is_empty());

        // 无匹配：一个组都没有
        assert!(build_groups(&parse("zzzz"), &conns).is_empty());
    }

    #[test]
    fn command_catalog_keys_are_stable_and_unique() {
        let rows = command_rows();
        let mut keys: Vec<&String> = rows.iter().map(|r| &r.key).collect();
        let total = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), total, "命令键必须唯一（选中跟随依赖它）");
        assert_eq!(rows[0].action, Action::NewDocument(EditorMode::Sql));
    }
}
