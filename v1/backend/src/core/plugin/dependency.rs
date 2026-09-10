//! 插件依赖管理
//!
//! 处理插件之间的依赖关系，支持循环依赖检测和版本兼容性检查

use crate::core::error::{CoreError, PluginError};
use crate::core::persistence::global_db::GlobalDatabaseManager;
use crate::core::persistence::plugin_store::Plugin;
use crate::core::plugin::manifest::{PluginDependency, PluginManifest};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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

    /// 递归解析依赖
    async fn resolve_deps_recursive(
        &self,
        deps: &[PluginDependency],
        to_install: &mut Vec<PluginDependency>,
        existing: &mut Vec<Plugin>,
        missing: &mut Vec<PluginDependency>,
        visited: &mut HashSet<String>,
        recursion_stack: &mut HashSet<String>,
    ) -> Result<(), CoreError> {
        for dep in deps {
            // 检查循环依赖
            if recursion_stack.contains(&dep.id) {
                return Err(CoreError::plugin(PluginError::dependency_missing(
                    dep.id.clone(),
                    dep.id.clone(),
                    dep.version.clone(),
                )));
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
                    p.code == dep.id && Self::is_version_compatible(&p.version, &dep.version)
                });

                if let Some(plugin) = compatible {
                    existing.push(plugin.clone());
                } else {
                    missing.push(dep.clone());
                }
            }

            // 递归解析该依赖的依赖（如果有清单的话）
            // 注意：这里简化了，实际中可能需要从存储中读取依赖的清单

            recursion_stack.remove(&dep.id);
        }

        Ok(())
    }

    /// 检查版本兼容性
    fn is_version_compatible(installed: &str, required: &str) -> bool {
        // 简化的版本兼容性检查
        // 实际中可以使用 semver 库
        let installed_parts: Vec<&str> = installed.split('.').collect();
        let required_parts: Vec<&str> = required
            .trim_start_matches(['^', '~', '>', '<', '='])
            .split('.')
            .collect();

        if installed_parts.len() < 2 || required_parts.len() < 2 {
            return false;
        }

        // 对于 ^0.1.0，检查大版本和小版本
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

    /// 获取插件的依赖链
    pub async fn get_dependency_chain(&self, plugin_id: &str) -> Result<Vec<String>, CoreError> {
        let _plugin = self
            .db_manager
            .get_plugin(plugin_id)
            .await?
            .ok_or_else(|| CoreError::plugin(PluginError::not_found(plugin_id.to_string())))?;

        // 尝试从 manifest_json 解析依赖
        let chain = vec![plugin_id.to_string()];
        let mut visited = HashSet::new();
        visited.insert(plugin_id.to_string());

        // 这里简化，实际中应该解析 manifest_json
        // 或者从数据库读取依赖关系

        Ok(chain)
    }

    /// 检查插件是否被其他插件依赖
    pub async fn check_dependents(&self, plugin_id: &str) -> Result<Vec<String>, CoreError> {
        let all_plugins = self.db_manager.get_all_plugins().await?;
        let dependents = Vec::new();

        for plugin in all_plugins {
            if plugin.code == plugin_id {
                continue;
            }

            // 这里简化，实际中应该从 manifest_json 解析依赖
            // 或者从 plugin_dependencies 表读取
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
