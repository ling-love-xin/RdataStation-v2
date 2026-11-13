//! 面板动作的内部助手：待办提示、项目根、草稿扫描、分组与别名解析。

use super::*;

impl ResourceHost {
    /// 给一次明确回执（写入 `Shared::notice` 并请求宿主重绘）。
    pub(super) fn pending(&self, action: &str, detail: &str, cx: &mut App) {
        self.backend.notice(format!("资产库：{action}{detail}"), cx);
    }

    /// 状态栏提示（动作结果与失败都走它）。
    pub(super) fn notice(&self, message: impl Into<String>, cx: &mut App) {
        self.backend.notice(message.into(), cx);
    }

    /// 项目根（未打开项目时给回执并返回 `None`）。
    pub(super) fn require_project(&self, action: &str, cx: &mut App) -> Option<PathBuf> {
        match self.backend.project_root() {
            Some(root) => Some(root),
            None => {
                self.pending(action, "：还没有打开项目", cx);
                None
            }
        }
    }

    pub(super) fn read_only(&self) -> bool {
        self.backend.read_only()
    }

    /// 草稿箱里的可归档文件（**宿主侧扫描，本 crate 只收候选快照**）。
    ///
    /// 2026-11-12：不再借宿主的 `ScratchpadStore` 自己遍历草稿树——那会把上游类型
    /// 拖进 M6 的事件路径。宿主返回 `Vec<DraftCandidate>`（含预取的标签与来源连接），
    /// 这里只做稳定排序与错误回执。
    pub(super) fn draft_candidates(&self, cx: &mut App) -> Option<Vec<DraftCandidate>> {
        let mut out = match self.backend.draft_candidates() {
            Ok(candidates) => candidates,
            Err(reason) => {
                self.notice(format!("资产库：读不到草稿箱（{reason}）"), cx);
                return None;
            }
        };
        out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        Some(out)
    }

    /// 分组显示名（从面板快照的分组字典取；取不到就 `None`——调用方会退回一句泛称）。
    ///
    /// 不给“直接查库”的选择：分组字典每次刷新都在快照里，而查库要开项目库（异步 + I/O），
    /// 事件路径上不该出现。
    /// 分组名单（面板快照里的字典）：归档对话框的「分组」下拉用它。
    ///
    /// 不另查库——面板的字典就是这一屏真实可见的分组（同一次取数产物）；快照还没到时
    /// 给空名单（对话框只摆「未分组」）。
    pub(super) fn known_groups(&self, cx: &App) -> Vec<GroupOption> {
        self.backend.snapshot_groups(cx)
    }

    /// 归档要预选的分组：设置里的默认分组，**且它还在字典里**。
    ///
    /// 两个都要：认不出的 id 不能当预选值（用户会以为“归档没进默认分组”，而实际是分组已删）。
    /// 返回 `None` = 未分组——与对话框自己的兑底同一口径（它也会把悬空 id 显示成未分组）。
    pub(super) fn default_group_for_archive(&self, cx: &App) -> Option<String> {
        let root = self.backend.project_root()?;
        let id = self.backend.default_group(&root.to_string_lossy(), cx)?;
        self.known_groups(cx)
            .iter()
            .any(|group| group.id == id)
            .then_some(id)
    }

    pub(super) fn group_name(&self, folder_id: &str, cx: &App) -> Option<String> {
        self.backend.group_name(folder_id, cx)
    }

    /// 一条存档的当前**显示名**（重命名对话框的预填值）。
    ///
    /// 从面板快照取（与 [`group_name`](Self::group_name) 同一处取法）：预览与真实值同源，
    /// 不为了一个名字再查一次库；取不到（行刚被刷掉）就让输入框空着，由用户自己填。
    pub(super) fn row_name(&self, resource_id: &str, cx: &App) -> Option<String> {
        self.backend.row_name(resource_id, cx)
    }

    /// 一条存档的当前**别名**（别名对话框的预填值；`None` = 没别名）。
    ///
    /// 从**详情**取（头部展示的就是它）：与 `row_name` 同一口径（不为了一个字段再查库）。
    pub(super) fn row_alias(&self, resource_id: &str, cx: &App) -> Option<Option<String>> {
        self.backend.row_alias(resource_id, cx)
    }

    /// 本体绝对路径（按归档模式分派；越界 / 点前缀守卫由 `PayloadStore` 把关）。
    ///
    /// - **copy**：`resources/` 相对路径 → `resolve`（需要项目根）；
    /// - **reference**：源**绝对**路径 → `resolve_source`（不依赖项目根；源不存在 / 已移动
    ///   由守卫报错，宿主回执说明快照失效）。
    ///
    /// 拿不到就**给回执**：旧行可能没登记路径，或项目已关闭——两种都要说清，不能静默。
    pub(super) fn payload_path(&self, detail: &ArchiveDetail, cx: &mut App) -> Option<PathBuf> {
        match detail.payload_mode {
            PayloadMode::Copy => {
                let Some(root) = self.backend.project_root() else {
                    self.pending("无法定位本体", "：还没有打开项目", cx);
                    return None;
                };
                let Some(rel) = detail.payload_rel_path.as_deref() else {
                    self.notice("资产库：这条存档没有登记本体路径（旧行）", cx);
                    return None;
                };
                match PayloadStore::new(root).resolve(rel) {
                    Ok(path) => Some(path),
                    Err(error) => {
                        self.notice(format!("资产库：无法定位本体（{error}）"), cx);
                        None
                    }
                }
            }
            PayloadMode::Reference => {
                let Some(source) = detail.source_path.as_deref() else {
                    self.notice("资产库：引用型存档没有登记源路径（数据异常）", cx);
                    return None;
                };
                // resolve_source 不读项目根（源在模块管辖目录之外）；空 root 只为满足构造签名。
                match PayloadStore::new(PathBuf::new()).resolve_source(source) {
                    Ok(path) => Some(path),
                    Err(error) => {
                        self.notice(format!("资产库：引用源不可用（{error}）"), cx);
                        None
                    }
                }
            }
        }
    }
}
