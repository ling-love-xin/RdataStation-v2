//! 草稿箱文件源：清单快照、可见文件与组头说明。
//!
//! 文件源**不分档、不跟词走**（过滤是本地的事）：只在「打开」边沿向后台取一次
//! 扁平清单（[`QuickOpenPalette::schedule_file_list`]），结果由回填泵收进
//! [`FilesSnapshot`]（键 = 项目根，切项目后旧快照不匹配）。

use std::path::PathBuf;

use gpui_kit::*;

use crate::quick_open::model;

use super::QuickOpenPalette;

/// 文件源快照（键 = 项目根：切项目后旧快照不匹配，不会显示别的项目的文件）。
pub(super) struct FilesSnapshot {
    pub(super) root: PathBuf,
    pub(super) files: Vec<model::FileObject>,
    /// 后台因 [上限](scratchpad::jobs::FLAT_FILE_LIMIT) 截断过（行上要说「没全列」）。
    pub(super) truncated: bool,
}

impl QuickOpenPalette {
    /// 当前项目根对应的文件清单（切项目 / 未打开项目时为空）。
    pub(super) fn visible_files(&self) -> &[model::FileObject] {
        let Some(root) = self.shared.project_root() else {
            return &[];
        };
        self.files
            .as_ref()
            .filter(|snapshot| snapshot.root == root)
            .map(|snapshot| snapshot.files.as_slice())
            .unwrap_or(&[])
    }

    /// 文件组的组头补充（清单被后台截断时要说清「没全列」）。
    pub(super) fn files_note(&self) -> String {
        let Some(root) = self.shared.project_root() else {
            return String::new();
        };
        match self.files.as_ref().filter(|snapshot| snapshot.root == root) {
            Some(snapshot) if snapshot.truncated => format!(
                "文件较多，只列前 {} 个（继续输入缩小范围）",
                scratchpad::jobs::FLAT_FILE_LIMIT
            ),
            _ => String::new(),
        }
    }

    /// 给后台排一次**扁平文件清单**（每次打开一次；结果由泵回填）。
    ///
    /// 与元数据搜索的关键差别：文件源**不分档、不跟词走**（过滤是本地的事），
    /// 所以不需要防抖，也不需要随输入重发——只在“打开”这个边沿拿一次。
    pub(super) fn schedule_file_list(&mut self, _cx: &mut Context<Self>) {
        let Some(root) = self.shared.project_root() else {
            // 未打开项目：草稿箱是项目级能力（不留上一个项目的行）。
            self.files = None;
            self.files_requested_for = None;
            return;
        };
        if self.files_pending || self.files_requested_for.as_deref() == Some(root.as_path()) {
            return;
        }
        self.files_requested_for = Some(root.clone());
        self.files_pending = true;
        scratchpad::jobs::enqueue_flatten_files(&root);
    }
}
