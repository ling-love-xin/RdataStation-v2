//! PostgreSQL 方言 provider（B3 L3）：双引号 + 独立 Schema 前缀；序列 / 触发器 / 表 / 视图改名。

use engine::ObjectKind;
use shared::error::CoreError;

use super::{DialectRegistry, DialectSpi, unsupported_err};

pub(crate) struct PostgresqlDialect;

impl DialectSpi for PostgresqlDialect {
    fn family(&self) -> &'static str {
        "postgresql"
    }

    fn rename_sql(
        &self,
        kind: ObjectKind,
        schema: Option<&str>,
        old: &str,
        parent: Option<&str>,
        new: &str,
    ) -> Result<String, CoreError> {
        let new_ident = self.quote_ident(new);
        // PG 有独立 Schema 层：schema 非空时拼 `schema.name`。
        let qual = |n: &str| match schema {
            Some(s) if !s.is_empty() => format!("{}.{}", self.quote_ident(s), self.quote_ident(n)),
            _ => self.quote_ident(n),
        };
        match kind {
            ObjectKind::Table => Ok(format!(
                "ALTER TABLE {} RENAME TO {new_ident}",
                qual(old)
            )),
            ObjectKind::View => Ok(format!(
                "ALTER VIEW {} RENAME TO {new_ident}",
                qual(old)
            )),
            ObjectKind::Sequence => Ok(format!(
                "ALTER SEQUENCE {} RENAME TO {new_ident}",
                qual(old)
            )),
            // 触发器改名需父表限定名：`ALTER TRIGGER <名> ON <父表> RENAME TO …`。
            ObjectKind::Trigger => {
                let Some(parent) = parent else {
                    return Err(unsupported_err(
                        self.family(),
                        "rename_sql",
                        "触发器重命名需要父表限定名（parent 缺失）",
                    ));
                };
                Ok(format!(
                    "ALTER TRIGGER {} ON {} RENAME TO {new_ident}",
                    self.quote_ident(old),
                    qual(parent)
                ))
            }
            // 例程改名需函数签名（`ALTER FUNCTION name(args) RENAME TO`），参数签名采集挂 D24 后续。
            ObjectKind::Routine => Err(unsupported_err(
                self.family(),
                "rename_sql",
                "例程重命名需函数签名（PG `ALTER FUNCTION name(args) RENAME TO`），暂不支持",
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
        // PG 有独立 Schema 层：schema 非空时拼 `schema.name`。
        let qual = |n: &str| match schema {
            Some(s) if !s.is_empty() => format!("{}.{}", self.quote_ident(s), self.quote_ident(n)),
            _ => self.quote_ident(n),
        };
        match kind {
            ObjectKind::Table => Ok(format!("DROP TABLE {}", qual(name))),
            ObjectKind::View => Ok(format!("DROP VIEW {}", qual(name))),
            ObjectKind::Sequence => Ok(format!("DROP SEQUENCE {}", qual(name))),
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

    /// 目录级行估计：`pg_class.reltuples`，`relkind IN ('r','p')` 覆盖普通表与分区父表。
    fn row_estimate_sql(&self, schema: &str) -> Option<String> {
        let esc = schema.replace('\'', "''");
        Some(format!(
            "SELECT c.relname, c.reltuples::bigint AS est FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = '{esc}' AND c.relkind IN ('r','p')"
        ))
    }

    /// 目录级表大小：`pg_total_relation_size(oid)`（整表占用，含索引与 toast；
    /// 与 MySQL `DATA_LENGTH` 仅数据段的口径差异挂账，跨族大小对比不做严格等价）。
    fn table_size_sql(&self, schema: &str) -> Option<String> {
        let esc = schema.replace('\'', "''");
        Some(format!(
            "SELECT c.relname, pg_total_relation_size(c.oid) FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = '{esc}' AND c.relkind IN ('r','p')"
        ))
    }
}

pub(crate) fn register(registry: &mut DialectRegistry) {
    registry.register(Box::new(PostgresqlDialect));
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::ObjectKind;

    #[test]
    fn rename_table_view_sequence_with_schema_prefix() {
        let d = PostgresqlDialect;
        assert_eq!(
            d.rename_sql(ObjectKind::Table, Some("public"), "t_user", None, "t_account")
                .unwrap(),
            "ALTER TABLE \"public\".\"t_user\" RENAME TO \"t_account\""
        );
        assert_eq!(
            d.rename_sql(ObjectKind::View, Some("public"), "v_active", None, "v_enabled")
                .unwrap(),
            "ALTER VIEW \"public\".\"v_active\" RENAME TO \"v_enabled\""
        );
        assert_eq!(
            d.rename_sql(ObjectKind::Sequence, Some("public"), "seq_id", None, "seq_uid")
                .unwrap(),
            "ALTER SEQUENCE \"public\".\"seq_id\" RENAME TO \"seq_uid\""
        );
    }

    #[test]
    fn rename_trigger_needs_parent_table() {
        let d = PostgresqlDialect;
        let sql = d
            .rename_sql(
                ObjectKind::Trigger,
                Some("public"),
                "trg_audit",
                Some("t_user"),
                "trg_audit_v2",
            )
            .unwrap();
        assert_eq!(
            sql,
            "ALTER TRIGGER \"trg_audit\" ON \"public\".\"t_user\" RENAME TO \"trg_audit_v2\""
        );
        let err = d
            .rename_sql(ObjectKind::Trigger, Some("public"), "trg_audit", None, "trg_audit_v2")
            .expect_err("缺父表应拒绝");
        assert!(err.to_string().contains("父表"), "{err}");
    }

    #[test]
    fn rename_rejects_routine_and_non_addressable() {
        let d = PostgresqlDialect;
        let err = d
            .rename_sql(ObjectKind::Routine, Some("public"), "sp_do", None, "sp_run")
            .expect_err("例程需签名，应拒绝");
        assert!(err.to_string().contains("暂不支持"), "{err}");
        assert!(d.rename_sql(ObjectKind::Schema, Some("public"), "s", None, "s2").is_err());
    }

    #[test]
    fn rename_escapes_embedded_quotes() {
        let d = PostgresqlDialect;
        let sql = d
            .rename_sql(ObjectKind::Table, Some("public"), "we\"ird", None, "new\"name")
            .unwrap();
        assert_eq!(
            sql,
            "ALTER TABLE \"public\".\"we\"\"ird\" RENAME TO \"new\"\"name\""
        );
    }

    #[test]
    fn row_estimate_uses_pg_class_and_escapes_schema() {
        let d = PostgresqlDialect;
        let sql = d.row_estimate_sql("public").unwrap();
        assert!(sql.contains("reltuples"), "{sql}");
        assert!(sql.contains("relkind IN ('r','p')"), "{sql}");
        assert!(sql.contains("'public'"), "{sql}");
        let escaped = d.row_estimate_sql("a'b").unwrap();
        assert!(escaped.contains("'a''b'"), "{escaped}");
    }

    #[test]
    fn table_size_uses_pg_total_relation_size_and_escapes_schema() {
        let d = PostgresqlDialect;
        let sql = d.table_size_sql("public").unwrap();
        assert!(sql.contains("pg_total_relation_size"), "{sql}");
        assert!(sql.contains("relkind IN ('r','p')"), "{sql}");
        assert!(sql.contains("'public'"), "{sql}");
        let escaped = d.table_size_sql("a'b").unwrap();
        assert!(escaped.contains("'a''b'"), "{escaped}");
    }

    #[test]
    fn drop_table_view_sequence_with_schema_prefix() {
        let d = PostgresqlDialect;
        assert_eq!(
            d.drop_sql(ObjectKind::Table, Some("public"), "t_user", None)
                .unwrap(),
            "DROP TABLE \"public\".\"t_user\""
        );
        assert_eq!(
            d.drop_sql(ObjectKind::View, Some("public"), "v_active", None)
                .unwrap(),
            "DROP VIEW \"public\".\"v_active\""
        );
        assert_eq!(
            d.drop_sql(ObjectKind::Sequence, Some("public"), "seq_id", None)
                .unwrap(),
            "DROP SEQUENCE \"public\".\"seq_id\""
        );
    }

    #[test]
    fn drop_rejects_non_drop_types() {
        let d = PostgresqlDialect;
        for kind in [
            ObjectKind::Routine,
            ObjectKind::Trigger,
            ObjectKind::Schema,
            ObjectKind::Column,
        ] {
            assert!(
                d.drop_sql(kind, Some("public"), "x", None).is_err(),
                "{kind:?} 删除应拒绝"
            );
        }
    }

    #[test]
    fn truncate_table_uses_schema_prefix() {
        let d = PostgresqlDialect;
        assert_eq!(
            d.truncate_table_sql(Some("public"), "t_user").unwrap(),
            "TRUNCATE TABLE \"public\".\"t_user\""
        );
        assert_eq!(
            d.truncate_table_sql(None, "t_user").unwrap(),
            "TRUNCATE TABLE \"t_user\""
        );
    }

    #[test]
    fn drop_and_truncate_escape_embedded_quotes() {
        let d = PostgresqlDialect;
        let drop = d
            .drop_sql(ObjectKind::Table, Some("public"), "we\"ird", None)
            .unwrap();
        assert_eq!(drop, "DROP TABLE \"public\".\"we\"\"ird\"");
        let trunc = d
            .truncate_table_sql(Some("public"), "we\"ird")
            .unwrap();
        assert_eq!(trunc, "TRUNCATE TABLE \"public\".\"we\"\"ird\"");
    }
}
