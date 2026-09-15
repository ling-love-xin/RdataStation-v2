//! 模式判定（新建 / 打开文档时决定用哪一档能力）
//!
//! ## 规则优先级（与原型文档 §1.2 一致）
//!
//! 1. **显式记忆**：该文档上次被显式切过模式 → 沿用（含连接绑定，由上层持久化）
//! 2. **扩展名**：见 [`mode_for_extension`]
//! 3. 入口语义（「新建查询」→ SQL、「新建笔记」→ 分析）：由调用方直接给出模式，不经本模块
//!
//! ## 为什么不做「扩展名 + 语言」混合匹配
//!
//! v1 的 `EDITOR_MODE_RULES` 同时匹配扩展名与语言，产生了两个不可达/互相遮蔽的缺陷：
//! `extensions: ['duckdb.sql']` 永远匹配不到（`ext` 只取最后一段，恒为 `sql`，被前一条规则截获），
//! 以及"某规则的语言命中会遮蔽后续规则的扩展名命中"。本模块只按**最后一段扩展名**判定，
//! 规则表是线性的、可穷举测试的。

use std::path::Path;

use engine::sql::split_statements;

use crate::model::EditorMode;

/// 分析笔记的扩展名（自有格式；`.sqlnote` 为兼容别名）
pub const NOTEBOOK_EXTENSIONS: &[&str] = &["rdsnote", "sqlnote"];

/// SQL 脚本的扩展名
pub const SQL_EXTENSIONS: &[&str] = &["sql", "mysql", "pgsql", "psql", "ddl", "tsql"];

/// 按最后一段扩展名判定模式（不含"显式记忆"与"入口语义"两条更高优先级规则）
///
/// 未匹配或没有扩展名 → [`EditorMode::Text`]（安全默认：文本模式不碰数据库）。
pub fn mode_for_extension(path: &Path) -> EditorMode {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if NOTEBOOK_EXTENSIONS.contains(&ext.as_str()) {
        EditorMode::Analysis
    } else if SQL_EXTENSIONS.contains(&ext.as_str()) {
        EditorMode::Sql
    } else {
        EditorMode::Text
    }
}

/// 打开文档时的模式判定：**显式记忆 > 扩展名 > 文本**
pub fn resolve_mode(path: &Path, remembered: Option<EditorMode>) -> EditorMode {
    remembered.unwrap_or_else(|| mode_for_extension(path))
}

// ═══════════════════════════════════════════════════════════════════════
// 切换矩阵（原型设计 §1.3）
// ═══════════════════════════════════════════════════════════════════════

/// SQL → 分析 的单元粒度（由确认对话框二选一）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellGranularity {
    /// 整篇作为一个单元
    Single,
    /// 按语句拆分（一条语句一个单元）
    PerStatement,
}

/// 需要确认的切换类型（视图据此选文案）
///
/// **禁止静默切换**：v1 的 `changeFileType` 直接改字段，三处状态不同步，是本原型要规避的反面案例。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    /// SQL → 文本：隐藏结果（可切回查看）
    HideResults,
    /// SQL（或文本）→ 分析：内容进入单元
    ConvertToCells,
    /// 分析 → SQL / 文本：导出为脚本（输出丢弃）
    ExportToScript,
    /// 分析 → 分析（换会话）：输出全部标 stale
    SwitchSession,
}

/// 切换后内容如何变化
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchContent {
    /// 内容不变（文本 ↔ SQL）
    Unchanged,
    /// 重写为 SQL / 纯脚本文本
    Sql(String),
    /// 拆分为单元（分析模式）
    Cells(Vec<String>),
}

/// 一次模式切换的计划（**纯值**：调用方拿它去弹确认、落内容、记状态栏）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchPlan {
    pub from: EditorMode,
    pub to: EditorMode,
    /// `None` = 可直接切；`Some` = 必须先确认
    pub confirm: Option<ConfirmKind>,
    pub content: SwitchContent,
    /// 状态栏 / 提示用的说明
    pub note: Option<&'static str>,
}

impl SwitchPlan {
    /// 是否需要用户确认
    pub fn needs_confirmation(&self) -> bool {
        self.confirm.is_some()
    }
}

impl CellGranularity {
    /// 粒度选择器的选项文案
    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "整篇作为一个单元",
            Self::PerStatement => "按语句拆分（一条语句一个单元）",
        }
    }
}

impl Default for CellGranularity {
    /// SQL → 分析的默认粒度：按语句拆分（多数脚本是多条语句的集合）
    fn default() -> Self {
        Self::PerStatement
    }
}

/// 对话框文案（**纯函数**：四种确认分支的措辞在这里可穷举断言，不埋在视图里）
///
/// 措辞口径：标题说“要发生什么”，正文说“代价是什么”，按钮说“确认后做什么”——
/// 不用“确定 / 取消”这种看不出后果的通用词。
impl ConfirmKind {
    /// 对话框标题
    pub fn title(self) -> &'static str {
        match self {
            Self::HideResults => "切换到文本模式",
            Self::ConvertToCells => "切换到分析模式",
            Self::ExportToScript => "离开分析模式",
            Self::SwitchSession => "切换会话",
        }
    }

    /// 对话框正文（说明代价；`None` 表示正文由调用方补充）
    pub fn body(self) -> &'static str {
        match self {
            Self::HideResults => "结果仍保留在结果面板，但不再可交互；切回 SQL 模式可继续使用。",
            Self::ConvertToCells => "当前内容将按所选粒度拆分为可执行单元；连接绑定提升为会话默认连接。",
            Self::ExportToScript => "单元结构会被展开为纯脚本，单元输出与过期状态不会保留。",
            Self::SwitchSession => "会话切换意味着变量空间变化：所有单元的输出将标记为过期。",
        }
    }

    /// 确认按钮文案（动宾结构：按下去会发生什么）
    pub fn confirm_label(self) -> &'static str {
        match self {
            Self::HideResults => "切换并隐藏结果",
            Self::ConvertToCells => "拆分并切换",
            Self::ExportToScript => "导出为脚本",
            Self::SwitchSession => "切换会话",
        }
    }

    /// 是否需要用户先选单元粒度（只有“转单元”这一类切换需要）
    pub fn picks_granularity(self) -> bool {
        matches!(self, Self::ConvertToCells)
    }
}

/// 规划一次模式切换（纯函数：不改状态、不弹窗、不做 I/O）
///
/// 对应原型 §1.3 的切换矩阵；矩阵未直接列出的一对（文本 ↔ 分析）按“先到 SQL 再转”
/// 的口径处理，并复用同一套确认与内容变换。
pub fn plan_switch(
    from: EditorMode,
    to: EditorMode,
    content: &str,
    has_results: bool,
    granularity: CellGranularity,
) -> SwitchPlan {
    if from == to {
        return SwitchPlan {
            from,
            to,
            confirm: None,
            content: SwitchContent::Unchanged,
            note: None,
        };
    }

    match (from, to) {
        // 文本 → SQL：无内容影响；连接可以晚点再绑（执行时才要求）
        (EditorMode::Text, EditorMode::Sql) => SwitchPlan {
            from,
            to,
            confirm: None,
            content: SwitchContent::Unchanged,
            note: Some("已启用 SQL 语言服务；执行前需绑定连接"),
        },
        // SQL → 文本：结果不丢，但不再可交互
        (EditorMode::Sql, EditorMode::Text) => SwitchPlan {
            from,
            to,
            confirm: Some(ConfirmKind::HideResults),
            content: SwitchContent::Unchanged,
            note: Some(if has_results {
                "结果仍保留在结果面板，但置灰不可交互"
            } else {
                "当前没有结果需要隐藏"
            }),
        },
        // SQL → 分析：内容进入单元，连接绑定提升为会话默认连接
        (EditorMode::Sql, EditorMode::Analysis) => SwitchPlan {
            from,
            to,
            confirm: Some(ConfirmKind::ConvertToCells),
            content: SwitchContent::Cells(sql_to_cells(content, granularity)),
            note: Some("连接绑定提升为会话的默认连接"),
        },
        // 分析 → SQL：取 SQL 单元拼接为脚本；输出与 stale 不跟随
        (EditorMode::Analysis, EditorMode::Sql) => SwitchPlan {
            from,
            to,
            confirm: Some(ConfirmKind::ExportToScript),
            content: SwitchContent::Sql(cells_to_sql(&cells_from_text(content))),
            note: Some("输出与 stale 状态不会被带过来"),
        },
        // 文本 → 分析：等价于先转 SQL 再转分析（同样需要确认单元粒度）
        (EditorMode::Text, EditorMode::Analysis) => SwitchPlan {
            from,
            to,
            confirm: Some(ConfirmKind::ConvertToCells),
            content: SwitchContent::Cells(sql_to_cells(content, granularity)),
            note: Some("文本将按所选粒度拆为单元（等价于先切 SQL 再切分析）"),
        },
        // 分析 → 文本：单元结构展开为纯脚本，输出丢弃
        (EditorMode::Analysis, EditorMode::Text) => SwitchPlan {
            from,
            to,
            confirm: Some(ConfirmKind::ExportToScript),
            content: SwitchContent::Sql(cells_to_sql(&cells_from_text(content))),
            note: Some("单元结构会被展开为纯脚本，输出丢弃"),
        },
        // 同一模式本应在上面的提前返回里处理：这里只为穷尽性兵底（行为与“不变”一致）
        _ => SwitchPlan {
            from,
            to,
            confirm: None,
            content: SwitchContent::Unchanged,
            note: None,
        },
    }
}

/// 换会话（分析模式内部动作）也要确认：变量空间变化，输出全部 stale
pub fn plan_session_switch() -> SwitchPlan {
    SwitchPlan {
        from: EditorMode::Analysis,
        to: EditorMode::Analysis,
        confirm: Some(ConfirmKind::SwitchSession),
        content: SwitchContent::Unchanged,
        note: Some("切换会话后所有单元输出标记为过期"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 内容变换（纯函数）
// ═══════════════════════════════════════════════════════════════════════

/// 单元在文本层的分隔标记（jupytext 风格）
///
/// `.rdsnote` 的最终落盘格式属 1c（可能是 JSON）：这里只定义**文本层**的等价表示，
/// 供模式切换与“导出为脚本”复用；1c 落地时即便改成 JSON，这两个互转函数仍是导出的实现。
pub const CELL_SEPARATOR: &str = "-- %%";

/// SQL 脚本 → 单元：整篇一个单元，或按**词法级语句切分**（`engine::sql::split`）
pub fn sql_to_cells(sql: &str, granularity: CellGranularity) -> Vec<String> {
    match granularity {
        CellGranularity::Single => {
            let whole = sql.trim();
            if whole.is_empty() {
                Vec::new()
            } else {
                vec![whole.to_string()]
            }
        }
        CellGranularity::PerStatement => split_statements(sql)
            .into_iter()
            .map(|statement| statement.text(sql).trim().to_string())
            .filter(|text| !text.is_empty())
            .collect(),
    }
}

/// 单元 → SQL 脚本：以 `;` 结尾、空行分隔（与格式化器同一口径）
pub fn cells_to_sql(cells: &[String]) -> String {
    let mut bodies: Vec<String> = Vec::with_capacity(cells.len());
    for cell in cells {
        let body = cell.trim().trim_end_matches(';').trim();
        if !body.is_empty() {
            bodies.push(body.to_string());
        }
    }
    if bodies.is_empty() {
        return String::new();
    }

    let mut out = bodies.join(";\n\n");
    out.push(';');
    out
}

/// 文本层 → 单元：按 [`CELL_SEPARATOR`] 切分（无分隔标记时整篇作为一个单元）
pub fn cells_from_text(text: &str) -> Vec<String> {
    let mut cells: Vec<String> = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        if line.trim() == CELL_SEPARATOR {
            cells.push(std::mem::take(&mut current));
            continue;
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }
    cells.push(current);

    cells
        .into_iter()
        .map(|cell| cell.trim().to_string())
        .filter(|cell| !cell.is_empty())
        .collect()
}

/// 单元 → 文本层（与 [`cells_from_text`] 互逆）
pub fn cells_to_text(cells: &[String]) -> String {
    cells
        .iter()
        .map(|cell| cell.trim().to_string())
        .collect::<Vec<String>>()
        .join(&format!("\n\n{CELL_SEPARATOR}\n"))
}

// ═══════════════════════════════════════════════════════════════════════
// 切换矩阵测试（A5 验收：矩阵逐项 + 内容变换）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod switch_tests {
    use super::*;

    fn plan(from: EditorMode, to: EditorMode, content: &str, has_results: bool) -> SwitchPlan {
        plan_switch(from, to, content, has_results, CellGranularity::PerStatement)
    }

    /// 四种确认分支逐一断言文案：标题 / 正文 / 按钮都必须有内容，且**不出现“确定 / 取消”**
    /// 这种看不出后果的通用词（措辞口径见 `ConfirmKind` 文档）。
    #[test]
    fn every_confirmation_has_its_own_wording() {
        let kinds = [
            ConfirmKind::HideResults,
            ConfirmKind::ConvertToCells,
            ConfirmKind::ExportToScript,
            ConfirmKind::SwitchSession,
        ];
        let mut titles = Vec::new();
        for kind in kinds {
            let title = kind.title();
            assert!(!title.is_empty(), "{kind:?} 缺标题");
            assert!(!kind.body().is_empty(), "{kind:?} 缺正文");
            assert!(!kind.confirm_label().is_empty(), "{kind:?} 缺按钮文案");
            for text in [title, kind.body(), kind.confirm_label()] {
                assert!(!text.contains("确定"), "{kind:?} 不许用笼统的“确定”：{text}");
                assert!(!text.contains("取消"), "{kind:?} 的取消由对话框统一提供：{text}");
            }
            assert!(!titles.contains(&title), "标题彼此重复：{title}");
            titles.push(title);
        }
    }

    /// 粒度选择器只在“转单元”那一类出现：别的切换没有粒度可选，不该多问一句
    #[test]
    fn only_conversion_asks_for_granularity() {
        assert!(ConfirmKind::ConvertToCells.picks_granularity());
        assert!(!ConfirmKind::HideResults.picks_granularity());
        assert!(!ConfirmKind::ExportToScript.picks_granularity());
        assert!(!ConfirmKind::SwitchSession.picks_granularity());

        // 默认粒度是按语句拆分（多数脚本多条语句），与两个选项文案都有区分度
        assert_eq!(CellGranularity::default(), CellGranularity::PerStatement);
        assert_ne!(
            CellGranularity::Single.label(),
            CellGranularity::PerStatement.label()
        );
    }

    /// 计划里的确认类型与文案表对得上：免确认的切换不该有文案可言
    #[test]
    fn plan_confirmation_kinds_are_covered_by_wording() {
        // 文本 → SQL 免确认
        assert!(plan(EditorMode::Text, EditorMode::Sql, "select 1", false).confirm.is_none());

        // 需要确认的四个方向都能取到各自文案
        for (from, to) in [
            (EditorMode::Sql, EditorMode::Text),
            (EditorMode::Sql, EditorMode::Analysis),
            (EditorMode::Analysis, EditorMode::Sql),
            (EditorMode::Text, EditorMode::Analysis),
        ] {
            let plan = plan(from, to, "select 1", false);
            let kind = plan.confirm.expect("{from:?} → {to:?} 需要确认");
            assert!(!kind.title().is_empty());
        }
        assert!(plan_session_switch().confirm == Some(ConfirmKind::SwitchSession));
    }

    #[test]
    fn same_mode_needs_no_confirmation() {
        let plan = plan(EditorMode::Sql, EditorMode::Sql, "select 1", true);
        assert!(!plan.needs_confirmation());
        assert_eq!(plan.content, SwitchContent::Unchanged);
    }

    #[test]
    fn text_to_sql_is_free_and_keeps_content() {
        let plan = plan(EditorMode::Text, EditorMode::Sql, "select 1", false);
        assert!(!plan.needs_confirmation(), "文本 → SQL 不需要确认");
        assert_eq!(plan.content, SwitchContent::Unchanged);
        // 连接可以晚点绑：只是执行前需要
        assert!(plan.note.is_some_and(|n| n.contains("连接")));
    }

    #[test]
    fn sql_to_text_warns_only_about_results() {
        let with_results = plan(EditorMode::Sql, EditorMode::Text, "select 1", true);
        assert_eq!(with_results.confirm, Some(ConfirmKind::HideResults));
        assert!(with_results.note.is_some_and(|n| n.contains("置灰")));

        let without = plan(EditorMode::Sql, EditorMode::Text, "select 1", false);
        assert_eq!(without.confirm, Some(ConfirmKind::HideResults));
        assert!(without.note.is_some_and(|n| n.contains("没有结果")));
    }

    #[test]
    fn sql_to_analysis_converts_cells_by_chosen_granularity() {
        let sql = "select 1; select 2;";

        let per_statement = plan_switch(
            EditorMode::Sql,
            EditorMode::Analysis,
            sql,
            false,
            CellGranularity::PerStatement,
        );
        assert_eq!(per_statement.confirm, Some(ConfirmKind::ConvertToCells));
        assert_eq!(
            per_statement.content,
            SwitchContent::Cells(vec!["select 1".to_string(), "select 2".to_string()])
        );

        let single = plan_switch(
            EditorMode::Sql,
            EditorMode::Analysis,
            sql,
            false,
            CellGranularity::Single,
        );
        assert_eq!(
            single.content,
            SwitchContent::Cells(vec![sql.trim().to_string()])
        );
    }

    #[test]
    fn analysis_to_sql_exports_a_script() {
        let cells = vec!["select 1".to_string(), "select 2".to_string()];
        let text = cells_to_text(&cells);
        let plan = plan(EditorMode::Analysis, EditorMode::Sql, &text, false);

        assert_eq!(plan.confirm, Some(ConfirmKind::ExportToScript));
        assert_eq!(plan.content, SwitchContent::Sql("select 1;\n\nselect 2;".to_string()));
        assert!(plan.note.is_some_and(|n| n.contains("stale")));
    }

    #[test]
    fn text_and_analysis_pairs_ask_for_confirmation_too() {
        let up = plan(EditorMode::Text, EditorMode::Analysis, "select 1", false);
        assert_eq!(up.confirm, Some(ConfirmKind::ConvertToCells));
        assert!(matches!(up.content, SwitchContent::Cells(_)));

        let down = plan(EditorMode::Analysis, EditorMode::Text, "select 1", false);
        assert_eq!(down.confirm, Some(ConfirmKind::ExportToScript));
        assert_eq!(down.content, SwitchContent::Sql("select 1;".to_string()));
    }

    #[test]
    fn session_switch_is_confirmed_and_marks_outputs_stale() {
        let plan = plan_session_switch();
        assert_eq!(plan.confirm, Some(ConfirmKind::SwitchSession));
        assert!(plan.note.is_some_and(|n| n.contains("过期")));
    }

    #[test]
    fn statement_split_does_not_break_inside_literals() {
        // 词法级切分（engine::sql::split）：字符串里的分号不是语句边界
        let cells = sql_to_cells("select ';' as x; select 2", CellGranularity::PerStatement);
        assert_eq!(cells, vec!["select ';' as x".to_string(), "select 2".to_string()]);
    }

    #[test]
    fn empty_input_produces_no_cells() {
        assert!(sql_to_cells("   \n", CellGranularity::PerStatement).is_empty());
        assert!(sql_to_cells("   \n", CellGranularity::Single).is_empty());
        assert!(cells_to_sql(&[]).is_empty());
    }

    #[test]
    fn cell_text_round_trips() {
        let cells = vec![
            "-- 口径说明\nselect 1".to_string(),
            "select 2".to_string(),
        ];
        let text = cells_to_text(&cells);
        assert!(text.contains(CELL_SEPARATOR));
        assert_eq!(cells_from_text(&text), cells, "文本层与单元序列应可互转");
    }

    #[test]
    fn text_without_separator_is_one_cell() {
        assert_eq!(
            cells_from_text("select 1\nselect 2"),
            vec!["select 1\nselect 2".to_string()]
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode_of(path: &str) -> EditorMode {
        mode_for_extension(Path::new(path))
    }

    #[test]
    fn notebook_extensions_map_to_analysis() {
        assert_eq!(mode_of("笔记.rdsnote"), EditorMode::Analysis);
        assert_eq!(mode_of("分析.rdsnote"), EditorMode::Analysis);
        assert_eq!(mode_of("/tmp/a/b.sqlnote"), EditorMode::Analysis);
    }

    #[test]
    fn sql_extensions_map_to_sql() {
        for path in [
            "query.sql",
            "q.mysql",
            "q.pgsql",
            "q.psql",
            "schema.ddl",
            "q.tsql",
            "C:/work/deep/dir/report.SQL",
        ] {
            assert_eq!(mode_of(path), EditorMode::Sql, "{path}");
        }
    }

    #[test]
    fn other_and_unknown_extensions_are_text() {
        for path in [
            "notes.md",
            "data.csv",
            "app.log",
            "config.json",
            "no_extension",
            ".gitignore",
            "a/b/",
        ] {
            assert_eq!(mode_of(path), EditorMode::Text, "{path}");
        }
    }

    #[test]
    fn only_the_last_extension_decides() {
        // 刻意不做 v1 的 `duckdb.sql` 特例：末段扩展名是 sql → SQL 模式。
        // （v1 该规则因"只取最后一段扩展名"而永不可达，此处用测试固定新口径。）
        assert_eq!(mode_of("analysis.duckdb.sql"), EditorMode::Sql);
    }

    #[test]
    fn remembered_mode_wins_over_extension() {
        let path = Path::new("query.sql");
        assert_eq!(mode_for_extension(path), EditorMode::Sql);
        assert_eq!(
            resolve_mode(path, Some(EditorMode::Text)),
            EditorMode::Text,
            "显式记忆优先于扩展名"
        );
        assert_eq!(resolve_mode(path, None), EditorMode::Sql);

        let note = Path::new("笔记.rdsnote");
        assert_eq!(
            resolve_mode(note, Some(EditorMode::Sql)),
            EditorMode::Sql
        );
    }

    #[test]
    fn unknown_paths_default_to_safe_text_mode() {
        assert_eq!(resolve_mode(Path::new("unknown.bin"), None), EditorMode::Text);
    }
}
