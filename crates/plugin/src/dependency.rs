//! 插件依赖管理
//!
//! 处理插件之间的依赖关系，支持循环依赖检测和版本兼容性检查

use shared::error::{CoreError, PluginError};
use engine::persistence::global_db::GlobalDatabaseManager;
use engine::persistence::plugin_store::Plugin;
use crate::manifest::{PluginDependency, PluginManifest};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

/// 依赖解析结果
#[derive(Debug, Clone)]
pub struct DependencyResolution {
    /// 要安装的插件列表（按安装顺序排列）
    pub to_install: Vec<PluginDependency>,
    /// 已存在的插件
    pub existing: Vec<Plugin>,
    /// 缺失的依赖
    pub missing: Vec<PluginDependency>,
}

/// 插件依赖管理器
pub struct DependencyManager {
    db_manager: &'static GlobalDatabaseManager,
}

impl DependencyManager {
    /// 创建新的依赖管理器
    pub fn new(db_manager: &'static GlobalDatabaseManager) -> Self {
        Self { db_manager }
    }

    /// 解析插件依赖
    pub async fn resolve_dependencies(
        &self,
        manifest: &PluginManifest,
    ) -> Result<DependencyResolution, CoreError> {
        let mut to_install = Vec::new();
        let mut existing = Vec::new();
        let mut missing = Vec::new();
        let mut visited = HashSet::new();

        // 递归解析依赖
        self.resolve_deps_recursive(
            &manifest.dependencies,
            &mut to_install,
            &mut existing,
            &mut missing,
            &mut visited,
            &mut HashSet::new(),
        )
        .await?;

        Ok(DependencyResolution {
            to_install,
            existing,
            missing,
        })
    }

    /// 递归解析依赖（async 递归需 boxing —— 内部实现，供 [`Self::resolve_dependencies`] 调用）
    async fn resolve_deps_recursive(
        &self,
        deps: &[PluginDependency],
        to_install: &mut Vec<PluginDependency>,
        existing: &mut Vec<Plugin>,
        missing: &mut Vec<PluginDependency>,
        visited: &mut HashSet<String>,
        recursion_stack: &mut HashSet<String>,
    ) -> Result<(), CoreError> {
        self.resolve_deps_inner(
            deps,
            to_install,
            existing,
            missing,
            visited,
            recursion_stack,
        )
        .await
    }

    /// async 递归的实际载体（`Pin<Box<dyn Future>>` 打破无限大小 future；见 E0733）
    fn resolve_deps_inner<'a>(
        &'a self,
        deps: &'a [PluginDependency],
        to_install: &'a mut Vec<PluginDependency>,
        existing: &'a mut Vec<Plugin>,
        missing: &'a mut Vec<PluginDependency>,
        visited: &'a mut HashSet<String>,
        recursion_stack: &'a mut HashSet<String>,
    ) -> Pin<Box<dyn Future<Output = Result<(), CoreError>> + Send + 'a>> {
        Box::pin(async move {
            for dep in deps {
                // 检查循环依赖（2026-10-15：循环依赖此前误报 dependency_missing ——
                // PluginError 没有 Cyclic 变体，这里用通用错误如实说明）
                if recursion_stack.contains(&dep.id) {
                    return Err(CoreError::common(
                        shared::error::CommonError::General(
                            format!("检测到循环依赖: {}", dep.id),
                        ),
                    ));
                }

                if visited.contains(&dep.id) {
                    continue;
                }
                visited.insert(dep.id.clone());
                recursion_stack.insert(dep.id.clone());

                // 检查插件是否已存在
                if let Some(plugin) = self
                    .db_manager
                    .get_plugin_by_code_version(&dep.id, &dep.version)
                    .await?
                {
                    if plugin.is_enabled {
                        existing.push(plugin);
                    } else {
                        // 插件存在但未启用，需要启用
                        to_install.push(dep.clone());
                    }
                } else {
                    // 检查是否有其他版本的插件
                    let all_plugins = self.db_manager.get_all_plugins().await?;
                    let compatible = all_plugins.iter().find(|p| {
                        p.code == dep.id
                            && Self::is_version_compatible(&p.version, &dep.version)
                    });

                    if let Some(plugin) = compatible {
                        existing.push(plugin.clone());
                    } else {
                        missing.push(dep.clone());
                    }
                }

                // 递归解析该依赖的依赖：从已安装插件的 manifest_json 读取（2026-10-15 补全，
                // 此前只处理一层、不递归子依赖）
                if let Some(plugin) = existing.last() {
                    if let Some(mj) = &plugin.manifest_json {
                        if let Ok(sub_manifest) = serde_json::from_str::<PluginManifest>(mj) {
                            self.resolve_deps_inner(
                                &sub_manifest.dependencies,
                                to_install,
                                existing,
                                missing,
                                visited,
                                recursion_stack,
                            )
                            .await?;
                        }
                    }
                }

                recursion_stack.remove(&dep.id);
            }

            Ok(())
        })
    }

    /// 检查版本兼容性（语义化版本数值比较 —— 此前字符串比较会把 "9" 排在 "10" 之后，D9 修复）
    fn is_version_compatible(installed: &str, required: &str) -> bool {
        fn parse_version(v: &str) -> Vec<u64> {
            v.trim_start_matches(['^', '~', '>', '<', '='])
                .split('.')
                .filter_map(|s| s.parse::<u64>().ok())
                .collect()
        }

        let installed_parts = parse_version(installed);
        let required_parts = parse_version(required);

        if installed_parts.len() < 2 || required_parts.len() < 2 {
            // 无法解析成至少两段版本号时退化为精确匹配
            return installed == required;
        }

        // 对于 ^0.1.0，检查大版本相同、小版本 ≥
        if required.starts_with('^') {
            return installed_parts[0] == required_parts[0]
                && installed_parts[1] >= required_parts[1];
        }

        // 对于 ~0.1.0，只检查大版本和小版本相同
        if required.starts_with('~') {
            return installed_parts[0] == required_parts[0]
                && installed_parts[1] == required_parts[1];
        }

        // 精确匹配
        installed == required
    }

    /// 验证依赖满足
    pub async fn validate_dependencies(&self, manifest: &PluginManifest) -> Result<(), CoreError> {
        let resolution = self.resolve_dependencies(manifest).await?;

        if !resolution.missing.is_empty() {
            let missing_str = resolution
                .missing
                .iter()
                .map(|d| format!("{}@{}", d.id, d.version))
                .collect::<Vec<_>>()
                .join(", ");

            return Err(CoreError::plugin(PluginError::dependency_missing(
                String::new(),
                String::new(),
                missing_str,
            )));
        }

        Ok(())
    }

    /// 获取插件的依赖链（2026-10-15：从 manifest_json 解析依赖并递归，此前只返回自身）
    pub async fn get_dependency_chain(&self, plugin_id: &str) -> Result<Vec<String>, CoreError> {
        let plugin = self
            .db_manager
            .get_plugin(plugin_id)
            .await?
            .ok_or_else(|| CoreError::plugin(PluginError::not_found(plugin_id.to_string())))?;

        let mut chain = vec![plugin_id.to_string()];
        let mut visited = HashSet::new();
        visited.insert(plugin_id.to_string());

        let mut queue = vec![plugin];
        while let Some(current) = queue.pop() {
            if let Some(mj) = &current.manifest_json {
                if let Ok(manifest) = serde_json::from_str::<PluginManifest>(mj) {
                    for dep in &manifest.dependencies {
                        if visited.insert(dep.id.clone()) {
                            if let Some(dep_plugin) = self
                                .db_manager
                                .get_plugin_by_code_version(&dep.id, &dep.version)
                                .await?
                            {
                                chain.push(dep.id.clone());
                                queue.push(dep_plugin);
                            }
                        }
                    }
                }
            }
        }

        Ok(chain)
    }

    /// 检查插件是否被其他插件依赖（2026-10-15：遍历全部插件解析 manifest_json，
    /// 此前直接返回空 Vec）
    pub async fn check_dependents(&self, plugin_id: &str) -> Result<Vec<String>, CoreError> {
        let all_plugins = self.db_manager.get_all_plugins().await?;
        let mut dependents = Vec::new();

        for plugin in all_plugins {
            if plugin.code == plugin_id {
                continue;
            }

            if let Some(mj) = &plugin.manifest_json {
                if let Ok(manifest) = serde_json::from_str::<PluginManifest>(mj) {
                    if manifest.dependencies.iter().any(|d| d.id == plugin_id) {
                        dependents.push(plugin.code.clone());
                    }
                }
            }
        }

        Ok(dependents)
    }
}

/// 依赖状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyStatus {
    /// 满足
    Satisfied,
    /// 版本不兼容
    VersionMismatch,
    /// 缺失
    Missing,
    /// 循环依赖
    Cyclic,
}

impl DependencyStatus {
    pub fn is_ok(&self) -> bool {
        matches!(self, DependencyStatus::Satisfied)
    }
}

/// 依赖检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyCheckResult {
    pub plugin_id: String,
    pub dependencies: Vec<(String, DependencyStatus)>,
}
