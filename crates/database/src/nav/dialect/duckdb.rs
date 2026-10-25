//! DuckDB 方言 provider（B3 L3）：固定 main、不拼 Schema 前缀；表 / 视图 / 序列改名，触发器 / 例程不支持。

use engine::ObjectKind;
use shared::error::CoreError;

use super::{DialectRegistry, DialectSpi, unsupported_err};

pub(crate) struct DuckdbDialect;

impl DialectSpi for DuckdbDialect {
    fn family(&self) -> &'static str {
        "duckdb"
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
            // ⚠️ 视图改名运行时未真机复核（§10 :768）；代码沿用现状分支。
            ObjectKind::View => Ok(format!(
                "ALTER VIEW {} RENAME TO {new_ident}",
                self.quote_ident(old)
            )),
            ObjectKind::Sequence => Ok(format!(
                "ALTER SEQUENCE {} RENAME TO {new_ident}",
                self.quote_ident(old)
            )),
            ObjectKind::Trigger => Err(unsupported_err(
                self.family(),
                "触发器重命名（DuckDB 不支持触发器改名）",
            )),
            ObjectKind::Routine => Err(unsupported_err(
                self.family(),
                "例程重命名需函数签名，暂不支持",
            )),
            other => Err(unsupported_err(
                self.family(),
                format!("该类型 {other:?} 暂不支持对象重命名"),
            )),
        }
    }
    // 行估计：None（DuckDB 目录统计口径另案，不发真 COUNT）。
}

pub(crate) fn register(registry: &mut DialectRegistry) {
    registry.register(Box::new(DuckdbDialect));
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::ObjectKind;

    #[test]
    fn rename_table_and_sequence_omit_schema_prefix() {
        let d = DuckdbDialect;
        // DuckDB 固定 main：schema 段不拼前缀（等价现状口径）。
        assert_eq!(
            d.rename_sql(ObjectKind::Table, Some("main"), "t_user", None, "t_account")
                .unwrap(),
            "ALTER TABLE \"t_user\" RENAME TO \"t_account\""
        );
        assert_eq!(
            d.rename_sql(ObjectKind::Sequence, None, "seq_id", None, "seq_uid")
                .unwrap(),
            "ALTER SEQUENCE \"seq_id\" RENAME TO \"seq_uid\""
        );
    }

    #[test]
    fn rename_rejects_trigger_and_routine() {
        let d = DuckdbDialect;
        let trig = d
            .rename_sql(ObjectKind::Trigger, None, "trg", Some("t"), "trg2")
            .expect_err("DuckDB 触发器改名应拒绝");
        assert!(trig.to_string().contains("触发器重命名"), "{trig}");
        let routine = d
            .rename_sql(ObjectKind::Routine, None, "sp_do", None, "sp_run")
            .expect_err("例程需签名，应拒绝");
        assert!(routine.to_string().contains("暂不支持"), "{routine}");
    }

    #[test]
    fn row_estimate_not_supported() {
        assert!(DuckdbDialect.row_estimate_sql("main").is_none());
        assert!(DuckdbDialect.table_size_sql("main").is_none());
    }
}
