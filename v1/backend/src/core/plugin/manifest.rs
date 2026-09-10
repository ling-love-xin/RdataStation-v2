use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::core::error::{CommonError, CoreError, StorageError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub permissions: PluginPermissions,
    #[serde(default)]
    pub contributes: PluginContributes,
    #[serde(default)]
    pub dependencies: Vec<PluginDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    pub engines: PluginEngines,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEngines {
    pub rdatastation: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub frontend: Option<CapabilitiesFrontend>,
    #[serde(default)]
    pub wasm: Option<CapabilitiesWasm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesFrontend {
    pub entry: String,
    #[serde(default)]
    pub activation_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesWasm {
    pub entry: String,
    #[serde(default)]
    pub max_memory_mb: Option<usize>,
    #[serde(default)]
    pub max_cpu_time_ms: Option<u64>,
    #[serde(default)]
    pub allowed_host_functions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginPermissions {
    #[serde(default)]
    pub frontend: Vec<String>,
    #[serde(default)]
    pub wasm: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginContributes {
    #[serde(default)]
    pub commands: Vec<ContributesCommand>,
    #[serde(default)]
    pub panels: Vec<ContributesPanel>,
    #[serde(default)]
    pub drivers: Vec<ContributesDriver>,
    #[serde(default)]
    pub settings: Vec<ContributesSetting>,
    #[serde(default)]
    pub menus: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesCommand {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesPanel {
    pub id: String,
    pub title: String,
    pub location: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub order: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesDriver {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub default_port: Option<u16>,
    #[serde(default)]
    pub connection_schema: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesSetting {
    pub key: String,
    #[serde(rename = "type")]
    pub setting_type: String,
    pub default: serde_json::Value,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDependency {
    pub id: String,
    pub version: String,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.plugin.id.is_empty() {
            return Err(CoreError::common(CommonError::General(
                "Manifest missing required field: plugin.id".into(),
            )));
        }
        if self.plugin.name.is_empty() {
            return Err(CoreError::common(CommonError::General(
                "Manifest missing required field: plugin.name".into(),
            )));
        }
        if self.plugin.version.is_empty() {
            return Err(CoreError::common(CommonError::General(
                "Manifest missing required field: plugin.version".into(),
            )));
        }
        if self.capabilities.frontend.is_none() && self.capabilities.wasm.is_none() {
            return Err(CoreError::common(CommonError::General(
                "Manifest must have at least one capability (frontend or wasm)".into(),
            )));
        }
        Ok(())
    }

    pub fn check_engine_compatibility(&self, current_version: &str) -> Result<(), CoreError> {
        let required = &self.plugin.engines.rdatastation;

        let current_major: u32 = current_version
            .split('.')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let range_str = required
            .trim_start_matches('^')
            .trim_start_matches('~')
            .trim_start_matches('>')
            .trim_start_matches('<')
            .trim_start_matches('=');
        let required_major: u32 = range_str
            .split('.')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if current_major < required_major && required.starts_with('^') {
            return Err(CoreError::common(CommonError::General(format!(
                "Plugin '{}' requires engine version {}, but current version is {}",
                self.plugin.id, required, current_version
            ))));
        }
        Ok(())
    }
}

pub struct ManifestParser;

impl ManifestParser {
    pub fn parse(path: &Path) -> Result<PluginManifest, CoreError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            CoreError::storage(StorageError::io(
                path.to_string_lossy().to_string(),
                "read".to_string(),
                e.to_string(),
            ))
        })?;

        let manifest: PluginManifest = toml::from_str(&content).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to parse manifest '{}': {}",
                path.display(),
                e
            )))
        })?;

        manifest.validate()?;

        manifest.check_engine_compatibility(env!("CARGO_PKG_VERSION"))?;

        Ok(manifest)
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_toml(content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join("test_rdata_plugin.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_parse_valid_manifest_with_all_fields() {
        let toml_content = r#"
[plugin]
id = "com.example.test"
name = "Test Plugin"
version = "1.0.0"
publisher = "Example Corp"
description = "A test plugin"
icon = "icon.png"
homepage = "https://example.com"
license = "MIT"

[plugin.engines]
rdatastation = "^0.1.0"

[capabilities.frontend]
entry = "./extension.ts"
activation_events = ["onStartup"]

[capabilities.wasm]
entry = "./plugin.wasm"
max_memory_mb = 256
max_cpu_time_ms = 15000
allowed_host_functions = ["db_query"]

[permissions]
frontend = ["data:query"]
wasm = ["wasm:db_query"]

[[contributes.commands]]
id = "test.hello"
title = "Say Hello"
category = "Test"
icon = "hand"
shortcut = "Ctrl+H"

[[contributes.panels]]
id = "test.panel"
title = "Test Panel"
location = "right"
icon = "flask"
order = 100

[[contributes.drivers]]
id = "testdb"
display_name = "Test Database"
default_port = 9999
features = ["tables", "views"]

[[contributes.settings]]
key = "test.enabled"
type = "boolean"
default = true
label = "Enable Test"
description = "Enable or disable the test feature"

[[dependencies]]
id = "com.example.base"
version = "^1.0.0"
"#;

        let path = write_temp_toml(toml_content);
        let result = ManifestParser::parse(&path);

        assert!(result.is_ok(), "Expected Ok but got: {:?}", result.err());

        let manifest = result.unwrap();
        assert_eq!(manifest.plugin.id, "com.example.test");
        assert_eq!(manifest.plugin.name, "Test Plugin");
        assert_eq!(manifest.plugin.version, "1.0.0");
        assert_eq!(manifest.plugin.publisher, "Example Corp");
        assert_eq!(manifest.plugin.description, "A test plugin");
        assert_eq!(manifest.plugin.icon, Some("icon.png".to_string()));
        assert_eq!(
            manifest.plugin.homepage,
            Some("https://example.com".to_string())
        );
        assert_eq!(manifest.plugin.license, Some("MIT".to_string()));
        assert_eq!(manifest.plugin.engines.rdatastation, "^0.1.0");

        let frontend = manifest.capabilities.frontend.unwrap();
        assert_eq!(frontend.entry, "./extension.ts");
        assert_eq!(frontend.activation_events, vec!["onStartup"]);

        let wasm = manifest.capabilities.wasm.unwrap();
        assert_eq!(wasm.entry, "./plugin.wasm");
        assert_eq!(wasm.max_memory_mb, Some(256));
        assert_eq!(wasm.max_cpu_time_ms, Some(15000));
        assert_eq!(wasm.allowed_host_functions, vec!["db_query"]);

        assert_eq!(manifest.permissions.frontend, vec!["data:query"]);
        assert_eq!(manifest.permissions.wasm, vec!["wasm:db_query"]);

        assert_eq!(manifest.contributes.commands.len(), 1);
        assert_eq!(manifest.contributes.commands[0].id, "test.hello");

        assert_eq!(manifest.contributes.panels.len(), 1);
        assert_eq!(manifest.contributes.panels[0].id, "test.panel");
        assert_eq!(manifest.contributes.panels[0].location, "right");

        assert_eq!(manifest.contributes.drivers.len(), 1);
        assert_eq!(manifest.contributes.drivers[0].id, "testdb");

        assert_eq!(manifest.contributes.settings.len(), 1);
        assert_eq!(manifest.contributes.settings[0].key, "test.enabled");

        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.dependencies[0].id, "com.example.base");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_parse_manifest_missing_plugin_id() {
        let toml_content = r#"
[plugin]
# id is intentionally missing
name = "Test Plugin"
version = "1.0.0"
publisher = "Example Corp"

[plugin.engines]
rdatastation = "^0.1.0"

[capabilities.frontend]
entry = "./extension.ts"
"#;

        let path = write_temp_toml(toml_content);
        let result = ManifestParser::parse(&path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("plugin.id") || msg.contains("missing field"),
            "Error should mention plugin.id, got: {}",
            msg
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_parse_manifest_incompatible_engine_version() {
        let toml_content = r#"
[plugin]
id = "com.example.test"
name = "Test Plugin"
version = "1.0.0"
publisher = "Example Corp"

[plugin.engines]
rdatastation = "^99.0.0"

[capabilities.frontend]
entry = "./extension.ts"
"#;

        let path = write_temp_toml(toml_content);
        let result = ManifestParser::parse(&path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("requires engine version") || msg.contains("version"),
            "Error should mention version incompatibility, got: {}",
            msg
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_parse_manifest_missing_publisher() {
        let toml_content = r#"
[plugin]
id = "com.example.test"
name = "Test Plugin"
version = "1.0.0"
# publisher intentionally missing

[plugin.engines]
rdatastation = "^0.1.0"

[capabilities.frontend]
entry = "./extension.ts"
"#;

        let path = write_temp_toml(toml_content);
        let result = ManifestParser::parse(&path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("publisher") || msg.contains("missing field"),
            "Error should mention publisher, got: {}",
            msg
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_parse_manifest_missing_capabilities() {
        let toml_content = r#"
[plugin]
id = "com.example.test"
name = "Test Plugin"
version = "1.0.0"
publisher = "Example Corp"

[plugin.engines]
rdatastation = "^0.1.0"
"#;

        let path = write_temp_toml(toml_content);
        let manifest: PluginManifest = toml::from_str(toml_content).unwrap();
        let result = manifest.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("capability"),
            "Error should mention capability, got: {}",
            msg
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_check_engine_compatibility_same_major() {
        let manifest = PluginManifest {
            plugin: PluginMeta {
                id: "test".into(),
                name: "test".into(),
                version: "1.0.0".into(),
                publisher: "test".into(),
                description: String::new(),
                icon: None,
                homepage: None,
                license: None,
                engines: PluginEngines {
                    rdatastation: "^0.1.0".into(),
                },
            },
            capabilities: PluginCapabilities {
                frontend: Some(CapabilitiesFrontend {
                    entry: "test.js".into(),
                    activation_events: vec![],
                }),
                wasm: None,
            },
            permissions: PluginPermissions::default(),
            contributes: PluginContributes::default(),
            dependencies: vec![],
        };

        assert!(manifest.check_engine_compatibility("0.1.0").is_ok());
        assert!(manifest.check_engine_compatibility("0.2.0").is_ok());
        assert!(manifest.check_engine_compatibility("0.10.0").is_ok());
    }

    #[test]
    fn test_check_engine_compatibility_different_major() {
        let manifest = PluginManifest {
            plugin: PluginMeta {
                id: "test".into(),
                name: "test".into(),
                version: "1.0.0".into(),
                publisher: "test".into(),
                description: String::new(),
                icon: None,
                homepage: None,
                license: None,
                engines: PluginEngines {
                    rdatastation: "^1.0.0".into(),
                },
            },
            capabilities: PluginCapabilities {
                frontend: Some(CapabilitiesFrontend {
                    entry: "test.js".into(),
                    activation_events: vec![],
                }),
                wasm: None,
            },
            permissions: PluginPermissions::default(),
            contributes: PluginContributes::default(),
            dependencies: vec![],
        };

        assert!(manifest.check_engine_compatibility("1.0.0").is_ok());
        assert!(manifest.check_engine_compatibility("0.5.0").is_err());
    }

    #[test]
    fn test_default_values() {
        let toml_content = r#"
[plugin]
id = "minimal.plugin"
name = "Minimal"
version = "1.0.0"
publisher = "Minimal Corp"

[plugin.engines]
rdatastation = "^0.1.0"

[capabilities.wasm]
entry = "./plugin.wasm"
"#;

        let path = write_temp_toml(toml_content);
        let result = ManifestParser::parse(&path);
        assert!(result.is_ok());

        let manifest = result.unwrap();
        let wasm = manifest.capabilities.wasm.unwrap();
        assert_eq!(wasm.max_memory_mb, None);
        assert_eq!(wasm.max_cpu_time_ms, None);
        assert!(wasm.allowed_host_functions.is_empty());
        assert!(manifest.dependencies.is_empty());
        assert!(manifest.contributes.commands.is_empty());

        let _ = std::fs::remove_file(&path);
    }
}
