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
