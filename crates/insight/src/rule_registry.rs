use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use include_dir::Dir;
use shared::error::{CommonError, CoreError};
use tracing;

use super::rule::{RuleLoadFailure, RuleScope, RuleSource};
use super::rule_types::RuleFile;

/// 规则注册表：内存中的规则集（按项目根缓存，见 `crate::registry_for`）。
///
/// 不维护分类索引：同名规则可被后加载的作用域整体覆盖（内置 → 全局 → 项目），
/// 独立索引在覆盖时无法清理旧条目，会同时产生「重复项」与「一规则现身两个分类」。
/// 规则总量在数十条量级，按需派生（[`Self::list_by_category`]）成本可忽略。
pub struct RuleRegistry {
    rules: HashMap<String, RuleFile>,
    /// 每条规则的**实际来源**（被覆盖时同步改写），供界面展示「这条规则从哪来」。
    sources: HashMap<String, RuleSource>,
    /// 解析失败记录：单条失败不连坐，仅登记以便界面展示错误原文。
    failures: Vec<RuleLoadFailure>,
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
            sources: HashMap::new(),
            failures: Vec::new(),
        }
    }

    /// 从文件系统目录加载一层规则（`Global` 或 `Project`）。
    ///
    /// 目录不存在返回 0 条而不是报错：项目无自有规则是常态。
    pub fn load_from_dir(
        &mut self,
        dir: &Path,
        scope: RuleScope,
    ) -> Result<usize, CoreError> {
        if !dir.exists() || !dir.is_dir() {
            return Ok(0);
        }
        let mut count = 0;
        self.scan_directory(dir, scope, &mut count)?;
        Ok(count)
    }

    fn scan_directory(
        &mut self,
        dir: &Path,
        scope: RuleScope,
        count: &mut usize,
    ) -> Result<(), CoreError> {
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
                self.scan_directory(&path, scope, count)?;
            } else if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let display_path = path.display().to_string();
                match self.parse_rule_file(&path) {
                    Ok(rule) => {
                        let id = rule.meta.id.clone();

                        if !seen_ids.insert(id.clone()) {
                            tracing::warn!(
                                "Duplicate rule ID '{}' in file '{}': rule will be overwritten by later file",
                                id,
                                display_path
                            );
                        }

                        self.insert_rule(id, rule, scope, display_path);
                        *count += 1;
                    }
                    Err(e) => self.record_failure(scope, display_path, e),
                }
            }
        }
        Ok(())
    }

    /// 加载内置规则（`include_dir!` 编译期嵌入）。
    ///
    /// 内嵌条目无文件系统路径，`RuleSource.path` 记为 `<builtin>/<虚拟路径>`，
    /// 仅用于界面展示与排障（该层不可写，没有「打开文件」动作）。
    pub fn load_builtin(&mut self, dir: &Dir) -> Result<usize, CoreError> {
        self.load_embedded_dir(dir, RuleScope::Builtin)
    }

    fn load_embedded_dir(&mut self, dir: &Dir, scope: RuleScope) -> Result<usize, CoreError> {
        let mut count = 0;
        let mut seen_ids = HashSet::new();
        for entry in dir.entries() {
            match entry {
                include_dir::DirEntry::Dir(subdir) => {
                    count += self.load_embedded_dir(subdir, scope)?;
                }
                include_dir::DirEntry::File(file) => {
                    if file.path().extension().and_then(|s| s.to_str()) == Some("toml") {
                        if let Some(content) = file.contents_utf8() {
                            let display_path = format!("<builtin>/{}", file.path().display());
                            match parse_rule_toml(content) {
                                Ok(rule) => {
                                    let id = rule.meta.id.clone();

                                    if !seen_ids.insert(id.clone()) {
                                        tracing::warn!(
                                            "Duplicate rule ID '{}' in embedded file '{}': rule will be overwritten",
                                            id,
                                            display_path
                                        );
                                    }

                                    self.insert_rule(id, rule, scope, display_path);
                                    count += 1;
                                }
                                Err(e) => self.record_failure(scope, display_path, e),
                            }
                        }
                    }
                }
            }
        }
        Ok(count)
    }

    /// 插入/覆盖一条规则，并同步其来源。
    fn insert_rule(&mut self, id: String, rule: RuleFile, scope: RuleScope, path: String) {
        self.sources.insert(id.clone(), RuleSource::new(scope, path));
        self.rules.insert(id, rule);
    }

    /// 登记一次解析失败（不中断其余规则的加载）。
    fn record_failure(&mut self, scope: RuleScope, path: String, err: CoreError) {
        tracing::warn!("Skipping invalid rule '{}': {}", path, err);
        self.failures.push(RuleLoadFailure {
            scope,
            path,
            error: err.to_string(),
        });
    }

    fn parse_rule_file(&self, path: &Path) -> Result<RuleFile, CoreError> {
        let content = fs::read_to_string(path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Cannot read rule file '{}': {}",
                path.display(),
                e
            )))
        })?;
        parse_rule_toml(&content)
    }

    pub fn get(&self, id: &str) -> Option<&RuleFile> {
        self.rules.get(id)
    }

    /// 按分类列出规则。
    ///
    /// **从 `rules` 实时派生**，不维护独立索引：
    /// 规则可被后加载的同名规则整体覆盖（内置 → 全局 → 项目），若索引与 `rules` 分开维护，
    /// 覆盖时旧分类条目无法清理，会同时产生「重复项」与「一规则现身两个分类」两种不一致。
    /// 规则总量在数十条量级，派生成本可忽略。
    pub fn list_by_category(&self, category: &str) -> Vec<&RuleFile> {
        self.rules
            .values()
            .filter(|r| r.meta.category == category)
            .collect()
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

    /// 规则的**实际来源**（被覆盖后指向胜出的那一层）。
    pub fn source_of(&self, id: &str) -> Option<&RuleSource> {
        self.sources.get(id)
    }

    /// 全部规则来源（`id → 来源`），供界面分组展示。
    pub fn sources(&self) -> &HashMap<String, RuleSource> {
        &self.sources
    }

    /// 按作用域列出规则（界面按「项目 / 全局 / 内置」分组用）。
    pub fn list_by_scope(&self, scope: RuleScope) -> Vec<&RuleFile> {
        self.rules
            .iter()
            .filter(|(id, _)| {
                self.sources
                    .get(id.as_str())
                    .is_some_and(|s| s.scope == scope)
            })
            .map(|(_, rule)| rule)
            .collect()
    }

    /// 实际出现在注册表中的作用域（用于界面只在有内容时渲染分组）。
    pub fn scopes_present(&self) -> Vec<RuleScope> {
        let mut scopes: Vec<RuleScope> = self
            .sources
            .values()
            .map(|s| s.scope)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        scopes.sort();
        scopes
    }

    /// 解析失败记录（不连坐的那部分规则）。
    pub fn failures(&self) -> &[RuleLoadFailure] {
        &self.failures
    }

    /// 清空失败记录（重建注册表前的显式重置；加载过程会自行追加）。
    pub fn clear_failures(&mut self) {
        self.failures.clear();
    }

    /// 按 id 集合摘除规则（用于应用「禁用」），返回实际摘除的条数。
    ///
    /// 同时清理 `sources`：来源描述必须与实际生效的规则集一致，
    /// 留着被摘规则的行会让界面显示一条“存在但用不了”的规则。
    pub fn remove_rules(&mut self, ids: &HashSet<String>) -> usize {
        let mut removed = 0;
        for id in ids {
            if self.rules.remove(id).is_some() {
                self.sources.remove(id);
                removed += 1;
            }
        }
        removed
    }
}

/// 解析单条规则的 TOML 正文。
///
/// 独立成自由函数而非注册表方法：**索引器需要它**——索引要覆盖「解析失败的规则文件」
/// （这正是索引存在的意义：把静默跳过的规则变成可见的错误），所以不能只依赖注册表。
pub fn parse_rule_toml(content: &str) -> Result<RuleFile, CoreError> {
    toml::from_str::<RuleFile>(content).map_err(|e| {
        CoreError::common(CommonError::General(format!("TOML parse error: {}", e)))
    })
}

/// 项目级规则目录：`{项目}/.RSmeta/insight-rules/`。
///
/// 目录名复用 engine 的权威常量（与 `project::store` 同一拼写），
/// 不在本模块硬编码字符串——历史遗留的 `.RSMETA` 写法在大小写敏感文件系统上会分叉到两个目录。
pub fn get_project_rules_dir(project_path: &Path) -> PathBuf {
    project_path
        .join(engine::persistence::connection_org_store::RS_META_DIR_NAME)
        .join(RULES_DIR_NAME)
}

/// 用户全局规则目录：`{系统目录}/insight-rules/`（跨项目复用，作用域见 `RuleScope`）。
pub fn get_global_rules_dir(system_dir: &Path) -> PathBuf {
    system_dir.join(RULES_DIR_NAME)
}

/// 规则目录名（项目级与全局级同名，仅父目录不同）。
pub const RULES_DIR_NAME: &str = "insight-rules";

#[cfg(test)]
mod tests {
    use super::*;
    // 测试需要真实文件系统：显式导入，不依赖父模块 use 的 glob 传递
    use std::fs;
    use std::path::{Path, PathBuf};
    use shared::error::CoreError;

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
        let rule = parse_rule_toml(sample_toml())?;
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
        let result = parse_rule_toml("not valid toml {{");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_and_list() {
        let rule = parse_rule_toml(sample_toml()).unwrap();

        let mut reg = RuleRegistry::new();
        reg.rules.insert(rule.meta.id.clone(), rule);

        assert!(reg.get("test-rule-1").is_some());
        assert!(reg.get("nonexistent").is_none());
        assert_eq!(reg.rule_count(), 1);
    }

    #[test]
    fn test_list_by_category() {
        let mut registry = RuleRegistry::new();
        let rule = parse_rule_toml(sample_toml()).unwrap();
        registry.rules.insert(rule.meta.id.clone(), rule);

        assert_eq!(registry.list_by_category("test").len(), 1);
        assert_eq!(registry.list_by_category("unknown").len(), 0);
    }

    /// 建一个本次调用专属的临时规则目录（测试并行安全：目录名含 pid 与 tag）。
    fn temp_rules_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds_insight_rules_{}_{}",
            std::process::id(),
            tag
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp rules dir");
        dir
    }

    /// 回归（T2a）：同名同分类覆盖 → 列表不得出现重复项。
    ///
    /// 旧实现维护独立的 `by_category` 索引且只增不减，覆盖时同一 id 会被 push 两次，
    /// `list_by_category` 于是返回两条**同一条规则**。
    /// 分两次 `load_from_dir` 调用（模拟内置 → 项目两层），避免目录内读取顺序不确定。
    #[test]
    fn test_override_same_category_has_no_duplicate() -> Result<(), CoreError> {
        let base = temp_rules_dir("same_cat_base");
        let user = temp_rules_dir("same_cat_user");
        fs::write(base.join("r.rule.toml"), sample_toml()).expect("write base rule");
        fs::write(
            user.join("r.rule.toml"),
            sample_toml().replace(r#"version = "1.0""#, r#"version = "2.0""#),
        )
        .expect("write user rule");

        let mut registry = RuleRegistry::new();
        assert_eq!(registry.load_from_dir(&base, RuleScope::Builtin)?, 1);
        assert_eq!(registry.load_from_dir(&user, RuleScope::Project)?, 1);

        assert_eq!(registry.rule_count(), 1, "同名规则应整体覆盖为一条");
        assert_eq!(
            registry.list_by_category("test").len(),
            1,
            "同分类覆盖后不得出现重复项"
        );
        assert_eq!(registry.get("test-rule-1").unwrap().meta.version, "2.0");

        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&user);
        Ok(())
    }

    /// 回归（T2b）：同名但换分类覆盖 → 规则只出现在新分类。
    ///
    /// 旧实现的旧分类条目残留，`list_by_category` 会让一条规则**同时现身两个分类**。
    #[test]
    fn test_override_changed_category_moves() -> Result<(), CoreError> {
        let base = temp_rules_dir("chg_cat_base");
        let user = temp_rules_dir("chg_cat_user");
        fs::write(base.join("r.rule.toml"), sample_toml()).expect("write base rule");
        fs::write(
            user.join("r.rule.toml"),
            sample_toml().replace(r#"category = "test""#, r#"category = "moved""#),
        )
        .expect("write user rule");

        let mut registry = RuleRegistry::new();
        registry.load_from_dir(&base, RuleScope::Builtin)?;
        registry.load_from_dir(&user, RuleScope::Project)?;

        assert_eq!(registry.rule_count(), 1);
        assert_eq!(
            registry.list_by_category("test").len(),
            0,
            "旧分类不应残留该规则"
        );
        assert_eq!(
            registry.list_by_category("moved").len(),
            1,
            "应归入覆盖后的新分类"
        );

        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&user);
        Ok(())
    }

    /// 项目规则目录必须使用权威元数据目录名（`.RSmeta`），不得回退到历史的 `.RSMETA`。
    /// 大小写敏感文件系统上两者会分叉到不同目录。
    #[test]
    fn test_project_rules_dir_uses_canonical_meta_dir() {
        let dir = get_project_rules_dir(Path::new("/tmp/rds_proj"));
        assert!(
            dir.ends_with(Path::new(RULES_DIR_NAME)),
            "应以规则目录名结尾: {}",
            dir.display()
        );
        let s = dir.to_string_lossy();
        assert!(s.contains(".RSmeta"), "应使用权威拼写 .RSmeta: {}", s);
        assert!(!s.contains(".RSMETA"), "不得使用历史拼写 .RSMETA: {}", s);
    }

    /// 全局规则目录与项目规则目录同级同名，仅父目录不同（作用域三层中的 `Global` 层）。
    #[test]
    fn test_global_rules_dir() {
        let dir = get_global_rules_dir(Path::new("/tmp/rds_system"));
        assert!(dir.ends_with(Path::new(RULES_DIR_NAME)));
        assert!(!dir.to_string_lossy().contains(".RSmeta"));
    }

    /// 不存在的目录返回 0 条而不是报错（项目无自有规则是常态）。
    #[test]
    fn test_load_from_missing_dir_returns_zero() -> Result<(), CoreError> {
        let mut registry = RuleRegistry::new();
        let missing = std::env::temp_dir().join("rds_insight_rules_definitely_missing_dir");
        let _ = fs::remove_dir_all(&missing);
        assert_eq!(registry.load_from_dir(&missing, RuleScope::Project)?, 0);
        assert_eq!(registry.rule_count(), 0);
        Ok(())
    }

    /// 单个非法 TOML 不得连坐：同目录下合法规则照常加载。
    #[test]
    fn test_invalid_rule_does_not_block_others() -> Result<(), CoreError> {
        let dir = temp_rules_dir("invalid_mixed");
        fs::write(dir.join("ok.rule.toml"), sample_toml()).expect("write ok rule");
        fs::write(dir.join("bad.rule.toml"), "not valid toml {{").expect("write bad rule");

        let mut registry = RuleRegistry::new();
        let loaded = registry.load_from_dir(&dir, RuleScope::Project)?;

        assert_eq!(loaded, 1, "只应加载成功的那条");
        assert_eq!(registry.rule_count(), 1);
        assert!(registry.get("test-rule-1").is_some());

        // 失败必须**有出口**：登记路径与错误原文，供界面逐条展示。
        assert_eq!(registry.failures().len(), 1);
        let failure = &registry.failures()[0];
        assert_eq!(failure.scope, RuleScope::Project);
        assert!(
            failure.path.ends_with("bad.rule.toml"),
            "失败记录应包含出错文件路径: {}",
            failure.path
        );
        assert!(
            !failure.error.is_empty(),
            "失败记录应包含解析错误原文"
        );

        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    /// 来源跟随覆盖：同名规则被上层覆盖后，`source_of` 必须指向胜出的那一层。
    #[test]
    fn test_source_of_follows_override() -> Result<(), CoreError> {
        let base = temp_rules_dir("src_base");
        let user = temp_rules_dir("src_user");
        fs::write(base.join("r.rule.toml"), sample_toml()).expect("write base rule");
        fs::write(user.join("r.rule.toml"), sample_toml()).expect("write user rule");

        let mut registry = RuleRegistry::new();
        registry.load_from_dir(&base, RuleScope::Global)?;
        let after_global = registry.source_of("test-rule-1").expect("global 层来源");
        assert_eq!(after_global.scope, RuleScope::Global);
        assert!(after_global.path.ends_with("r.rule.toml"));

        registry.load_from_dir(&user, RuleScope::Project)?;
        let after_project = registry.source_of("test-rule-1").expect("project 层来源");
        assert_eq!(
            after_project.scope,
            RuleScope::Project,
            "被上层覆盖后来源应改指上层"
        );

        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&user);
        Ok(())
    }

    /// 按作用域分组：`scopes_present` 反映**实际拥有生效规则**的作用域，
    /// 被上层覆盖的规则改指上层（它已不再算在旧层）。
    #[test]
    fn test_list_by_scope_and_scopes_present() -> Result<(), CoreError> {
        let global_dir = temp_rules_dir("scope_global");
        let project_dir = temp_rules_dir("scope_project");
        // 全局层：一条将被项目层覆盖，一条保留
        fs::write(
            global_dir.join("g1.rule.toml"),
            sample_toml().replace("test-rule-1", "global-rule-1"),
        )
        .expect("write global rule 1");
        fs::write(
            global_dir.join("g2.rule.toml"),
            sample_toml().replace("test-rule-1", "global-rule-2"),
        )
        .expect("write global rule 2");
        // 项目层：覆盖 global-rule-1 + 新增 project-rule-1
        fs::write(
            project_dir.join("p1.rule.toml"),
            sample_toml().replace("test-rule-1", "global-rule-1"),
        )
        .expect("write project override");
        fs::write(
            project_dir.join("p2.rule.toml"),
            sample_toml().replace("test-rule-1", "project-rule-1"),
        )
        .expect("write project rule");

        let mut registry = RuleRegistry::new();
        registry.load_from_dir(&global_dir, RuleScope::Global)?;
        registry.load_from_dir(&project_dir, RuleScope::Project)?;

        assert_eq!(registry.rule_count(), 3, "三条不同 id 的规则");
        assert_eq!(
            registry.list_by_scope(RuleScope::Global).len(),
            1,
            "仅 global-rule-2 仍归全局层；global-rule-1 已被覆盖"
        );
        assert_eq!(registry.list_by_scope(RuleScope::Project).len(), 2);
        assert_eq!(registry.list_by_scope(RuleScope::Builtin).len(), 0);

        assert_eq!(
            registry.scopes_present(),
            vec![RuleScope::Global, RuleScope::Project],
            "来源集合应按优先级升序返回"
        );
        assert_eq!(
            registry.source_of("global-rule-1").map(|s| s.scope),
            Some(RuleScope::Project),
            "被覆盖的规则来源应改指上层"
        );
        assert_eq!(
            registry.source_of("global-rule-2").map(|s| s.scope),
            Some(RuleScope::Global),
            "未被覆盖的规则保留原来源"
        );

        let _ = fs::remove_dir_all(&global_dir);
        let _ = fs::remove_dir_all(&project_dir);
        Ok(())
    }

    /// 内置层加载与枚举（编译期内嵌，不依赖文件系统）。
    #[test]
    fn test_load_builtin_records_scope() -> Result<(), CoreError> {
        let mut registry = RuleRegistry::new();
        let loaded = registry.load_builtin(&crate::BUILTIN_RULES_DIR)?;

        assert_eq!(
            loaded,
            crate::rule_types::BUILTIN_RULE_COUNT,
            "内嵌规则数应与 BUILTIN_RULE_COUNT 一致"
        );
        assert_eq!(registry.rule_count(), loaded);
        assert!(
            registry.failures().is_empty(),
            "内置规则必须全部可解析，失败项: {:?}",
            registry.failures()
        );
        assert_eq!(registry.list_by_scope(RuleScope::Builtin).len(), loaded);

        let source = registry.source_of("null-check").expect("内置规则 null-check");
        assert_eq!(source.scope, RuleScope::Builtin);
        assert!(
            source.path.starts_with("<builtin>/"),
            "内嵌来源应带 <builtin>/ 前缀: {}",
            source.path
        );
        Ok(())
    }

    #[test]
    fn test_rules_for_column_type() -> Result<(), CoreError> {
        let mut registry = RuleRegistry::new();
        let rule = parse_rule_toml(sample_toml())?;
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
        let rule = parse_rule_toml(sample_toml()).unwrap();
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
