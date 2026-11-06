//! 索引修复：文件系统（**本体权威**）与登记表（**索引**）之间的三类差异（架构 §7.2）。
//!
//! 硬原则：**不做自动修复**。`scan` 只报告，任何改动都由调用方（界面）在用户确认后调用——
//! 静默导入会让"存档"变成用户没同意过的东西，静默删除更不可接受。
//!
//! 三类差异（与原型 §4.5 的分组一一对应）：
//! 1. `UntrackedFile`  有文件、无记录（手工放入 / 从别处拷入）→ 补登
//! 2. `MissingPayload` 有记录、无本体（被手工删除 / 移动）→ 从回收站还原（待 P0.8）/ 删除记录
//! 3. `ContentChanged` 指纹不匹配（绕过只读改了内容）→ 接受当前内容（生成新版本）/ 从历史还原

use std::collections::HashSet;

use crate::model::{ArchiveBinding, ArchiveKind, NewArchiveInput, PayloadMode};
use crate::payload::PayloadStore;
use crate::{AnalyticsResource, AnalyticsResourceStore};
use shared::error::CoreError;

/// 一条索引差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexIssue {
    /// 有文件、无记录。
    UntrackedFile { rel_path: String },
    /// 有记录、无本体。
    MissingPayload {
        resource_id: String,
        name: String,
        rel_path: String,
    },
    /// 指纹不匹配：本体被绕过只读改过。
    ContentChanged {
        resource_id: String,
        name: String,
        rel_path: String,
        /// 登记表里的指纹（旧行可能为空 = 无比对基准，此时不会产生本差异）。
        expected_hash: String,
        /// 当前实际指纹。
        actual_hash: String,
    },
}

impl IndexIssue {
    /// 相关存档 id（`UntrackedFile` 无）。
    pub fn resource_id(&self) -> Option<&str> {
        match self {
            Self::UntrackedFile { .. } => None,
            Self::MissingPayload { resource_id, .. } | Self::ContentChanged { resource_id, .. } => {
                Some(resource_id)
            }
        }
    }

    /// 相关本体相对路径。
    pub fn rel_path(&self) -> &str {
        match self {
            Self::UntrackedFile { rel_path }
            | Self::MissingPayload { rel_path, .. }
            | Self::ContentChanged { rel_path, .. } => rel_path,
        }
    }

    /// 界面分组用的一句话说明。
    pub fn summary(&self) -> String {
        match self {
            Self::UntrackedFile { rel_path } => {
                format!("resources/{rel_path}：本体存在但没有登记记录")
            }
            Self::MissingPayload { name, rel_path, .. } => {
                format!("存档「{name}」的本体缺失（期望位置 resources/{rel_path}）")
            }
            Self::ContentChanged { name, rel_path, .. } => {
                format!("存档「{name}」的内容已变（resources/{rel_path}）")
            }
        }
    }
}

/// 扫描报告。
#[derive(Debug, Clone, Default)]
pub struct IndexScanReport {
    pub issues: Vec<IndexIssue>,
}

impl IndexScanReport {
    /// 三项均为 0（界面状态行显示"干净"）。
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn untracked_files(&self) -> Vec<&IndexIssue> {
        self.pick(|issue| matches!(issue, IndexIssue::UntrackedFile { .. }))
    }

    pub fn missing_payloads(&self) -> Vec<&IndexIssue> {
        self.pick(|issue| matches!(issue, IndexIssue::MissingPayload { .. }))
    }

    pub fn content_changes(&self) -> Vec<&IndexIssue> {
        self.pick(|issue| matches!(issue, IndexIssue::ContentChanged { .. }))
    }

    fn pick(&self, predicate: impl Fn(&IndexIssue) -> bool) -> Vec<&IndexIssue> {
        self.issues
            .iter()
            .filter(|issue| predicate(issue))
            .collect()
    }
}

/// 索引修复：借用本体层与索引层（不持有，避免与 `ArchiveService` 争所有权）。
pub struct IndexRepair<'a> {
    payload: &'a PayloadStore,
    store: &'a AnalyticsResourceStore,
}

impl<'a> IndexRepair<'a> {
    pub fn new(payload: &'a PayloadStore, store: &'a AnalyticsResourceStore) -> Self {
        Self { payload, store }
    }
}

fn repair_err(operation: &str, reason: &str) -> CoreError {
    CoreError::storage(shared::error::StorageError::Persistence {
        store: "analytics_resources".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

#[cfg(test)]

mod repair;
mod scan;

#[cfg(test)]
mod tests;
