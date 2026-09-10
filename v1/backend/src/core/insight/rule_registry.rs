use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::error::{CommonError, CoreError};
use include_dir::Dir;
use tracing;

use super::rule_types::RuleFile;

pub struct RuleRegistry {
    rules: HashMap<String, RuleFile>,
    by_category: HashMap<String, Vec<String>>,
}

impl Default for RuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleRegistry {
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
            by_category: HashMap::new(),
        }
    }

    pub fn load_from_dir(&mut self, dir: &Path) -> Result<usize, CoreError> {
        if !dir.exists() || !dir.is_dir() {
            return Ok(0);
        }
        let mut count = 0;
        self.scan_directory(dir, &mut count)?;
        Ok(count)
    }

    fn scan_directory(&mut self, dir: &Path, count: &mut usize) -> Result<(), CoreError> {
        let entries = fs::read_dir(dir).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Cannot read rules directory '{}': {}",
                dir.display(),
                e
            )))
        })?;

        let mut seen_ids = HashSet::new();

        for entry in entries {
            let entry = entry.map_err(|e| {
                CoreError::common(CommonError::General(format!("Dir entry error: {}", e)))
            })?;
            let path = entry.path();

            if path.is_dir() {
                self.scan_directory(&path, count)?;
            } else if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                match self.parse_rule_file(&path) {
                    Ok(rule) => {
                        let category = rule.meta.category.clone();
                        let id = rule.meta.id.clone();

                        if !seen_ids.insert(id.clone()) {
                            tracing::warn!(
                                "Duplicate rule ID '{}' in file '{}': rule will be overwritten by later file",
                                id,
                                path.display()
                            );
                        }

                        self.by_category
                            .entry(category)
                            .or_default()
                            .push(id.clone());

                        self.rules.insert(id, rule);
                        *count += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Skipping invalid rule file '{}': {}", path.display(), e);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn load_from_embedded_dir(&mut self, dir: &Dir) -> Result<usize, CoreError> {
        let mut count = 0;
        let mut seen_ids = HashSet::new();
        for entry in dir.entries() {
            match entry {
                include_dir::DirEntry::Dir(subdir) => {
                    count += self.load_from_embedded_dir(subdir)?;
                }
                include_dir::DirEntry::File(file) => {
                    if file.path().extension().and_then(|s| s.to_str()) == Some("toml") {
                        if let Some(content) = file.contents_utf8() {
                            match self.parse_toml(content) {
                                Ok(rule) => {
                                    let category = rule.meta.category.clone();
                                    let id = rule.meta.id.clone();

                                    if !seen_ids.insert(id.clone()) {
                                        tracing::warn!(
                                            "Duplicate rule ID '{}' in embedded file '{}': rule will be overwritten",
                                            id,
                                            file.path().display()
                                        );
                                    }

                                    self.by_category
                                        .entry(category)
                                        .or_default()
                                        .push(id.clone());
                                    self.rules.insert(id, rule);
                                    count += 1;
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Skipping invalid embedded rule '{}': {}",
                                        file.path().display(),
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(count)
    }

    fn parse_rule_file(&self, path: &Path) -> Result<RuleFile, CoreError> {
        let content = fs::read_to_string(path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Cannot read rule file '{}': {}",
                path.display(),
                e
            )))
        })?;
        self.parse_toml(&content)
    }

    fn parse_toml(&self, content: &str) -> Result<RuleFile, CoreError> {
        toml::from_str::<RuleFile>(content).map_err(|e| {
            CoreError::common(CommonError::General(format!("TOML parse error: {}", e)))
        })
    }

    pub fn get(&self, id: &str) -> Option<&RuleFile> {
        self.rules.get(id)
    }

    pub fn list_by_category(&self, category: &str) -> Vec<&RuleFile> {
        self.by_category
            .get(category)
            .map(|ids| ids.iter().filter_map(|id| self.rules.get(id)).collect())
            .unwrap_or_default()
    }

    pub fn rules_for_column_type(&self, column_type: &str) -> Vec<&RuleFile> {
        self.rules
            .values()
            .filter(|r| {
                r.meta
                    .applies_to
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(column_type))
            })
            .collect()
    }

    pub fn all_rules(&self) -> Vec<&RuleFile> {
        self.rules.values().collect()
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

pub fn get_project_rules_dir(project_path: &Path) -> PathBuf {
    project_path.join(".RSMETA").join("insight-rules")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::CoreError;

    fn sample_toml() -> &'static str {
        r#"
[meta]
id = "test-rule-1"
name = "Test Rule"
description = "A rule for testing"
version = "1.0"
category = "test"
applies_to = ["Numeric", "i64"]
builtin = true

[query]
template = "SELECT {col} FROM {table}"
parameters = ["table", "col"]
result_type = "single"

[[output]]
json_name = "result"
sql_name = "col"
value_type = "f64"
"#
    }

    #[test]
    fn test_parse_toml_valid() -> Result<(), CoreError> {
        let registry = RuleRegistry::new();
        let rule = registry.parse_toml(sample_toml())?;
        assert_eq!(rule.meta.id, "test-rule-1");
        assert_eq!(rule.meta.name, "Test Rule");
        assert_eq!(rule.meta.category, "test");
        assert_eq!(rule.meta.applies_to, vec!["Numeric", "i64"]);
        assert_eq!(rule.query.parameters, vec!["table", "col"]);
        assert_eq!(rule.output.len(), 1);
        assert_eq!(rule.output[0].json_name, "result");
        Ok(())
    }

    #[test]
    fn test_parse_toml_invalid() {
        let registry = RuleRegistry::new();
        let result = registry.parse_toml("not valid toml {{");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_and_list() {
        let registry = RuleRegistry::new();
        let rule = registry.parse_toml(sample_toml()).unwrap();

        let mut reg = RuleRegistry::new();
        reg.rules.insert(rule.meta.id.clone(), rule);

        assert!(reg.get("test-rule-1").is_some());
        assert!(reg.get("nonexistent").is_none());
        assert_eq!(reg.rule_count(), 1);
    }

    #[test]
    fn test_list_by_category() {
        let mut registry = RuleRegistry::new();
        let rule = registry.parse_toml(sample_toml()).unwrap();

        let mut by_cat = std::collections::HashMap::new();
        by_cat.insert(rule.meta.category.clone(), vec![rule.meta.id.clone()]);
        registry.rules.insert(rule.meta.id.clone(), rule);
        registry.by_category = by_cat;

        assert_eq!(registry.list_by_category("test").len(), 1);
        assert_eq!(registry.list_by_category("unknown").len(), 0);
    }

    #[test]
    fn test_rules_for_column_type() -> Result<(), CoreError> {
        let mut registry = RuleRegistry::new();
        let rule = registry.parse_toml(sample_toml())?;
        registry.rules.insert(rule.meta.id.clone(), rule);

        let numeric_rules = registry.rules_for_column_type("Numeric");
        assert_eq!(numeric_rules.len(), 1);

        let i64_rules = registry.rules_for_column_type("i64");
        assert_eq!(i64_rules.len(), 1);

        let text_rules = registry.rules_for_column_type("Text");
        assert_eq!(text_rules.len(), 0);
        Ok(())
    }

    #[test]
    fn test_all_rules() {
        let mut registry = RuleRegistry::new();
        let rule = registry.parse_toml(sample_toml()).unwrap();
        registry.rules.insert(rule.meta.id.clone(), rule);

        assert_eq!(registry.all_rules().len(), 1);
    }

    #[test]
    fn test_new_registry_is_empty() {
        let registry = RuleRegistry::new();
        assert_eq!(registry.rule_count(), 0);
        assert!(registry.all_rules().is_empty());
    }
}
