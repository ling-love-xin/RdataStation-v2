//! 路径：项目根、资源目录、相对路径解析与可写校验。

use super::*;

impl PayloadStore {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
        }
    }
    /// 项目根。
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }
    /// 受管内容根：`{项目}/resources/`。
    pub fn resources_dir(&self) -> PathBuf {
        self.project_root.join(RESOURCES_DIR_NAME)
    }
    /// 项目级回收站（`.RSmeta/trash/`）：本体删除与还原走它（原型 §4.7）。
    ///
    /// 回收站是**整个项目**的（草稿箱与资产库共用），所以它不认识 `resources/`——
    /// 调用方给绝对路径与 `origin` 标记，还原目标根也由调用方指定。
    pub fn trash(&self) -> engine::persistence::trash::ProjectTrash {
        engine::persistence::trash::ProjectTrash::new(&self.project_root)
    }
    /// 历史内容副本根：`{项目}/.RSmeta/resources/versions/`。
    pub(super) fn versions_dir(&self) -> PathBuf {
        self.project_root
            .join(META_DIR_NAME)
            .join(RESOURCES_DIR_NAME)
            .join(VERSIONS_DIR_NAME)
    }
    /// 建目录（首次归档时调用；幂等）。
    pub async fn ensure_dir(&self) -> Result<(), CoreError> {
        let dir = self.resources_dir();
        fs::create_dir_all(&dir)
            .await
            .map_err(|e| io_err(&dir, "create_resources_dir", e))
    }
    /// 相对路径 → 绝对路径，带**越界守卫**与规范化。
    ///
    /// 拒绝：空路径、绝对路径与根相对路径（"/x"、"C:\\x"、"\\x"）、`..`（穿越）、
    /// 以及任何点前缀组件（`.RSmeta`、`.git`、隐藏文件）——内部态不得经任何 API 触达，
    /// 与草稿箱的 `resolve_path` 同一口径。`./` 视为无操作（不参与结果路径）。
    ///
    /// 不做"去前导分隔符"的宽容处理：把 `/abs/x.sql` 悄悄当成 `resources/abs/x.sql`
    /// 比报错危险得多（调用方给错了绝对路径，却得到一个看似成功的归档）。
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, CoreError> {
        let rel = rel.trim();
        if rel.is_empty() {
            return Err(invalid_rel(rel, "路径为空"));
        }

        let candidate = Path::new(rel);
        if candidate.is_absolute() || candidate.has_root() {
            return Err(invalid_rel(rel, "不允许绝对路径 / 根相对路径"));
        }

        let mut resolved = self.resources_dir();
        for component in candidate.components() {
            match component {
                Component::Normal(name) => {
                    if name.to_string_lossy().starts_with('.') {
                        return Err(invalid_rel(rel, "不允许点前缀（内部态）路径"));
                    }
                    resolved.push(name);
                }
                Component::CurDir => {}
                _ => return Err(invalid_rel(rel, "不允许 .. / 根 / 前缀组件")),
            }
        }

        Ok(resolved)
    }
    /// 写入守卫：落在 `resources/` 下的路径一律拒写（只读三重守卫的第一层，原型 §4.6）。
    pub fn ensure_writable(&self, path: &Path) -> Result<(), CoreError> {
        if path.starts_with(self.resources_dir()) {
            return Err(io_err(
                path,
                "ensure_writable",
                "这是归档本体（只读）；要修改请先「取回（检出）」为草稿工作副本",
            ));
        }
        Ok(())
    }
    /// 目标相对路径是否已被占用（**文件系统层面**：本体在位）。
    ///
    /// 给对话框做冲突预探用：登记表层面的占用由服务层在提交时拒绝（两处各管一层，
    /// 这里不查库——查库是异步的，而它要在开窗前同步拿到）。
    pub fn rel_path_taken(&self, rel: &str) -> bool {
        self.resolve(rel).map(|path| path.exists()).unwrap_or(false)
    }
    /// 找一个不冲突的相对路径：主名加 `-2` / `-3`…（原型 §4.1：目标已存在就改名，不静默覆盖）。
    ///
    /// 命名规则与取回的落点避让同一套（`workbench` 的 `resource_jobs::free_dest`）：先到先得的
    /// 后缀，不把时间戳塞进文件名——用户后来要拿这个名字去说话，得念得出来。
    /// 试到 `-999` 仍冲突就退回原名，让服务层给出明确的失败。
    pub fn free_rel_path(&self, rel: &str) -> String {
        if !self.rel_path_taken(rel) {
            return rel.to_string();
        }
        let path = Path::new(rel);
        let parent = path.parent().filter(|dir| !dir.as_os_str().is_empty());
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("archive");
        let ext = path.extension().and_then(|ext| ext.to_str());
        for index in 2..1000 {
            let name = match ext {
                Some(ext) => format!("{stem}-{index}.{ext}"),
                None => format!("{stem}-{index}"),
            };
            let candidate = match parent {
                Some(dir) => dir.join(name),
                None => PathBuf::from(name),
            };
            // 统一用 `/`：`file_rel_path` 是要入库并展示的字符串，不能随平台变。
            let candidate = candidate.to_string_lossy().replace('\\', "/");
            if !self.rel_path_taken(&candidate) {
                return candidate;
            }
        }
        rel.to_string()
    }
}
