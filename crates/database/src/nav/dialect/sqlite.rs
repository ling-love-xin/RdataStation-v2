//! SQLite 方言 provider（B3 L3）：单库不拼 Schema 前缀；仅表 / 视图改名，序列 / 触发器 / 例程不支持。

use engine::ObjectKind;
use shared::error::CoreError;

use super::{DialectRegistry, DialectSpi, unsupported_err};

pub(crate) struct SqliteDialect;

impl DialectSpi for SqliteDialect {
    fn family(&self) -> &'static str {
        "sqlite"
    }

    fn rename_sql(
        &self,
        kind: ObjectKind,
        _schema: Option<&str>,
        old: &str,
        _parent: Option<&str>,
        new: &str,
    ) -> Result<String, CoreError> {
        let new_ident = self.quote_ident(new);
        match kind {
            ObjectKind::Table => Ok(format!(
                "ALTER TABLE {} RENAME TO {new_ident}",
                self.quote_ident(old)
            )),
            // ⚠️ 视图改名 `ALTER VIEW … RENAME TO` 运行时未真机复核（§10 :768）；
            // 代码沿用现状分支，真机走查后再定支持位。
            ObjectKind::View => Ok(format!(
                "ALTER VIEW {} RENAME TO {new_ident}",
                self.quote_ident(old)
            )),
            ObjectKind::Sequence => Err(unsupported_err(
                self.family(),
                "rename_sql",
                "序列重命名（SQLite 无序列概念）",
            )),
            ObjectKind::Trigger => Err(unsupported_err(
                self.family(),
                "rename_sql",
                "触发器重命名（SQLite 触发器依附父表，无独立改名语句）",
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
        _schema: Option<&str>,
        name: &str,
        _parent: Option<&str>,
    ) -> Result<String, CoreError> {
        match kind {
            ObjectKind::Table => Ok(format!("DROP TABLE {}", self.quote_ident(name))),
            ObjectKind::View => Ok(format!("DROP VIEW {}", self.quote_ident(name))),
            ObjectKind::Sequence => Err(unsupported_err(
                self.family(),
                "drop_sql",
                "删除序列（SQLite 无序列概念）",
            )),
            other => Err(unsupported_err(
                self.family(),
                "drop_sql",
                format!("该类型 {other:?} 暂不支持删除"),
            )),
        }
    }

    fn truncate_table_sql(&self, _schema: Option<&str>, name: &str) -> Result<String, CoreError> {
        // SQLite 无 TRUNCATE 语句：清空表走 DELETE FROM；VACUUM 回收空间 v1 不做（§7.1 挂账）。
        Ok(format!("DELETE FROM {}", self.quote_ident(name)))
    }
    // 行估计：None（SQLite 无目录统计视图，不发真 COUNT）。
}

pub(crate) fn register(registry: &mut DialectRegistry) {
    registry.register(Box::new(SqliteDialect));
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::ObjectKind;

    #[test]
    fn rename_table_and_view_omit_schema_prefix() {
        let d = SqliteDialect;
        // SQLite 单库：即使上层传入 schema 段也不拼前缀（等价现状口径）。
        assert_eq!(
            d.rename_sql(ObjectKind::Table, Some("main"), "t_user", None, "t_account")
                .unwrap(),
            "ALTER TABLE \"t_user\" RENAME TO \"t_account\""
        );
        assert_eq!(
            d.rename_sql(ObjectKind::View, None, "v_active", None, "v_enabled")
                .unwrap(),
            "ALTER VIEW \"v_active\" RENAME TO \"v_enabled\""
        );
    }

    #[test]
    fn rename_rejects_sequence_trigger_and_routine() {
        let d = SqliteDialect;
        let seq = d
            .rename_sql(ObjectKind::Sequence, None, "seq_id", None, "seq_uid")
            .expect_err("SQLite 无序列概念，应拒绝");
        assert!(seq.to_string().contains("序列重命名"), "{seq}");
        let trig = d
            .rename_sql(ObjectKind::Trigger, None, "trg", Some("t"), "trg2")
            .expect_err("SQLite 触发器改名应拒绝");
        assert!(trig.to_string().contains("触发器重命名"), "{trig}");
        let routine = d
            .rename_sql(ObjectKind::Routine, None, "sp_do", None, "sp_run")
            .expect_err("例程需签名，应拒绝");
        assert!(routine.to_string().contains("暂不支持"), "{routine}");
    }

    #[test]
    fn row_estimate_not_supported() {
        assert!(SqliteDialect.row_estimate_sql("main").is_none());
        assert!(SqliteDialect.table_size_sql("main").is_none());
    }

    #[test]
    fn drop_table_and_view_omit_schema_prefix() {
        let d = SqliteDialect;
        // SQLite 单库：即使上层传入 schema 段也不拼前缀（等价 rename 现状口径）。
        assert_eq!(
            d.drop_sql(ObjectKind::Table, Some("main"), "t_user", None)
                .unwrap(),
            "DROP TABLE \"t_user\""
        );
        assert_eq!(
            d.drop_sql(ObjectKind::View, None, "v_active", None).unwrap(),
            "DROP VIEW \"v_active\""
        );
    }

    #[test]
    fn drop_rejects_sequence_and_non_drop_types() {
        let d = SqliteDialect;
        let seq = d
            .drop_sql(ObjectKind::Sequence, None, "seq_id", None)
            .expect_err("SQLite 无序列概念，应拒绝删除序列");
        assert!(seq.to_string().contains("序列"), "{seq}");
        for kind in [ObjectKind::Routine, ObjectKind::Trigger, ObjectKind::Schema] {
            assert!(
                d.drop_sql(kind, None, "x", None).is_err(),
                "{kind:?} 删除应拒绝"
            );
        }
    }

    #[test]
    fn truncate_uses_delete_from_without_schema_prefix() {
        let d = SqliteDialect;
        // SQLite 无 TRUNCATE 语句：清空表走 DELETE FROM，不拼 schema 前缀。
        assert_eq!(
            d.truncate_table_sql(Some("main"), "t_user").unwrap(),
            "DELETE FROM \"t_user\""
        );
    }
}
