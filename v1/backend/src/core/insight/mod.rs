pub mod rule_executor;
pub mod rule_registry;
pub mod rule_types;
pub mod schema_analyzer;

use include_dir::{include_dir, Dir};
use std::sync::{OnceLock, RwLock};

pub use rule_executor::RuleExecutor;
pub use rule_registry::{get_project_rules_dir, RuleRegistry};
pub use rule_types::{
    ExecutionResult, OutputField, QualityCheck, QualityReport, QualityRule, RenderHint, RuleFile,
    RuleMeta, RuleQuery,
};
pub use schema_analyzer::{
    ForeignKeyCandidate, OrphanTable, RedundantColumn, SchemaAnalyzer, SchemaInsightReport,
    TableColumnInfo, TypeMismatch, TypeMismatchEntry,
};

pub const BUILTIN_RULES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/insight-rules");

static GLOBAL_REGISTRY: OnceLock<RwLock<RuleRegistry>> = OnceLock::new();

pub fn global_registry() -> &'static RwLock<RuleRegistry> {
    GLOBAL_REGISTRY.get_or_init(|| {
        let mut registry = RuleRegistry::new();
        if let Err(e) = registry.load_from_embedded_dir(&BUILTIN_RULES_DIR) {
            tracing::warn!("Failed to load built-in insight rules: {}", e);
        }
        RwLock::new(registry)
    })
}

pub fn load_user_rules(project_path: &std::path::Path) {
    let user_dir = get_project_rules_dir(project_path);
    if !user_dir.exists() {
        return;
    }
    match global_registry().write() {
        Ok(mut reg) => match reg.load_from_dir(&user_dir) {
            Ok(count) => tracing::info!(
                "Loaded {} user insight rules from {}",
                count,
                user_dir.display()
            ),
            Err(e) => tracing::warn!(
                "Failed to load user insight rules from {}: {}",
                user_dir.display(),
                e
            ),
        },
        Err(e) => tracing::warn!("Failed to acquire registry write lock: {}", e),
    }
}

pub fn reload_insight_rules(project_path: &std::path::Path) {
    match global_registry().write() {
        Ok(mut reg) => {
            *reg = RuleRegistry::new();
            if let Err(e) = reg.load_from_embedded_dir(&BUILTIN_RULES_DIR) {
                tracing::warn!("Failed to reload built-in insight rules: {}", e);
            }
            let user_dir = get_project_rules_dir(project_path);
            if user_dir.exists() {
                match reg.load_from_dir(&user_dir) {
                    Ok(count) => tracing::info!(
                        "Reloaded {} user insight rules from {}",
                        count,
                        user_dir.display()
                    ),
                    Err(e) => tracing::warn!(
                        "Failed to reload user insight rules from {}: {}",
                        user_dir.display(),
                        e
                    ),
                }
            }
            tracing::info!("Insight rules hot-reloaded successfully");
        }
        Err(e) => tracing::warn!(
            "Failed to acquire registry write lock for hot-reload: {}",
            e
        ),
    }
}
