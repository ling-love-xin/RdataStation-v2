//! 全局一版一张表的表名（G2，2026-11-16 口径）。
//!
//! 全局库按版本铺表：每次升级落一张**新表**，历史版本的表不删——「v3 长什么样」
//! 才有东西可看。表名 `res_<描述>_<yyyymmddHHMMSS>`：人眼能认出是谁、什么时候，
//! 又不会和用户看到的资产名（`orders`）混成一个。
//!
//! 登记行的 `name` 仍是**资产名**（列表 / 筛选 / 同名链版本号都靠它），表名单独记
//! 在 `analytics_table` 列——两者解耦是这一刀的前提。老行（迁移 036 之前）那列是
//! 空的，读取面回退到 `name`（那时表名就等于资产名）。

use chrono::{DateTime, Utc};

/// 描述段长度上限：再长就没人读得懂，日志里也难对齐。
const SLUG_MAX: usize = 32;

/// 一版一张表的表名：`res_<描述>_<时间戳>`。
pub fn version_table_name(name: &str, now: DateTime<Utc>) -> String {
    format!("res_{}_{}", slug(name), now.format("%Y%m%d%H%M%S"))
}

/// 撞名时的退路：同一秒内同名升级两次会用同一个时间戳，加版本号区分。
///
/// 正常路径几乎走不到（同名第二次升级是「上一版已存在 → 冲突 → 覆盖」），
/// 但秒级时间戳确实可能撞，留一条确定性的退路比事后查重名稳。
pub fn version_table_name_with_version(name: &str, version: i32, now: DateTime<Utc>) -> String {
    format!(
        "res_{}_{}_v{version}",
        slug(name),
        now.format("%Y%m%d%H%M%S")
    )
}

/// 描述段：小写、非字母数字折 `_`、压缩连续下划线、截断、去首尾下划线。
///
/// 用 Unicode 的 `is_alphanumeric`：中文资产名（`销售报表`）要能原样保留可读性，
/// 不能退化成 `res_asset_…`——表名是给人看的。
fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_underscore = false;
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            for c in ch.to_lowercase() {
                out.push(c);
            }
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }
    let cut: String = out.trim_matches('_').chars().take(SLUG_MAX).collect();
    let trimmed = cut.trim_matches('_').to_string();
    if trimmed.is_empty() {
        // 全是符号的名字（`!!!`）：退到一个固定前缀，不产出 `res__<ts>`。
        "asset".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 16, 9, 30, 5).unwrap()
    }

    #[test]
    fn plain_name_becomes_prefixed_table_name() {
        assert_eq!(version_table_name("orders", at()), "res_orders_20261116093005");
    }

    #[test]
    fn separators_and_case_fold_into_underscores() {
        assert_eq!(
            version_table_name("Q3 销售 报表.csv", at()),
            "res_q3_销售_报表_csv_20261116093005",
            "空格 / 点折成单个下划线，大小写折平"
        );
    }

    #[test]
    fn consecutive_symbols_collapse_and_edges_are_trimmed() {
        assert_eq!(version_table_name("__a--b!!", at()), "res_a_b_20261116093005");
    }

    #[test]
    fn symbol_only_names_fall_back_to_asset() {
        assert_eq!(version_table_name("!!!", at()), "res_asset_20261116093005");
    }

    #[test]
    fn long_names_are_shortened_without_a_trailing_underscore() {
        let name = "x".repeat(40);
        assert_eq!(
            version_table_name(&name, at()),
            format!("res_{}_20261116093005", "x".repeat(SLUG_MAX))
        );
        // 第 32 个字符正好是分隔符时也不能留尾部下划线。
        let mut tricky = "y".repeat(31);
        tricky.push_str(" 后续");
        assert_eq!(
            version_table_name(&tricky, at()),
            format!("res_{}_20261116093005", "y".repeat(31))
        );
    }

    #[test]
    fn collided_stamp_falls_back_to_a_versioned_name() {
        assert_eq!(
            version_table_name_with_version("orders", 3, at()),
            "res_orders_20261116093005_v3"
        );
    }
}
