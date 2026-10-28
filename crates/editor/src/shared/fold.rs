//! 编辑器共享状态 —— 折叠端口（folding_enabled/show_folds/indent_folding/关键词组）+ scan_scope
//!
//! 自 shared.rs 拆分（2026-10-29）：按能力域切分 `impl EditorShared` 方法块。
//! 端口方法全 pub（跨域互调）；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为。

use crate::model::DocumentId;
use crate::scan::ScanScope;

use super::*;

impl EditorShared {

    /// 折叠开不开：用户开关 + 能力表 + 文件档位
    ///
    /// 与 [`Self::completion_enabled`] 同形，但**不看编辑器只读**——折叠是「看结构」的能力，
    /// 只读文档（查看历史快照 / 只读预览）照样能折，把它关掉只会让长脚本更难读。
    /// 用户开关（工具栏按钮）在最前：关了折叠就是整体不喂候选（`sync_fold` 落
    /// `set_folding(false)`，与能力表关闭同口径），而不是只留一个静默判据。
    pub fn folding_enabled(&self, document: &DocumentId) -> bool {
        if !self.show_folds.get() {
            return false;
        }
        let service = self.service.borrow();
        let Some(doc) = service.find(document) else {
            return false;
        };
        doc.capabilities().folding && !doc.tier().disables_folding()
    }

    /// 用户折叠开关（工具栏按钮切换；2026-10-27 恢复折叠时加入）
    pub fn show_folds(&self) -> bool {
        self.show_folds.get()
    }

    /// 切换用户折叠开关（事件路径：按钮点击 → set + 宿主重跑 `sync_fold` + notify）
    pub fn set_show_folds(&self, on: bool) {
        self.show_folds.set(on);
    }

    /// 基于缩进的折叠开关（设置页切换；2026-10-28 加入）
    pub fn indent_folding(&self) -> bool {
        self.indent_folding.get()
    }

    /// 切换基于缩进的折叠开关（事件路径：设置页 → set + 宿主重跑 `sync_fold` + notify）
    pub fn set_indent_folding(&self, on: bool) {
        self.indent_folding.set(on);
    }

    /// 自定义开块关键字列表（折叠用）
    pub fn fold_open_keywords(&self) -> Vec<String> {
        self.fold_open_keywords.borrow().clone()
    }

    /// 设置自定义开块关键字列表（逗号分隔字符串解析后）
    pub fn set_fold_open_keywords(&self, keywords: Vec<String>) {
        *self.fold_open_keywords.borrow_mut() = keywords;
    }

    /// 自定义闭块关键字列表（折叠用）
    pub fn fold_close_keywords(&self) -> Vec<String> {
        self.fold_close_keywords.borrow().clone()
    }

    /// 设置自定义闭块关键字列表（逗号分隔字符串解析后）
    pub fn set_fold_close_keywords(&self, keywords: Vec<String>) {
        *self.fold_close_keywords.borrow_mut() = keywords;
    }

    /// 【B18】后台扫描的范围（折叠 / 诊断两个重活开不开）
    ///
    /// 判据只有一处：档位的降级表 [`crate::limits::FileTier::plan`]——不在这里另写一套
    /// `matches!(tier, Normal)`。只读不影响扫描（看结构不需要写权限）。
    pub fn scan_scope(&self, document: &DocumentId) -> ScanScope {
        let service = self.service.borrow();
        let Some(doc) = service.find(document) else {
            return ScanScope::none();
        };
        let plan = doc.tier().plan();
        ScanScope {
            folds: plan.folding && doc.capabilities().folding,
            diagnostics: plan.diagnostics,
        }
    }
}
