//! 来源链的一行说法：`origin_kind` + `origin_ref` → 界面文案。
//!
//! 跨项目搬运的老链路是「项目 A → 全局 → 项目 B」，**不允许直传**。用户口径是
//! 「各层版本各自编号、不冗余记录、但要有线索可寻」：A 的 v5 升级成全局 v1，再下发
//! 成 B 的 v1。所以每一层只记**直接上游**那一跳：
//!
//! | 行 | `origin_kind` | `origin_ref` |
//! | --- | --- | --- |
//! | 全局行（由项目 A 升级） | `project` | `{"resource_id":"arp_x","version":5}` |
//! | 项目行（由全局下发） | `global` | `{"instance":"gi_x","version":1}` |
//! | 项目行（草稿箱归档） | 空（来源在 `promoted_from` 里） | 空 |
//!
//! 拼整条链是读取侧的事：顺着每一跳往上看，比在每一行里冗余整条链更不容易走样
//! （冗余会随上游新增版本而过期）。
//!
//! 脏数据一律**降级为空串**（详情不画那一行），不硬报错：`origin_ref` 是老库 /
//! 手工修库都可能被写坏的列，为它让整个详情打不开不值得。

/// 上游是项目侧的存档。
pub const ORIGIN_KIND_PROJECT: &str = "project";
/// 上游是全局实例里的资产。
pub const ORIGIN_KIND_GLOBAL: &str = "global";

/// 一行「上游」文案；空串 = 不画。
///
/// 例：`project` + `{"resource_id":"arp_1","version":5}` → `项目存档 arp_1 的 v5`；
/// 缺版本号时退成 `项目存档 arp_1`（仍比只显示一串 id 有用）。
pub fn describe(kind: Option<&str>, reference: Option<&str>) -> String {
    let (Some(kind), Some(reference)) = (kind, reference) else {
        return String::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(reference) else {
        return String::new();
    };
    let (noun, id) = match kind {
        ORIGIN_KIND_PROJECT => ("项目存档", value.get("resource_id").and_then(|v| v.as_str())),
        ORIGIN_KIND_GLOBAL => ("全局实例", value.get("instance").and_then(|v| v.as_str())),
        _ => return String::new(),
    };
    let Some(id) = id.filter(|id| !id.is_empty()) else {
        return String::new();
    };
    match value.get("version").and_then(|v| v.as_i64()) {
        Some(version) => format!("{noun} {id} 的 v{version}"),
        None => format!("{noun} {id}"),
    }
}

/// 项目行升级到全局时写的 `origin_ref`（A 的一跳）。
pub fn project_ref(resource_id: &str, version: Option<i32>) -> String {
    serde_json::json!({
        "resource_id": resource_id,
        "version": version,
    })
    .to_string()
}

/// 全局行下发到项目时写的 `origin_ref`（全局的一跳）。
pub fn global_ref(instance: &str, version: i32) -> String {
    serde_json::json!({
        "instance": instance,
        "version": version,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_names_both_hop_kinds() {
        assert_eq!(
            describe(Some("project"), Some(r#"{"resource_id":"arp_1","version":5}"#)),
            "项目存档 arp_1 的 v5"
        );
        assert_eq!(
            describe(Some("global"), Some(r#"{"instance":"gi_1","version":1}"#)),
            "全局实例 gi_1 的 v1"
        );
    }

    #[test]
    fn missing_version_still_names_the_upstream() {
        assert_eq!(
            describe(Some("project"), Some(r#"{"resource_id":"arp_1","version":null}"#)),
            "项目存档 arp_1"
        );
    }

    #[test]
    fn dirty_or_unknown_input_renders_nothing() {
        // 没有来源、串不是 JSON、kind 认不出、id 为空：一律不画（详情少一行，不炸）。
        assert_eq!(describe(None, None), "");
        assert_eq!(describe(Some("project"), None), "");
        assert_eq!(describe(Some("project"), Some("not json")), "");
        assert_eq!(describe(Some("future"), Some(r#"{"resource_id":"arp_1"}"#)), "");
        assert_eq!(describe(Some("project"), Some(r#"{"resource_id":""}"#)), "");
    }

    #[test]
    fn builders_roundtrip_through_the_reader() {
        let project = project_ref("arp_1", Some(5));
        assert_eq!(describe(Some("project"), Some(&project)), "项目存档 arp_1 的 v5");
        let global = global_ref("gi_1", 1);
        assert_eq!(describe(Some("global"), Some(&global)), "全局实例 gi_1 的 v1");
    }
}
