//! 内省结果里的**枚举码 → 展示文案**（PG 的 `pg_constraint` / `pg_class` 码表等）。
//!
//! 纯字符串映射：没有驱动类型、没有 IO。放这里是因为插件与内核驱动都要用同一套文案
//! —— 同一个约束在两个驱动下显示成两种名字，用户会以为是两种约束。

/// 逗号分隔的列名 → `Vec<String>`（SQL 侧 `string_agg` 的产物）。
///
/// 空片段一律丢掉：`string_agg` 在「一列都没有」时给的是空串而不是 `","`，
/// 但组合键里也可能出现空位（PG 的 `indkey` 用 0 表示表达式索引的占位）。
pub fn split_csv(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// `pg_constraint.contype` → 展示用名称。
pub fn pg_constraint_kind(code: &str) -> String {
    match code {
        "p" => "PRIMARY KEY",
        "f" => "FOREIGN KEY",
        "u" => "UNIQUE",
        "c" => "CHECK",
        "x" => "EXCLUDE",
        other => other,
    }
    .to_string()
}

/// `confupdtype` / `confdeltype` → 规则名。
///
/// `' '`（空格）是 PG 的「没写 ON UPDATE/DELETE」——**不是空串**，漏了它每个外键
/// 都会多出一对空的规则行。
pub fn pg_fk_action(code: &str) -> Option<String> {
    match code {
        "a" => Some("NO ACTION".to_string()),
        "r" => Some("RESTRICT".to_string()),
        "c" => Some("CASCADE".to_string()),
        "n" => Some("SET NULL".to_string()),
        "d" => Some("SET DEFAULT".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_csv_drops_empty_fragments() {
        assert_eq!(split_csv("a,b,c"), vec!["a", "b", "c"]);
        assert_eq!(split_csv(" a , b "), vec!["a", "b"]);
        assert!(split_csv("").is_empty());
        // 组合键里可能有空位
        assert_eq!(split_csv("a,,b"), vec!["a", "b"]);
    }

    #[test]
    fn pg_constraint_codes_map_to_names() {
        assert_eq!(pg_constraint_kind("p"), "PRIMARY KEY");
        assert_eq!(pg_constraint_kind("f"), "FOREIGN KEY");
        assert_eq!(pg_constraint_kind("u"), "UNIQUE");
        assert_eq!(pg_constraint_kind("c"), "CHECK");
        assert_eq!(pg_constraint_kind("x"), "EXCLUDE");
        // 未知码原样透出（不吞成空串，也不猜）
        assert_eq!(pg_constraint_kind("t"), "t");
    }

    #[test]
    fn pg_fk_actions_map_and_blank_is_none() {
        assert_eq!(pg_fk_action("a").as_deref(), Some("NO ACTION"));
        assert_eq!(pg_fk_action("r").as_deref(), Some("RESTRICT"));
        assert_eq!(pg_fk_action("c").as_deref(), Some("CASCADE"));
        assert_eq!(pg_fk_action("n").as_deref(), Some("SET NULL"));
        assert_eq!(pg_fk_action("d").as_deref(), Some("SET DEFAULT"));
        // 空格 = 没写 ON UPDATE/DELETE，不是一条规则
        assert_eq!(pg_fk_action(" "), None);
        assert_eq!(pg_fk_action(""), None);
    }
}
