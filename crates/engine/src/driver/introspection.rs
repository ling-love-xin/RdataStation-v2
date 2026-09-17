use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;

/// 内省级别（对标 DataGrip 2026.1 Introspection Levels）
///
/// Level 1: 仅名称 + 类型签名（列类型为 unknown）
/// Level 2: 全部元数据，不含源码
/// Level 3: 全部元数据 + 例程源码
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum IntrospectionLevel {
    Level1,
    Level2,
    Level3,
}

impl IntrospectionLevel {
    /// 根据对象数量自动选择级别
    ///
    /// 阈值对标 DataGrip 2026.1:
    ///   N ≤ 1000 → Level3
    ///   N ≤ 3000 → Level2
    ///   否则 → Level1
    pub fn from_object_count(count: usize) -> Self {
        if count <= 1000 {
            Self::Level3
        } else if count <= 3000 {
            Self::Level2
        } else {
            Self::Level1
        }
    }

    /// 是否应加载列详情
    pub fn should_load_columns(&self) -> bool {
        matches!(self, Self::Level2 | Self::Level3)
    }

    /// 是否应加载源码（例程 DDL）
    pub fn should_load_source(&self) -> bool {
        matches!(self, Self::Level3)
    }
}

impl std::fmt::Display for IntrospectionLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Level1 => write!(f, "level1"),
            Self::Level2 => write!(f, "level2"),
            Self::Level3 => write!(f, "level3"),
        }
    }
}

/// 全局内省级别注册表
static INTROSPECTION_REGISTRY: OnceLock<Mutex<HashMap<String, IntrospectionLevel>>> =
    OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, IntrospectionLevel>> {
    INTROSPECTION_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 设置连接的内省级别
pub fn set_level(conn_id: &str, level: IntrospectionLevel) {
    if let Ok(mut map) = registry().lock() {
        map.insert(conn_id.to_string(), level);
    }
}

/// 获取连接的内省级别（默认 Level3）
pub fn get_level(conn_id: &str) -> IntrospectionLevel {
    registry()
        .lock()
        .ok()
        .and_then(|map| map.get(conn_id).copied())
        .unwrap_or(IntrospectionLevel::Level3)
}

/// 移除连接的级别设置
pub fn remove_level(conn_id: &str) {
    if let Ok(mut map) = registry().lock() {
        map.remove(conn_id);
    }
}

#[cfg(test)]
mod tests {
use super::*;

/// 阈值对标 DataGrip：≤1000 全量（Level3）、≤3000 概要（Level2）、否则仅索引（Level1）。
#[test]
fn auto_level_by_object_count() {
    assert_eq!(IntrospectionLevel::from_object_count(500), IntrospectionLevel::Level3);
    assert_eq!(IntrospectionLevel::from_object_count(1000), IntrospectionLevel::Level3);
    assert_eq!(IntrospectionLevel::from_object_count(1001), IntrospectionLevel::Level2);
    assert_eq!(IntrospectionLevel::from_object_count(3000), IntrospectionLevel::Level2);
    assert_eq!(IntrospectionLevel::from_object_count(3001), IntrospectionLevel::Level1);
}

/// 能力位：**仅 Level1 不加载列**（Level2/3 都加载）；源码只在 Level3。
///
/// 导航侧靠 `should_load_columns` 决定是否做 C2 邻接预取。
#[test]
fn capability_flags() {
    assert!(IntrospectionLevel::Level3.should_load_columns());
    assert!(IntrospectionLevel::Level2.should_load_columns());
    assert!(!IntrospectionLevel::Level1.should_load_columns());

    assert!(IntrospectionLevel::Level3.should_load_source());
    assert!(!IntrospectionLevel::Level2.should_load_source());
}

/// 注册表往返：设置 / 读取 / 移除（未知连接保底 Level3）。
#[test]
fn registry_roundtrip() {
    let conn = "P_introspection_test";
    assert_eq!(
        get_level("P_introspection_unknown"),
        IntrospectionLevel::Level3,
        "未知连接默认 Level3"
    );

    set_level(conn, IntrospectionLevel::Level1);
    assert_eq!(get_level(conn), IntrospectionLevel::Level1);

    remove_level(conn);
    assert_eq!(get_level(conn), IntrospectionLevel::Level3, "移除后回默认");
}
}
