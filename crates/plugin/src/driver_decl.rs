//! 插件驱动 → 连接对话框的桥（`drivers` 表 `driver_kind='sidecar'` 行的派生与同步）。
//!
//! 背景（2026-11-10 核查）：连接对话框的驱动目录读 global.db 的 `drivers` 表，
//! 插件激活只注册进运行时 `DriverRegistry`——两个世界没桥，oracle-jdbc / postgres-py
//! 声明的驱动在新建连接界面不可见。本模块按「全量重算」把桥补上：
//!
//! - **派生**（[`derive_rows`]）：清单 `contributes.drivers` → [`PluginDriverRow`]。
//!   连接表单的字段表（config_schema）从 `url_template` 占位符生成——占位符名就是
//!   字段 key，壳拼 URL 与表单填参天然用同一套键；
//! - **归属**：行带 `driver_kind='sidecar'`（内置声明从不写这个值，见
//!   store 侧 `plugin_driver_store.rs`），`enabled` 跟随插件启停；
//! - **防撞内置**（[`check_builtin_collisions`]）：插件驱动 id 撞内置声明 id 时在
//!   **加载期**拒绝——upsert 会覆盖内置行、运行时注册也会覆盖内置工厂，两处都伤用户；
//! - **同步**（[`resync_driver_rows`]]）：存活集合 = 当前所有已加载插件声明的驱动，
//! upsert 全量 + 删掉不在集合里的 sidecar 行（卸载 / 目录消失不留幽灵行）。
//!
//! 全局库不可用（测试宿主、启动流程未完成）时**保守放行**：记 warn 不失败——
//! 与依赖检查在 DB 未初始化时的口径一致（驱动行只是可见性，不该拦激活）。

use serde_json::{Value, json};

use shared::error::{CommonError, CoreError};

use crate::manifest::{ContributesDriver, PluginManifest};

/// store 层的行类型（经 `engine::persistence` 转发可见）
pub use engine::persistence::plugin_driver_store::PluginDriverRow;

/// 内置驱动的 id 集合。
///
/// 取 `BuiltinDriverDiscovery::builtin_factories()`（驱动列表的唯一真相源，auto_register.rs
/// 头注释）而不是运行时 `DriverRegistry`——注册表要等启动期 `auto_register` 跑过才有内容，
/// 插件加载可能发生在那之前（测试宿主 / 启动顺序变化），守卫不能依赖「恰好注册过」。
pub fn builtin_driver_ids() -> Vec<String> {
    engine::driver::BuiltinDriverDiscovery::builtin_factories()
        .iter()
        .map(|f| f.descriptor().id.clone())
        .collect()
}

/// 加载期守卫：插件声明的驱动 id 不得撞内置驱动。
///
/// 撞上的后果是双重覆盖：`drivers` 表 upsert 覆盖内置行（表单/端口/URL 全换）、
/// 运行时 `register_by_factory` 覆盖内置工厂——用户已有的内置连接会静默变行为。
/// 在 `load_plugin_from_dir` 里调用，冲突 = 加载失败（错误行留列表，修清单可重试）。
pub fn check_builtin_collisions(manifest: &PluginManifest) -> Result<(), CoreError> {
    let builtins = builtin_driver_ids();
    let clashes: Vec<&str> = manifest
        .contributes
        .drivers
        .iter()
        .filter(|d| builtins.iter().any(|b| b == &d.id))
        .map(|d| d.id.as_str())
        .collect();
    if clashes.is_empty() {
        return Ok(());
    }
    Err(CoreError::common(CommonError::general(format!(
        "插件 {} 声明的驱动 id {:?} 与内置驱动冲突（内置：{:?}）；\
         换一个驱动 id（如 {:?}-jdbc）再装",
        manifest.plugin.id, clashes, builtins, clashes[0]
    ))))
}

/// 一个清单驱动 → 表单字段（key 与 url_template 占位符同名，壳与表单共用一套键）。
fn form_fields(driver: &ContributesDriver) -> Value {
    let template = driver.url_template.as_deref().unwrap_or("");
    // 占位符按出现顺序去重
    let mut placeholders: Vec<String> = Vec::new();
    for raw in template.split(|c| c == '{' || c == '}') {
        let name = raw.trim();
        if !name.is_empty()
            && !placeholders.iter().any(|p| p == name)
            && template.contains(&format!("{{{name}}}"))
        {
            placeholders.push(name.to_string());
        }
    }
    if placeholders.is_empty() {
        // 没 url_template 也要给表单一个能填的形状：标准五件套（壳的兜底拼 URL 用同样的键）
        placeholders = ["host", "port", "database", "user", "password"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    }

    // 字段按惯例顺序排（host → port → database → user → password → file → 其余按出现序）
    let canonical = [
        "host",
        "port",
        "database",
        "user",
        "username",
        "password",
        "file_path",
        "file",
    ];
    let mut ordered: Vec<String> = canonical
        .iter()
        .filter(|c| placeholders.iter().any(|p| p == *c))
        .map(|c| c.to_string())
        .collect();
    for p in &placeholders {
        if !ordered.contains(p) {
            ordered.push(p.clone());
        }
    }

    let fields: Vec<Value> = ordered
        .iter()
        .map(|key| {
            let (label, kind, required) = match key.as_str() {
                "host" => ("主机", "text", true),
                "port" => ("端口", "number", driver.default_port.is_none()),
                "database" => ("数据库", "text", true),
                "user" | "username" => ("用户名", "text", true),
                "password" => ("密码", "password", true),
                "file_path" | "file" => ("数据库文件", "file", true),
                other => (other, "text", true),
            };
            let mut o = serde_json::Map::new();
            o.insert("key".into(), Value::from(key.clone()));
            o.insert("label".into(), Value::from(label));
            o.insert("type".into(), Value::from(kind));
            o.insert("required".into(), Value::from(required));
            if key == "port" {
                if let Some(port) = driver.default_port {
                    o.insert("default".into(), Value::from(port));
                }
            }
            Value::Object(o)
        })
        .collect();
    json!({ "fields": fields, "options": [] })
}

/// 清单声明的认证方法。
///
/// 两个来源都算数：`url_template` 里有 `{user}`/`{username}` + `{password}`（表单凭据嵌在 URL 里），
/// 或 `[permissions] credentials = "database"`（驱动壳自己接收凭据，不经过 URL 模板）。
/// 只看模板会让「凭据走 sidecar 参数」的插件在连接对话框里显示「驱动未声明认证方法」。
pub(crate) fn declared_auth_types(
    manifest: &PluginManifest,
    driver: &ContributesDriver,
) -> Vec<String> {
    let template = driver.url_template.as_deref().unwrap_or("");
    let template_has_login = (template.contains("{user}") || template.contains("{username}"))
        && template.contains("{password}");
    let declares_database_credentials = manifest
        .permissions
        .credentials
        .as_deref()
        .is_some_and(|credentials| credentials.eq_ignore_ascii_case("database"));
    if template_has_login || declares_database_credentials {
        vec!["password".to_string()]
    } else {
        Vec::new()
    }
}

/// 一个清单驱动 → 一行插件驱动声明。
///
/// `type_id`（数据库族）取驱动 id：常见族（oracle / postgresql…）内置声明已播种，
/// `ensure_plugin_family` 的 OR IGNORE 不会动它们；生僻族按驱动 id 建行，
/// 分类用清单 `category`（默认 relational，manifest 校验已卡四个合法值）。
pub fn derive_row(
    manifest: &PluginManifest,
    driver: &ContributesDriver,
    active: bool,
) -> PluginDriverRow {
    let auth = serde_json::to_string(&declared_auth_types(manifest, driver))
        .unwrap_or_else(|_| "[]".to_string());
    PluginDriverRow {
        id: driver.id.clone(),
        type_id: driver.id.clone(),
        name: driver.display_name.clone(),
        default_port: driver.default_port.map(i64::from),
        url_template: driver.url_template.clone(),
        version: manifest.plugin.version.clone(),
        config_schema: form_fields(driver).to_string(),
        supported_auth_types: auth,
        capabilities: json!(driver.features).to_string(),
        enabled: active,
    }
}

/// 全量重算并落库：存活集合 = `entries` 里所有清单声明的驱动。
///
/// 幂等；全局库未初始化时保守放行（warn 不报错）。返回 (写入行数, 删除行数) 供日志。
pub async fn resync_driver_rows(
    entries: &[(String, PluginManifest, bool)],
) -> Result<(usize, usize), CoreError> {
    let Some(db) = engine::migration::get_global_db_manager() else {
        tracing::warn!("全局数据库未初始化，跳过插件驱动行同步（连接对话框将看不到插件驱动）");
        return Ok((0, 0));
    };
    let mut rows: Vec<PluginDriverRow> = Vec::new();
    for (_, manifest, active) in entries {
        for driver in &manifest.contributes.drivers {
            rows.push(derive_row(manifest, driver, *active));
        }
    }
    let keep: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();

    let sqlite =
        db.sqlite_pool().acquire().await.map_err(|e| {
            CoreError::common(CommonError::general(format!("取全局库连接失败：{e}")))
        })?;
    let conn = sqlite
        .inner()
        .map_err(|e| CoreError::common(CommonError::general(format!("全局库连接不可用：{e}"))))?;
    use engine::persistence::plugin_driver_store as store;
    let mut written = 0usize;
    for row in &rows {
        // 族先行（OR IGNORE：oracle/postgresql 等既有族零改动）
        let category = manifest_category(entries, &row.id).unwrap_or("relational");
        store::ensure_plugin_family(conn, &row.type_id, &row.name, category)?;
        store::upsert_plugin_driver_row(conn, row)?;
        written += 1;
    }
    let deleted = store::delete_stale_plugin_driver_rows(conn, &keep)?;
    Ok((written, deleted))
}

/// 找到某个驱动 id 对应清单里声明的 category（未声明 → None → relational）。
fn manifest_category<'a>(
    entries: &'a [(String, PluginManifest, bool)],
    driver_id: &str,
) -> Option<&'a str> {
    entries.iter().find_map(|(_, manifest, _)| {
        manifest
            .contributes
            .drivers
            .iter()
            .find(|d| d.id == driver_id)
            .and_then(|d| d.category.as_deref())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with_driver(driver: ContributesDriver) -> PluginManifest {
        let mut extra = String::new();
        if let Some(port) = driver.default_port {
            extra.push_str(&format!("default_port = {port}\n"));
        }
        if let Some(t) = &driver.url_template {
            extra.push_str(&format!("url_template = {t:?}\n"));
        }
        if !driver.features.is_empty() {
            let list = driver
                .features
                .iter()
                .map(|f| format!("{f:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            extra.push_str(&format!("features = [{list}]\n"));
        }
        let toml = format!(
            r#"
            [plugin]
            id = "rds.driver.test"
            name = "Test"
            version = "0.2.0"
            publisher = "rds"
            [plugin.engines]
            rdatastation = ">=0.1"
            [backend]
            kind = "sidecar"
            executable = "shell"
            [[contributes.drivers]]
            id = "{id}"
            display_name = "{display_name}"
            {extra}"#,
            id = driver.id,
            display_name = driver.display_name,
        );
        // 走 toml 解析 + validate（category 校验在 validate 里）；引擎版本闸与文件读取
        // 不属于这里的被测面
        let manifest: crate::manifest::PluginManifest =
            toml::from_str(&toml).expect("清单应当能解析");
        manifest.validate().expect("清单应当合法");
        manifest
    }

    fn oracle_like_driver() -> ContributesDriver {
        ContributesDriver {
            id: "my-oracle".into(),
            display_name: "My Oracle".into(),
            default_port: Some(1521),
            connection_schema: None,
            url_template: Some("jdbc:oracle:thin:@//{host}:{port}/{database}".into()),
            features: vec!["transactions".into(), "cancel".into()],
            category: None,
        }
    }

    #[test]
    fn fields_follow_url_template_placeholders() {
        let driver = oracle_like_driver();
        let schema: Value = serde_json::from_str(&form_fields(&driver).to_string()).unwrap();
        let fields = schema["fields"].as_array().unwrap().clone();
        let keys: Vec<&str> = fields.iter().map(|f| f["key"].as_str().unwrap()).collect();
        assert_eq!(keys, vec!["host", "port", "database"]);
        // port 有 default_port → required=false + default=1521
        let port = &fields[1];
        assert_eq!(port["required"], json!(false));
        assert_eq!(port["default"], json!(1521));
        assert_eq!(port["type"], json!("number"));
    }

    #[test]
    fn template_without_url_gets_default_five_fields() {
        let mut driver = oracle_like_driver();
        driver.url_template = None;
        let schema: Value = serde_json::from_str(&form_fields(&driver).to_string()).unwrap();
        let keys: Vec<&str> = schema["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["key"].as_str().unwrap())
            .collect();
        assert_eq!(keys, vec!["host", "port", "database", "user", "password"]);
    }

    #[test]
    fn user_placeholder_marks_password_auth() {
        let mut driver = oracle_like_driver();
        driver.url_template =
            Some("postgresql://{user}:{password}@{host}:{port}/{database}".into());
        let manifest = manifest_with_driver(driver);
        let row = derive_row(&manifest, &manifest.contributes.drivers[0], false);
        assert_eq!(row.supported_auth_types, r#"["password"]"#);
        assert!(!row.enabled, "未激活插件的行应当置灰");
    }

    #[test]
    fn database_credentials_grant_marks_password_auth_without_template() {
        let manifest: crate::manifest::PluginManifest = toml::from_str(
            r#"
[plugin]
id = "rds.driver.credonly"
name = "Credential Only"
version = "1.0.0"
publisher = "rds"
[plugin.engines]
rdatastation = ">=0.1"
[backend]
kind = "sidecar"
executable = "bin/agent"
[permissions]
credentials = "database"
[[contributes.drivers]]
id = "credonly"
display_name = "Credential Only DB"
default_port = 1521
"#,
        )
        .expect("清单应当能解析");
        manifest.validate().expect("清单应当合法");
        let row = derive_row(&manifest, &manifest.contributes.drivers[0], true);
        assert_eq!(row.supported_auth_types, r#"["password"]"#);
    }

    #[test]
    fn derive_row_shape() {
        let manifest = manifest_with_driver(oracle_like_driver());
        let row = derive_row(&manifest, &manifest.contributes.drivers[0], true);
        assert_eq!(row.id, "my-oracle");
        assert_eq!(row.type_id, "my-oracle"); // 族 = 驱动 id（既有族 OR IGNORE 不动）
        assert_eq!(row.name, "My Oracle");
        assert_eq!(row.default_port, Some(1521));
        assert_eq!(row.version, "0.2.0");
        assert_eq!(row.capabilities, r#"["transactions","cancel"]"#);
        // 模板没有 {user} → 无认证面
        assert_eq!(row.supported_auth_types, "[]");
        assert!(row.enabled);
    }

    #[test]
    fn builtin_collision_is_rejected_with_named_ids() {
        let mut driver = oracle_like_driver();
        // 内置驱动（D3 之后内核只剩官方那 4 条；`postgres` / `mysql` 已归插件所有，
        // 插件贡献它们**不算**冲突——那正是 `rds.driver.sqlx` 的活法）
        driver.id = "postgres_native".into();
        let manifest = manifest_with_driver(driver);
        let err = check_builtin_collisions(&manifest).unwrap_err().to_string();
        assert!(err.contains("postgres_native"), "错误应点名冲突 id：{err}");
        assert!(err.contains("rds.driver.test"));

        // 不撞的放行
        let ok = manifest_with_driver(oracle_like_driver());
        assert!(check_builtin_collisions(&ok).is_ok());

        // 与 sqlx 驱动的 id（`postgres`）不再冲突：它已由插件贡献
        let mut driver = oracle_like_driver();
        driver.id = "postgres".into();
        let manifest = manifest_with_driver(driver);
        assert!(
            check_builtin_collisions(&manifest).is_ok(),
            "`postgres` 已归插件所有，不该再判成与内置冲突"
        );
    }

    #[test]
    fn category_must_be_one_of_type_tree_sections() {
        let toml = r#"
            [plugin]
            id = "rds.driver.cat"
            name = "Cat"
            version = "0.1.0"
            publisher = "rds"
            [plugin.engines]
            rdatastation = ">=0.1"
            [backend]
            kind = "sidecar"
            executable = "shell"
            [[contributes.drivers]]
            id = "something"
            display_name = "Something"
            category = "weird"
        "#;
        let err = toml::from_str::<crate::manifest::PluginManifest>(toml)
            .expect("解析")
            .validate()
            .unwrap_err()
            .to_string();
        assert!(err.contains("category"), "错误应指向 category：{err}");
    }
}
