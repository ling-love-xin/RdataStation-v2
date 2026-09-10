//! ID 前缀生成器
//!
//! 统一管理项目中的 ID 前缀约定：
//!   G_xxx  = 全局表主键（global.db）
//!   P_xxx  = 项目表主键（本地创建，project.db）
//!   GP_xxx = 从全局快照到项目的数据（project.db）
//!
//! 用法：
//! ```ignore
//! let gid = generate_gid("env", "dev");   // → "G_env_dev"
//! let pid = generate_pid("env");           // → "P_env_a1b2c3d4"
//! let gpid = generate_gpid("env", "dev");  // → "GP_env_dev_20260522"
//! ```

use chrono::Utc;

const GLOBAL_PREFIX: &str = "G_";
const PROJECT_PREFIX: &str = "P_";
const SNAPSHOT_PREFIX: &str = "GP_";

/// 生成全局 ID
///
/// 格式：G_{entity}_{name}
/// 示例：G_env_dev, G_auth_001, G_net_ssh_main
pub fn generate_gid(entity: &str, name: &str) -> String {
    let safe_name = sanitize_name(name);
    format!("{}{}_{}", GLOBAL_PREFIX, entity, safe_name)
}

/// 生成项目本地 ID
///
/// 格式：P_{entity}_{random_suffix}
/// 示例：P_env_a1b2c3d4
pub fn generate_pid(entity: &str) -> String {
    let suffix = short_rand();
    format!("{}{}_{}", PROJECT_PREFIX, entity, suffix)
}

/// 生成快照 ID（从全局快照到项目）
///
/// 格式：GP_{entity}_{name}_{date}
/// 示例：GP_env_dev_20260522
pub fn generate_gpid(entity: &str, name: &str) -> String {
    let safe_name = sanitize_name(name);
    let date = Utc::now().format("%Y%m%d");
    format!("{}{}_{}_{}", SNAPSHOT_PREFIX, entity, safe_name, date)
}

/// 判断 ID 是否为全局 ID
pub fn is_global(id: &str) -> bool {
    id.starts_with(GLOBAL_PREFIX)
}

/// 判断 ID 是否为项目本地 ID
pub fn is_project(id: &str) -> bool {
    id.starts_with(PROJECT_PREFIX)
}

/// 判断 ID 是否为快照 ID
pub fn is_snapshot(id: &str) -> bool {
    id.starts_with(SNAPSHOT_PREFIX)
}

/// 从 ID 提取来源类型
pub fn origin_from_id(id: &str) -> &'static str {
    if is_global(id) {
        "global"
    } else if is_snapshot(id) {
        "global_snapshot"
    } else {
        "project"
    }
}

/// 从快照 ID (GP_xxx_name_date) 反查全局源 ID (G_xxx_name)
///
/// 返回 None 如果 ID 不是快照格式
pub fn source_global_id(snapshot_id: &str) -> Option<String> {
    if !is_snapshot(snapshot_id) {
        return None;
    }
    // GP_env_dev_20260522 → G_env_dev
    let without_prefix = snapshot_id.strip_prefix(SNAPSHOT_PREFIX)?;
    let parts: Vec<&str> = without_prefix.rsplitn(2, '_').collect();
    if parts.len() < 2 {
        return None;
    }
    // parts[1] = "env_dev" (去掉日期后), parts[0] = "20260522"
    Some(format!("{}{}", GLOBAL_PREFIX, parts[1]))
}

/// 将全局 ID (G_xxx_name) 转换为快照 ID (GP_xxx_name_date)
///
/// 返回 None 如果 ID 不是全局格式
pub fn to_snapshot_id(global_id: &str) -> Option<String> {
    if !is_global(global_id) {
        return None;
    }
    // G_env_dev → env_dev → (entity="env", name="dev")
    let without_prefix = global_id.strip_prefix(GLOBAL_PREFIX)?;
    let first_underscore = without_prefix.find('_')?;
    let entity = &without_prefix[..first_underscore];
    let name = &without_prefix[first_underscore + 1..];
    if name.is_empty() {
        return None;
    }
    Some(generate_gpid(entity, name))
}

/// 生成项目级 ID（带指定后缀）
///
/// 格式：P_{entity}_{suffix}
/// 示例：gen_project_id("ep", "uuid") → P_ep_uuid
pub fn gen_project_id(entity: &str, suffix: &str) -> String {
    let safe_suffix = sanitize_name(suffix);
    format!("{}{}_{}", PROJECT_PREFIX, entity, safe_suffix)
}

/// 清理名称中的特殊字符，替换为下划线
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// 生成 8 位随机十六进制后缀
fn short_rand() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!("{:08x}", rng.gen::<u32>())
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::{CommonError, CoreError};

    #[test]
    fn test_generate_gid() {
        let id = generate_gid("env", "dev");
        assert!(id.starts_with("G_env_"));
        assert_eq!(id, "G_env_dev");
    }

    #[test]
    fn test_generate_pid() {
        let id = generate_pid("env");
        assert!(id.starts_with("P_env_"));
        assert_eq!(id.len(), "P_env_".len() + 8);
    }

    #[test]
    fn test_generate_gpid() {
        let id = generate_gpid("env", "dev");
        assert!(id.starts_with("GP_env_dev_"));

        let id2 = generate_gpid("env", "dev");
        assert_eq!(id, id2, "同一天同名的快照 ID 应该一致");
    }

    #[test]
    fn test_is_global() {
        assert!(is_global("G_env_dev"));
        assert!(!is_global("P_env_001"));
        assert!(!is_global("GP_env_dev_2026"));
    }

    #[test]
    fn test_is_project() {
        assert!(is_project("P_env_001"));
        assert!(!is_project("G_env_dev"));
    }

    #[test]
    fn test_is_snapshot() {
        assert!(is_snapshot("GP_env_dev_2026"));
        assert!(!is_snapshot("G_env_dev"));
    }

    #[test]
    fn test_origin_from_id() {
        assert_eq!(origin_from_id("G_env_dev"), "global");
        assert_eq!(origin_from_id("P_env_001"), "project");
        assert_eq!(origin_from_id("GP_env_dev_2026"), "global_snapshot");
    }

    #[test]
    fn test_source_global_id() {
        let gpid = generate_gpid("env", "dev");
        let source = source_global_id(&gpid);
        assert_eq!(source, Some("G_env_dev".to_string()));
    }

    #[test]
    fn test_to_snapshot_id() -> Result<(), CoreError> {
        let snapshot = to_snapshot_id("G_env_dev").ok_or_else(|| {
            CoreError::common(CommonError::General(
                "to_snapshot_id returned None".to_string(),
            ))
        })?;
        assert!(snapshot.starts_with("GP_env_dev_"));
        Ok(())
    }

    #[test]
    fn test_to_snapshot_id_invalid() {
        assert_eq!(to_snapshot_id("P_env_001"), None);
        assert_eq!(to_snapshot_id("GP_env_dev_2026"), None);
    }

    #[test]
    fn test_gen_project_id() {
        let id = gen_project_id("ep", "suffix");
        assert!(id.starts_with("P_ep_"));
        assert!(id.ends_with("suffix"));
    }

    #[test]
    fn test_sanitize_name() {
        let id = generate_gid("env", "my env!");
        assert!(!id.contains(' '));
        assert!(!id.contains('!'));
    }
}
