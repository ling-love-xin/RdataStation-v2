//! MySQL 方言 provider（B3 L3）：反引号引号；表 / 视图统一 `RENAME TABLE`（无独立 `ALTER VIEW RENAME`）。

use engine::ObjectKind;
use shared::error::CoreError;

use super::{DialectRegistry, DialectSpi, unsupported_err};

pub(crate) struct MysqlDialect;

impl DialectSpi for MysqlDialect {
    fn family(&self) -> &'static str {
        "mysql"
    }

    /// MySQL 用反引号包裹标识符；内嵌反引号双写（现状双引号为欠账，B3 补齐）。
    fn quote_ident(&self, ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }

    fn rename_sql(
        &self,
        kind: ObjectKind,
        schema: Option<&str>,
        old: &str,
        _parent: Option<&str>,
        new: &str,
    ) -> Result<String, CoreError> {
        let new_ident = self.quote_ident(new);
        // 库即 Schema：schema 非空时拼 `schema.name`。
        let qual = |n: &str| match schema {
            Some(s) if !s.is_empty() => format!("{}.{}", self.quote_ident(s), self.quote_ident(n)),
            _ => self.quote_ident(n),
        };
        match kind {
            // MySQL 表与视图都走 `RENAME TABLE`：8.0 起 `ALTER TABLE … RENAME` 亦可，
            // 但 `RENAME TABLE` 对视图同样有效，两态一语句（沿用现状口径）。
            ObjectKind::Table | ObjectKind::View => Ok(format!(
                "RENAME TABLE {} TO {new_ident}",
                qual(old)
            )),
            ObjectKind::Sequence => Err(unsupported_err(
                self.family(),
                "rename_sql",
                "序列重命名（MySQL 自增无独立序列对象）",
            )),
            ObjectKind::Trigger => Err(unsupported_err(
                self.family(),
                "rename_sql",
                "触发器重命名（MySQL 无独立触发器改名语句）",
            )),
            ObjectKind::Routine => Err(unsupported_err(
                self.family(),
                "rename_sql",
                "例程重命名需函数签名，暂不支持",
            )),
            other => Err(unsupported_err(
                self.family(),
                "rename_sql",
                format!("该类型 {other:?} 暂不支持对象重命名"),
            )),
        }
    }

    fn drop_sql(
        &self,
        kind: ObjectKind,
        schema: Option<&str>,
        name: &str,
        _parent: Option<&str>,
    ) -> Result<String, CoreError> {
        // 库即 Schema：schema 非空时拼 `schema.name`。
        let qual = |n: &str| match schema {
            Some(s) if !s.is_empty() => format!("{}.{}", self.quote_ident(s), self.quote_ident(n)),
            _ => self.quote_ident(n),
        };
        match kind {
            ObjectKind::Table => Ok(format!("DROP TABLE {}", qual(name))),
            ObjectKind::View => Ok(format!("DROP VIEW {}", qual(name))),
            ObjectKind::Sequence => Err(unsupported_err(
                self.family(),
                "drop_sql",
                "删除序列（MySQL 自增无独立序列对象）",
            )),
            other => Err(unsupported_err(
                self.family(),
                "drop_sql",
                format!("该类型 {other:?} 暂不支持删除"),
            )),
        }
    }

    fn truncate_table_sql(&self, schema: Option<&str>, name: &str) -> Result<String, CoreError> {
        let qual = match schema {
            Some(s) if !s.is_empty() => format!("{}.{}", self.quote_ident(s), self.quote_ident(name)),
            _ => self.quote_ident(name),
        };
        Ok(format!("TRUNCATE TABLE {qual}"))
    }

    /// 目录级行估计：`information_schema.TABLES.TABLE_ROWS`（InnoDB 为优化器估计、
    /// MyISAM 近似精确，v1 不区分引擎口径）。
    fn row_estimate_sql(&self, schema: &str) -> Option<String> {
        let esc = schema.replace('\'', "''");
        Some(format!(
            "SELECT TABLE_NAME, TABLE_ROWS FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = '{esc}'"
        ))
    }

    /// 目录级表大小：`information_schema.TABLES.DATA_LENGTH`（仅数据段，不含索引；
    /// 视图行为 NULL，任务层解析为 `None` 后徽标安静消失）。
    fn table_size_sql(&self, schema: &str) -> Option<String> {
        let esc = schema.replace('\'', "''");
        Some(format!(
            "SELECT TABLE_NAME, DATA_LENGTH FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = '{esc}'"
        ))
    }
}

pub(crate) fn register(registry: &mut DialectRegistry) {
    registry.register(Box::new(MysqlDialect));
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::ObjectKind;

    #[test]
    fn quote_ident_uses_backticks_and_doubles_embedded() {
        let d = MysqlDialect;
        assert_eq!(d.quote_ident("orders"), "`orders`");
        assert_eq!(d.quote_ident("o`rder`s"), "`o``rder``s`");
    }

    #[test]
    fn rename_table_and_view_both_use_rename_table() {
        let d = MysqlDialect;
        assert_eq!(
            d.rename_sql(ObjectKind::Table, Some("biz"), "t_user", None, "t_account")
                .unwrap(),
            "RENAME TABLE `biz`.`t_user` TO `t_account`"
        );
        // MySQL 无独立 ALTER VIEW RENAME，视图复用 RENAME TABLE。
        assert_eq!(
            d.rename_sql(ObjectKind::View, Some("biz"), "v_active", None, "v_enabled")
                .unwrap(),
            "RENAME TABLE `biz`.`v_active` TO `v_enabled`"
        );
    }

    #[test]
    fn rename_rejects_sequence_trigger_and_routine() {
        let d = MysqlDialect;
        let seq = d
            .rename_sql(ObjectKind::Sequence, Some("biz"), "seq_id", None, "seq_uid")
            .expect_err("MySQL 无独立序列对象，应拒绝");
        assert!(seq.to_string().contains("序列重命名"), "{seq}");
        let trig = d
            .rename_sql(ObjectKind::Trigger, Some("biz"), "trg", Some("t"), "trg2")
            .expect_err("MySQL 触发器改名应拒绝");
        assert!(trig.to_string().contains("触发器重命名"), "{trig}");
        let routine = d
            .rename_sql(ObjectKind::Routine, Some("biz"), "sp_do", None, "sp_run")
            .expect_err("例程需签名，应拒绝");
        assert!(routine.to_string().contains("暂不支持"), "{routine}");
    }

    #[test]
    fn row_estimate_uses_information_schema_and_escapes_schema() {
        let d = MysqlDialect;
        let sql = d.row_estimate_sql("warehouse").unwrap();
        assert!(sql.contains("TABLE_ROWS"), "{sql}");
        assert!(sql.contains("'warehouse'"), "{sql}");
        let escaped = d.row_estimate_sql("it's").unwrap();
        assert!(escaped.contains("'it''s'"), "{escaped}");
    }

    #[test]
    fn table_size_uses_data_length_and_escapes_schema() {
        let d = MysqlDialect;
        let sql = d.table_size_sql("warehouse").unwrap();
        assert!(sql.contains("DATA_LENGTH"), "{sql}");
        assert!(sql.contains("'warehouse'"), "{sql}");
        let escaped = d.table_size_sql("it's").unwrap();
        assert!(escaped.contains("'it''s'"), "{escaped}");
    }

    #[test]
    fn drop_table_and_view_use_backtick_schema_prefix() {
        let d = MysqlDialect;
        assert_eq!(
            d.drop_sql(ObjectKind::Table, Some("biz"), "t_user", None)
                .unwrap(),
            "DROP TABLE `biz`.`t_user`"
        );
        assert_eq!(
            d.drop_sql(ObjectKind::View, Some("biz"), "v_active", None)
                .unwrap(),
            "DROP VIEW `biz`.`v_active`"
        );
    }

    #[test]
    fn drop_rejects_sequence_and_non_drop_types() {
        let d = MysqlDialect;
        let seq = d
            .drop_sql(ObjectKind::Sequence, Some("biz"), "seq_id", None)
            .expect_err("MySQL 无独立序列对象，应拒绝删除序列");
        assert!(seq.to_string().contains("序列"), "{seq}");
        for kind in [ObjectKind::Routine, ObjectKind::Trigger, ObjectKind::Schema] {
            assert!(
                d.drop_sql(kind, Some("biz"), "x", None).is_err(),
                "{kind:?} 删除应拒绝"
            );
        }
    }

    #[test]
    fn truncate_table_uses_backtick_schema_prefix() {
        let d = MysqlDialect;
        assert_eq!(
            d.truncate_table_sql(Some("biz"), "t_user").unwrap(),
            "TRUNCATE TABLE `biz`.`t_user`"
        );
        assert_eq!(
            d.truncate_table_sql(None, "t_user").unwrap(),
            "TRUNCATE TABLE `t_user`"
        );
    }

    #[test]
    fn drop_escapes_embedded_backticks() {
        let d = MysqlDialect;
        let sql = d
            .drop_sql(ObjectKind::Table, Some("biz"), "o`rder`s", None)
            .unwrap();
        assert_eq!(sql, "DROP TABLE `biz`.`o``rder``s`");
    }
}
