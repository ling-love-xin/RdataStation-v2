//! 文档集合与生命周期（A1：打开 / 关闭 / 激活 / 重命名 / 脏状态）
//!
//! ## 为什么要有这一层
//!
//! “哪些文档开着、哪个是当前、谁脏了”在 V1 里散落在面板与视图（标签条自己算下标、保存逻辑
//! 自己比对），同一状态两处可写。这里收成**唯一权威**：
//!
//! - `DocumentId` 一经生成**永不变**：另存为只换路径与标题，不换身份。V1 用 `filePath` 派生
//!   面板 id，另存为后新旧 key 失配（标签找不到、结果错位）——“用路径当身份”的代价。
//! - 去重只有一条规则：**同一路径重复打开 = 激活已有文档**（不新开标签）。
//! - 脏状态只有一条判据：`content != baseline`（baseline = 上次加载 / 保存的内容）。
//!
//! ## 边界
//!
//! 本模块**不做 I/O**（读写文件属 `persist`）、不碰 GPUI：入参与出参都是值，因此 A1 的验收
//! 能在单测里跑完（见文件尾）。视图层只从 `documents()` / `active()` 取渲染所需的一切。

use std::path::{Path, PathBuf};

use crate::limits::FileTier;
use crate::model::{Capabilities, DocumentId, EditorMode, OutputTarget, ReadOnly};

// ═══════════════════════════════════════════════════════════════════════
// 文档
// ═══════════════════════════════════════════════════════════════════════

/// 打开中的文档（编辑器侧最小状态：身份 + 内容 + 模式 + 只读 + 档位）
#[derive(Debug, Clone)]
pub struct Document {
    id: DocumentId,
    /// 磁盘路径；未命名文档为 `None`
    path: Option<PathBuf>,
    /// 标签标题（未命名 = `未命名-N`）；另存为 / 重命名后更新，**id 不变**
    title: String,
    content: String,
    /// 上次加载或保存时的内容（脏状态判据）
    baseline: String,
    mode: EditorMode,
    read_only: ReadOnly,
    /// 文件档位（A13）：>50MB 关重能力，>200MB 只读打开 + 提示卡
    tier: FileTier,
    /// 绑定的连接 id（B1）；`None` = 未绑定 → 执行跟随**当前活动连接**（1a 口径）
    connection: Option<String>,
    /// 【B13】执行通道（文档级属性，与连接绑定同类；随会话持久化）
    channel: crate::channel::ExecChannel,
    /// 【T2c-1】绑定的 schema（文档级属性，与连接绑定同类；`None` = 未选 → 执行用连接默认）
    schema: Option<String>,
    /// 【2b】选中的执行环境 id（通用模式的脚本执行；`None` = 未选 → 运行不亮）
    env: Option<String>,
    /// 【T2c-4】缩进宽度（文档级；状态栏「Sp」微标 + Tab 键缩进都走它；随会话持久化）
    tab_size: usize,
    /// 【T2c-4】缩进用制表符（`true` = Tab 缩进，状态栏显「Tab」；`false` = 空格 × tab_size）
    hard_tabs: bool,
    /// 【2c】显示空白字符（空格 `•` / 制表符 `→`；视图偏好，不随会话持久化）
    show_whitespaces: bool,
    /// 【2c】显示隐藏字符标注的**类别位掩码**（bit0-3 = 零宽/特殊空格/双向控制/控制；
    /// 视图偏好，不随会话持久化）
    invisible_mask: u8,
    /// 【2c】当前保存格式（编码 + 换行；文档级权威值）
    format: crate::encoding::SaveFormat,
    /// 【2c】上次加载 / 保存时的保存格式（格式切换也是未保存的改动）
    saved_format: crate::encoding::SaveFormat,
}

impl Document {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: DocumentId,
        path: Option<PathBuf>,
        title: String,
        content: String,
        mode: EditorMode,
        read_only: ReadOnly,
        tier: FileTier,
        connection: Option<String>,
        channel: crate::channel::ExecChannel,
        schema: Option<String>,
        env: Option<String>,
        format: crate::encoding::SaveFormat,
    ) -> Self {
        Self {
            id,
            path,
            title,
            baseline: content.clone(),
            content,
            mode,
            read_only,
            tier,
            connection,
            channel,
            schema,
            env,
            tab_size: 2,
            hard_tabs: false,
            show_whitespaces: false,
            invisible_mask: 0,
            saved_format: format,
            format,
        }
    }

    /// 稳定身份（永不变）
    pub fn id(&self) -> &DocumentId {
        &self.id
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    pub fn read_only(&self) -> ReadOnly {
        self.read_only
    }

    /// 文件档位（>50MB / >200MB 的策略见 `limits` 模块）
    pub fn tier(&self) -> FileTier {
        self.tier
    }

    /// 绑定的连接 id（B1）；`None` = 未绑定，执行跟随当前活动连接
    pub fn connection(&self) -> Option<&str> {
        self.connection.as_deref()
    }

    /// 【B13】执行通道（文档级属性；默认源库直连）
    pub fn channel(&self) -> crate::channel::ExecChannel {
        self.channel
    }

    /// 【T2c-1】绑定的 schema（`None` = 未选 → 执行用连接默认 schema）
    pub fn schema(&self) -> Option<&str> {
        self.schema.as_deref()
    }

    /// 【2b】选中的执行环境 id（通用模式脚本执行；`None` = 未选）
    pub fn env(&self) -> Option<&str> {
        self.env.as_deref()
    }

    /// 【T2c-4】文档缩进宽度（状态栏「Sp」微标 + Tab 键缩进的权威值）
    pub fn tab_size(&self) -> usize {
        self.tab_size
    }

    /// 【T2c-4】缩进用制表符还是空格（`true` = Tab 缩进）
    pub fn hard_tabs(&self) -> bool {
        self.hard_tabs
    }

    /// 【2c】是否显示空白字符（空格 `•` / 制表符 `→`）
    pub fn show_whitespaces(&self) -> bool {
        self.show_whitespaces
    }

    /// 【2c】不可见字符的类别位掩码（0 = 全不显示；bit0-3 = 零宽/特殊空格/双向/控制）
    pub fn invisible_mask(&self) -> u8 {
        self.invisible_mask
    }

    /// 【2c】是否有任一类别显示（旧 bool 总开关语义；双栏面板的 ¶ 按钮 toggled 用）
    pub fn show_invisible(&self) -> bool {
        self.invisible_mask != 0
    }

    /// 【2c】当前保存格式（编码 + 换行）
    pub fn save_format(&self) -> crate::encoding::SaveFormat {
        self.format
    }

    /// 提示卡文案（档位带来的操作限制；`None` = 不显示）
    pub fn tier_notice(&self) -> Option<&'static str> {
        self.tier.notice()
    }

    /// 该模式的能力集（chrome 与服务的唯一分叉点）
    pub fn capabilities(&self) -> Capabilities {
        Capabilities::for_mode(self.mode)
    }

    pub fn output_target(&self) -> OutputTarget {
        self.capabilities().output
    }

    /// 是否与基线不同（脏判据：内容变了，或保存**编码**换了——编码改变保存产出不同字节）。
    ///
    /// 换行符（eol）不参与脏判据：eol 切换是显示/保存契约层面的设置，不产生内容差异，
    /// 不把文档标「未保存」（用户要求：切换 eol 不影响文本、不置脏）。
    pub fn is_dirty(&self) -> bool {
        self.content != self.baseline || self.format.encoding != self.saved_format.encoding
    }

    /// 供视图做差异比较的基线长度等统计（不暴露基线本身，避免外部改写）
    pub fn baseline_len(&self) -> usize {
        self.baseline.len()
    }

    fn set_content(&mut self, content: String) {
        self.content = content;
    }

    /// 把当前内容记为已保存（保存成功后调用；格式同时前移）
    fn mark_saved(&mut self) {
        self.baseline = self.content.clone();
        self.saved_format = self.format;
    }

    fn set_mode(&mut self, mode: EditorMode) {
        self.mode = mode;
    }

    fn set_read_only(&mut self, read_only: ReadOnly) {
        self.read_only = read_only;
    }

    /// 另存为 / 重命名：只换路径与标题
    fn rename_to(&mut self, path: PathBuf) {
        self.title = title_for_path(&path);
        self.path = Some(path);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 打开请求与结果
// ═══════════════════════════════════════════════════════════════════════

/// 打开请求（参数较多且多为可选，收成结构体避免长参数表）
#[derive(Debug, Clone)]
pub struct OpenRequest {
    pub path: Option<PathBuf>,
    pub content: String,
    pub mode: EditorMode,
    pub read_only: ReadOnly,
    /// 文件档位（默认常规；`persist::open_file` 会按实际大小给）
    pub tier: FileTier,
    /// 绑定连接（B1）；`None` = 未绑定，执行跟随当前活动连接
    pub connection: Option<String>,
    /// 【B13】执行通道（默认源库直连）
    pub channel: crate::channel::ExecChannel,
    /// 【T2c-1】绑定 schema（默认未选 → 执行用连接默认 schema）
    pub schema: Option<String>,
    /// 【2b】选中执行环境 id（通用模式脚本执行；默认未选 → 运行不亮）
    pub env: Option<String>,
    /// 【2c】保存格式（默认 UTF-8 + LF；`persist::open_file` 按检测结果给）
    pub format: crate::encoding::SaveFormat,
}

impl OpenRequest {
    /// 打开磁盘文件
    pub fn file(path: impl Into<PathBuf>, content: impl Into<String>, mode: EditorMode) -> Self {
        Self {
            path: Some(path.into()),
            content: content.into(),
            mode,
            read_only: ReadOnly::none(),
            tier: FileTier::Normal,
            connection: None,
            channel: crate::channel::ExecChannel::default(),
            schema: None,
            env: None,
            format: crate::encoding::SaveFormat::default(),
        }
    }

    /// 新建未命名文档
    pub fn untitled(content: impl Into<String>, mode: EditorMode) -> Self {
        Self {
            path: None,
            content: content.into(),
            mode,
            read_only: ReadOnly::none(),
            tier: FileTier::Normal,
            connection: None,
            channel: crate::channel::ExecChannel::default(),
            schema: None,
            env: None,
            format: crate::encoding::SaveFormat::default(),
        }
    }

    /// 绑定连接（B1；新建查询 / 导航“在 SQL 编辑器中打开”时给）
    pub fn with_connection(mut self, conn_id: impl Into<String>) -> Self {
        self.connection = Some(conn_id.into());
        self
    }

    pub fn with_read_only(mut self, read_only: ReadOnly) -> Self {
        self.read_only = read_only;
        self
    }

    /// 【2c】带上保存格式（`persist::open_file` 按磁盘检测结果给）
    pub fn with_format(mut self, format: crate::encoding::SaveFormat) -> Self {
        self.format = format;
        self
    }

    /// 【B13】带上执行通道（导航 / 会话恢复用）
    pub fn with_channel(mut self, channel: crate::channel::ExecChannel) -> Self {
        self.channel = channel;
        self
    }

    /// 带上文件档位（`persist::open_file` 用；超大文件同时置编辑器只读）
    pub fn with_tier(mut self, tier: FileTier) -> Self {
        self.tier = tier;
        if tier.opens_read_only() {
            self.read_only = ReadOnly {
                editor: true,
                connection: self.read_only.connection,
            };
        }
        self
    }
}

/// 打开结果：新开标签，还是激活已有标签（去重）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenOutcome {
    Opened(DocumentId),
    Activated(DocumentId),
}

impl OpenOutcome {
    pub fn id(&self) -> &DocumentId {
        match self {
            Self::Opened(id) | Self::Activated(id) => id,
        }
    }

    /// 是否只是激活了已有文档（调用方据此跳过重新读盘）
    pub fn is_activated(&self) -> bool {
        matches!(self, Self::Activated(_))
    }
}

/// 重命名 / 另存为的失败原因
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameError {
    /// 目标路径已在另一个标签里打开
    Duplicate(PathBuf),
    /// 文档不存在（已关闭）
    NoSuchDocument(DocumentId),
}

// ═══════════════════════════════════════════════════════════════════════
// 服务
// ═══════════════════════════════════════════════════════════════════════

/// 文档集合的唯一权威
#[derive(Debug, Default)]
pub struct EditorService {
    /// 保持**打开顺序**（标签顺序即此顺序，不参与排序语义）
    documents: Vec<Document>,
    active: Option<DocumentId>,
    /// 未命名文档编号（只增不减：避免重名，也避免 id 复用）
    serial: u64,
}

impl EditorService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    pub fn active_id(&self) -> Option<&DocumentId> {
        self.active.as_ref()
    }

    pub fn active(&self) -> Option<&Document> {
        let id = self.active.as_ref()?;
        self.find(id)
    }

    pub fn find(&self, id: &DocumentId) -> Option<&Document> {
        self.documents.iter().find(|doc| doc.id() == id)
    }

    /// 某路径是否已打开（去重判据；不做 I/O）
    pub fn find_by_path(&self, path: &Path) -> Option<&DocumentId> {
        let key = path_key(path);
        self.documents
            .iter()
            .find(|doc| doc.path.as_deref().is_some_and(|p| path_key(p) == key))
            .map(Document::id)
    }

    /// 已脏文档（关闭全部 / 退出确认用）
    pub fn dirty_ids(&self) -> Vec<DocumentId> {
        self.documents
            .iter()
            .filter(|doc| doc.is_dirty())
            .map(|doc| doc.id().clone())
            .collect()
    }

    /// 打开文档：同路径已打开则**激活**，否则新开并激活
    pub fn open(&mut self, request: OpenRequest) -> OpenOutcome {
        if let Some(path) = request.path.as_deref()
            && let Some(existing) = self.find_by_path(path)
        {
            let id = existing.clone();
            self.active = Some(id.clone());
            return OpenOutcome::Activated(id);
        }

        self.serial += 1;
        let id = DocumentId::new(format!("doc-{}", self.serial));
        let title = match request.path.as_deref() {
            Some(path) => title_for_path(path),
            None => format!("未命名-{}", self.serial),
        };

        self.documents.push(Document::new(
            id.clone(),
            request.path,
            title,
            request.content,
            request.mode,
            request.read_only,
            request.tier,
            request.connection,
            request.channel,
            request.schema,
            request.env,
            request.format,
        ));
        self.active = Some(id.clone());

        OpenOutcome::Opened(id)
    }

    /// 关闭文档；返回被关闭的文档（调用方可能需要按脏状态先落草稿）
    ///
    /// 关闭的是当前文档时，激活**同位置的邻居**（没有则最后一个）——与主流编辑器的
    /// “关掉当前标签后落到旁边”一致，避免焦点跳回第一个。
    pub fn close(&mut self, id: &DocumentId) -> Option<Document> {
        let index = self.documents.iter().position(|doc| doc.id() == id)?;
        let removed = self.documents.remove(index);

        if self.active.as_ref() == Some(id) {
            self.active = self
                .documents
                .get(index)
                .or_else(|| self.documents.last())
                .map(|doc| doc.id().clone());
        }

        Some(removed)
    }

    /// 激活某文档（不存在则不改动）
    pub fn activate(&mut self, id: &DocumentId) -> bool {
        if self.find(id).is_some() {
            self.active = Some(id.clone());
            true
        } else {
            false
        }
    }

    /// 重命名 / 另存为：**身份不变**，只换路径与标题
    pub fn rename(
        &mut self,
        id: &DocumentId,
        new_path: impl Into<PathBuf>,
    ) -> Result<(), RenameError> {
        let new_path = new_path.into();
        let key = path_key(&new_path);

        let clash = self
            .documents
            .iter()
            .any(|doc| doc.id() != id && doc.path.as_deref().is_some_and(|p| path_key(p) == key));
        if clash {
            return Err(RenameError::Duplicate(new_path));
        }

        let doc = self
            .documents
            .iter_mut()
            .find(|doc| doc.id() == id)
            .ok_or_else(|| RenameError::NoSuchDocument(id.clone()))?;
        doc.rename_to(new_path);
        Ok(())
    }

    /// 输入内容（编辑器只读时拒绝写入，返回是否被接受）
    pub fn set_content(&mut self, id: &DocumentId, content: String) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) if doc.read_only.can_edit() => {
                doc.set_content(content);
                true
            }
            _ => false,
        }
    }

    /// 保存成功：把当前内容记为基线（清脏）
    pub fn mark_saved(&mut self, id: &DocumentId) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) => {
                doc.mark_saved();
                true
            }
            None => false,
        }
    }

    /// 切换模式（合法性由 `mode` 模块的判定规则决定，调用方先判定再切）
    pub fn set_mode(&mut self, id: &DocumentId, mode: EditorMode) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) => {
                doc.set_mode(mode);
                true
            }
            None => false,
        }
    }

    /// 绑定 / 解绑连接（B1；`None` = 未绑定，执行跟随当前活动连接）
    ///
    /// 合法性由调用方保证（连接得先在宿主的列表里、且已建连）——服务层不做 I/O。
    pub fn set_connection(&mut self, id: &DocumentId, connection: Option<String>) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) => {
                doc.connection = connection;
                true
            }
            None => false,
        }
    }

    /// 【B13】切换某文档的执行通道（返回是否真的变了）
    pub fn set_channel(&mut self, id: &DocumentId, channel: crate::channel::ExecChannel) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) => {
                let changed = doc.channel != channel;
                doc.channel = channel;
                changed
            }
            None => false,
        }
    }

    /// 【B13】某文档当前的执行通道（文档不存在时给默认档）
    pub fn channel_for(&self, id: &DocumentId) -> crate::channel::ExecChannel {
        self.documents
            .iter()
            .find(|doc| doc.id() == id)
            .map(|doc| doc.channel)
            .unwrap_or_default()
    }

    /// 【T2c-1】绑定 / 解绑某文档的 schema（`None` = 未选 → 执行用连接默认 schema）
    pub fn set_schema(&mut self, id: &DocumentId, schema: Option<String>) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) => {
                let changed = doc.schema != schema;
                doc.schema = schema;
                changed
            }
            None => false,
        }
    }

    /// 【T2c-1】某文档绑定的 schema（执行时用；`None` = 未选或文档不存在）
    pub fn schema_for(&self, id: &DocumentId) -> Option<String> {
        self.documents
            .iter()
            .find(|doc| doc.id() == id)
            .and_then(|doc| doc.schema().map(str::to_string))
    }

    /// 【2b】绑定 / 解绑某文档的执行环境（`None` = 未选 → 运行不亮）
    pub fn set_env(&mut self, id: &DocumentId, env: Option<String>) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) => {
                let changed = doc.env != env;
                doc.env = env;
                changed
            }
            None => false,
        }
    }

    /// 【2b】某文档选中的执行环境（脚本执行用；`None` = 未选或文档不存在）
    pub fn env_for(&self, id: &DocumentId) -> Option<String> {
        self.documents
            .iter()
            .find(|doc| doc.id() == id)
            .and_then(|doc| doc.env().map(str::to_string))
    }

    /// 【T2c-4】文档缩进宽度（状态栏「Sp」微标 + Tab 键缩进；默认 2）
    pub fn document_tab_size(&self, id: &DocumentId) -> usize {
        self.documents
            .iter()
            .find(|doc| doc.id() == id)
            .map(|doc| doc.tab_size())
            .unwrap_or(2)
    }

    /// 【T2c-4】文档缩进是否用制表符（`true` = Tab 缩进，状态栏显「Tab」）
    pub fn document_hard_tabs(&self, id: &DocumentId) -> bool {
        self.documents
            .iter()
            .find(|doc| doc.id() == id)
            .map(|doc| doc.hard_tabs())
            .unwrap_or(false)
    }

    /// 【T2c-4】重设文档缩进宽度；返回是否真变（重复设置同值 = 未变）
    pub fn set_document_tab_size(&mut self, id: &DocumentId, size: usize) -> bool {
        let Some(doc) = self.documents.iter_mut().find(|doc| doc.id() == id) else {
            return false;
        };
        if doc.tab_size == size {
            return false;
        }
        doc.tab_size = size;
        true
    }

    /// 【T2c-4】重设缩进用制表符还是空格；返回是否真变
    pub fn set_document_hard_tabs(&mut self, id: &DocumentId, hard: bool) -> bool {
        let Some(doc) = self.documents.iter_mut().find(|doc| doc.id() == id) else {
            return false;
        };
        if doc.hard_tabs == hard {
            return false;
        }
        doc.hard_tabs = hard;
        true
    }

    /// 【2c】文档是否显示空白字符（空格 `•` / 制表符 `→`）
    pub fn document_show_whitespaces(&self, id: &DocumentId) -> bool {
        self.documents
            .iter()
            .find(|doc| doc.id() == id)
            .map(|doc| doc.show_whitespaces())
            .unwrap_or(false)
    }

    /// 【2c】重设显示空白字符；返回是否真变
    pub fn set_document_show_whitespaces(&mut self, id: &DocumentId, show: bool) -> bool {
        let Some(doc) = self.documents.iter_mut().find(|doc| doc.id() == id) else {
            return false;
        };
        if doc.show_whitespaces == show {
            return false;
        }
        doc.show_whitespaces = show;
        true
    }

    /// 【2c】文档是否有任一不可见字符类别显示（¶ 总开关语义）
    pub fn document_show_invisible(&self, id: &DocumentId) -> bool {
        self.documents
            .iter()
            .find(|doc| doc.id() == id)
            .map(|doc| doc.show_invisible())
            .unwrap_or(false)
    }

    /// 【2c】文档不可见字符的类别位掩码（双栏面板的真相源）
    pub fn document_invisible_mask(&self, id: &DocumentId) -> u8 {
        self.documents
            .iter()
            .find(|doc| doc.id() == id)
            .map(|doc| doc.invisible_mask())
            .unwrap_or(0)
    }

    /// 【2c】重设不可见字符类别位掩码；返回是否真变
    pub fn set_document_invisible_mask(&mut self, id: &DocumentId, mask: u8) -> bool {
        let Some(doc) = self.documents.iter_mut().find(|doc| doc.id() == id) else {
            return false;
        };
        if doc.invisible_mask == mask {
            return false;
        }
        doc.invisible_mask = mask;
        true
    }

    /// 【2c】重设显示隐藏字符标注（旧 bool 总开关：全开 / 全关）；返回是否真变
    pub fn set_document_show_invisible(&mut self, id: &DocumentId, show: bool) -> bool {
        self.set_document_invisible_mask(id, if show { crate::hidden::FULL_MASK } else { 0 })
    }

    /// 【2c】文档当前保存格式（编码 + 换行）
    pub fn document_save_format(&self, id: &DocumentId) -> crate::encoding::SaveFormat {
        self.documents
            .iter()
            .find(|doc| doc.id() == id)
            .map(|doc| doc.save_format())
            .unwrap_or_default()
    }

    /// 【2c】重设文档保存格式（编码 / 换行）；返回是否真变（同值 = 未变）
    pub fn set_document_save_format(
        &mut self,
        id: &DocumentId,
        format: crate::encoding::SaveFormat,
    ) -> bool {
        let Some(doc) = self.documents.iter_mut().find(|doc| doc.id() == id) else {
            return false;
        };
        if doc.format == format {
            return false;
        }
        doc.format = format;
        true
    }

    /// 【T2c-3】文档字节数（状态栏「大小」微标；O(1)，渲染路径可调）
    pub fn document_len(&self, id: &DocumentId) -> usize {
        self.documents
            .iter()
            .find(|doc| doc.id() == id)
            .map(|doc| doc.content.len())
            .unwrap_or(0)
    }

    /// 某文档绑定的连接（执行时用；`None` = 未绑定或文档不存在）
    pub fn connection_for(&self, id: &DocumentId) -> Option<String> {
        self.documents
            .iter()
            .find(|doc| doc.id() == id)
            .and_then(|doc| doc.connection().map(str::to_string))
    }

    /// 更新只读两维度（连接只读随连接策略变化）
    pub fn set_read_only(&mut self, id: &DocumentId, read_only: ReadOnly) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) => {
                doc.set_read_only(read_only);
                true
            }
            None => false,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 纯函数辅助
// ═══════════════════════════════════════════════════════════════════════

/// 路径去重键
///
/// Windows 的文件系统不区分大小写（`D:\a.sql` 与 `d:\A.SQL` 是同一个文件），Linux 区分。
/// 因此按平台归一：Windows 转小写，其它平台原样；分隔符统一为 `/` 便于比较。
/// **不做 canonicalize**（那是 I/O，且文件可能尚未存在）：调用方应传规范化后的绝对路径。
pub(crate) fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// 标签标题：取文件名；取不到则退回完整路径
fn title_for_path(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

// ═══════════════════════════════════════════════════════════════════════
// 测试（A1 验收 + A8 的纯逻辑部分）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn opened_file(path: &str, content: &str) -> (EditorService, DocumentId) {
        let mut service = EditorService::new();
        let outcome = service.open(OpenRequest::file(path, content, EditorMode::Sql));
        let id = outcome.id().clone();
        assert!(!outcome.is_activated());
        (service, id)
    }

    #[test]
    fn open_activates_and_starts_clean() {
        let (service, id) = opened_file(r"D:\sql\report.sql", "select 1");
        assert_eq!(service.len(), 1);
        assert_eq!(service.active_id(), Some(&id));
        let doc = service.active().expect("active document");
        assert_eq!(doc.title(), "report.sql");
        assert!(!doc.is_dirty(), "刚打开不应是脏的");
        assert_eq!(doc.mode(), EditorMode::Sql);
        assert_eq!(doc.output_target(), OutputTarget::DockPanel);
    }

    #[test]
    fn same_path_opens_once_and_just_activates() {
        let (mut service, first) = opened_file(r"D:\sql\report.sql", "select 1");
        service.open(OpenRequest::file(
            r"D:\sql\other.sql",
            "select 2",
            EditorMode::Sql,
        ));
        assert_eq!(service.len(), 2);

        let again = service.open(OpenRequest::file(
            r"D:\sql\report.sql",
            "被忽略的内容",
            EditorMode::Sql,
        ));
        assert!(again.is_activated(), "同路径重复打开只应激活");
        assert_eq!(again.id(), &first);
        assert_eq!(service.len(), 2, "不得新开标签");
        assert_eq!(service.active_id(), Some(&first));
        // 已有文档内容不被后来者覆盖
        assert_eq!(service.find(&first).expect("doc").content(), "select 1");
    }

    #[test]
    fn path_dedup_ignores_case_and_separator_style() {
        let (mut service, first) = opened_file(r"D:\sql\report.sql", "select 1");
        let same = service.open(OpenRequest::file("d:/SQL/REPORT.SQL", "x", EditorMode::Sql));
        if cfg!(windows) {
            assert!(
                same.is_activated(),
                "Windows 下大小写/分隔符不同仍是同一文件"
            );
            assert_eq!(same.id(), &first);
            assert_eq!(service.len(), 1);
        } else {
            // 非 Windows：区分大小写，属另一个文件（记录平台差异，不假装一致）
            assert!(!same.is_activated());
            assert_eq!(service.len(), 2);
        }
    }

    #[test]
    fn untitled_documents_get_distinct_identity_and_title() {
        let mut service = EditorService::new();
        let a = service
            .open(OpenRequest::untitled("", EditorMode::General))
            .id()
            .clone();
        let b = service
            .open(OpenRequest::untitled("", EditorMode::General))
            .id()
            .clone();

        assert_ne!(a, b);
        assert_eq!(service.find(&a).expect("a").title(), "未命名-1");
        assert_eq!(service.find(&b).expect("b").title(), "未命名-2");
        assert_eq!(service.len(), 2);
        assert!(service.find(&a).expect("a").path().is_none());
    }

    #[test]
    fn closing_active_activates_neighbour() {
        let mut service = EditorService::new();
        let a = service
            .open(OpenRequest::file(r"D:\a.sql", "a", EditorMode::Sql))
            .id()
            .clone();
        let b = service
            .open(OpenRequest::file(r"D:\b.sql", "b", EditorMode::Sql))
            .id()
            .clone();
        let c = service
            .open(OpenRequest::file(r"D:\c.sql", "c", EditorMode::Sql))
            .id()
            .clone();
        assert_eq!(service.active_id(), Some(&c));

        // 关掉中间的 b（不是当前）→ 当前不变
        service.close(&b).expect("closed");
        assert_eq!(service.active_id(), Some(&c));

        // 关掉当前的 c → 落到同位置的邻居（没有则最后一个）
        service.close(&c).expect("closed");
        assert_eq!(service.active_id(), Some(&a));

        // 关掉最后一个 → 无文档时 active 清空
        service.close(&a).expect("closed");
        assert!(service.is_empty());
        assert!(service.active_id().is_none());
        assert!(service.active().is_none());
    }

    #[test]
    fn closed_document_disappears_from_lookups() {
        let (mut service, id) = opened_file(r"D:\a.sql", "a");
        service.close(&id).expect("closed");
        assert!(service.find(&id).is_none());
        assert!(service.find_by_path(Path::new(r"D:\a.sql")).is_none());
        assert!(!service.activate(&id), "已关闭的文档不能被激活");
        assert!(!service.set_content(&id, "x".to_string()));
    }

    #[test]
    fn rename_keeps_identity_and_updates_path_and_title() {
        let (mut service, id) = opened_file(r"D:\sql\report.sql", "select 1");
        service
            .rename(&id, r"D:\sql\report_v2.sql")
            .expect("rename");

        let doc = service.find(&id).expect("same id still there");
        assert_eq!(doc.id(), &id, "身份不变（V1 的教训：不能用路径当身份）");
        assert_eq!(doc.title(), "report_v2.sql");
        assert_eq!(doc.path(), Some(Path::new(r"D:\sql\report_v2.sql")));
        // 旧路径不再命中，新路径命中
        assert!(
            service
                .find_by_path(Path::new(r"D:\sql\report.sql"))
                .is_none()
        );
        assert_eq!(
            service.find_by_path(Path::new(r"D:\sql\report_v2.sql")),
            Some(&id)
        );
    }

    #[test]
    fn rename_to_an_open_path_is_rejected() {
        let (mut service, a) = opened_file(r"D:\a.sql", "a");
        let b = service
            .open(OpenRequest::file(r"D:\b.sql", "b", EditorMode::Sql))
            .id()
            .clone();

        let err = service.rename(&a, r"D:\b.sql").expect_err("应拒绝");
        assert_eq!(err, RenameError::Duplicate(PathBuf::from(r"D:\b.sql")));
        // 冲突不产生任何半成品
        assert_eq!(
            service.find(&a).expect("a").path(),
            Some(Path::new(r"D:\a.sql"))
        );
        assert_eq!(
            service.find(&b).expect("b").path(),
            Some(Path::new(r"D:\b.sql"))
        );
    }

    #[test]
    fn editing_marks_dirty_and_reverting_clears_it() {
        let (mut service, id) = opened_file(r"D:\a.sql", "select 1");
        assert!(!service.find(&id).expect("doc").is_dirty());

        assert!(service.set_content(&id, "select 1, 2".to_string()));
        assert!(service.find(&id).expect("doc").is_dirty(), "改过即脏");
        assert_eq!(service.dirty_ids(), vec![id.clone()]);

        // 撤销回原值 → 干净（脏判据只看内容，不看编辑历史）
        assert!(service.set_content(&id, "select 1".to_string()));
        assert!(!service.find(&id).expect("doc").is_dirty());
        assert!(service.dirty_ids().is_empty());
    }

    #[test]
    fn mark_saved_moves_the_baseline() {
        let (mut service, id) = opened_file(r"D:\a.sql", "select 1");
        service.set_content(&id, "select 1, 2".to_string());
        assert!(service.find(&id).expect("doc").is_dirty());

        assert!(service.mark_saved(&id));
        let doc = service.find(&id).expect("doc");
        assert!(!doc.is_dirty(), "保存后应干净");
        assert_eq!(doc.baseline_len(), "select 1, 2".len());

        // 保存之后再改 → 仍能判脏（基线已前移）
        service.set_content(&id, "select 1, 2, 3".to_string());
        assert!(service.find(&id).expect("doc").is_dirty());
    }

    #[test]
    fn read_only_editor_rejects_content_changes() {
        let mut service = EditorService::new();
        let id = service
            .open(
                OpenRequest::file(r"D:\a.sql", "select 1", EditorMode::Sql)
                    .with_read_only(ReadOnly::editor_only()),
            )
            .id()
            .clone();

        assert!(
            !service.set_content(&id, "select 2".to_string()),
            "只读不可写入"
        );
        assert_eq!(service.find(&id).expect("doc").content(), "select 1");
        assert!(!service.find(&id).expect("doc").is_dirty());
        assert!(!service.find(&id).expect("doc").read_only().can_edit());
    }

    #[test]
    fn switching_mode_updates_capabilities() {
        // 模式是**文档属性**（架构 D2）：切换后能力表派生的 chrome / 输出目标随之变化
        let (mut service, id) = opened_file(r"D:\a.sql", "select 1");
        assert_eq!(
            service.find(&id).expect("doc").output_target(),
            OutputTarget::DockPanel
        );

        assert!(service.set_mode(&id, EditorMode::General));
        let text = service.find(&id).expect("doc");
        assert_eq!(text.mode(), EditorMode::General);
        assert_eq!(text.output_target(), OutputTarget::None);
        assert!(
            !text.capabilities().talks_to_database(),
            "文本模式禁止一切数据库通信"
        );

        assert!(service.set_mode(&id, EditorMode::Analysis));
        assert_eq!(
            service.find(&id).expect("doc").output_target(),
            OutputTarget::Inline
        );
    }

    #[test]
    fn titles_fall_back_to_full_path_when_no_file_name() {
        assert_eq!(
            title_for_path(Path::new(r"D:\sql\report.sql")),
            "report.sql"
        );
        // 取不到文件名时不能返回空标题
        assert_eq!(title_for_path(Path::new(r"D:\")), r"D:\");
    }

    /// 【T2c-4】缩进重设：宽度与制表符各自独立存储，重复设置同值 = 未变
    #[test]
    fn indent_reset_tracks_width_and_hard_tabs_separately() {
        let (mut service, id) = opened_file(r"D:\a.sql", "select 1");

        // 默认：空格 × 2，非制表符
        assert_eq!(service.document_tab_size(&id), 2);
        assert!(!service.document_hard_tabs(&id));

        // 改宽度：真变 + 重复设置同值不变
        assert!(service.set_document_tab_size(&id, 4), "改宽要报真变");
        assert_eq!(service.document_tab_size(&id), 4);
        assert!(!service.set_document_tab_size(&id, 4), "同值重设 = 未变");
        assert!(!service.document_hard_tabs(&id), "改宽度不影响制表符开关");

        // 切制表符：真变 + 宽度保留（切回空格时用）
        assert!(service.set_document_hard_tabs(&id, true));
        assert!(service.document_hard_tabs(&id));
        assert_eq!(service.document_tab_size(&id), 4, "切制表符保留空格宽度");
        assert!(!service.set_document_hard_tabs(&id, true), "同值重设 = 未变");
        assert!(service.set_document_hard_tabs(&id, false));
        assert!(!service.document_hard_tabs(&id));

        // 不存在的文档：读取回退默认、设置返回 false
        let ghost = DocumentId::new("ghost-doc");
        assert_eq!(service.document_tab_size(&ghost), 2);
        assert!(!service.document_hard_tabs(&ghost));
        assert!(!service.set_document_tab_size(&ghost, 8));
    }

    /// 【2c】保存格式（编码 + 换行）：默认 UTF-8 + LF；切换标脏、保存后干净、切回原格式不脏
    #[test]
    fn save_format_switch_marks_dirty_and_mark_saved_clears_it() {
        let (mut service, id) = opened_file(r"D:\a.sql", "select 1");

        // 默认 UTF-8 + LF，不脏
        let default = crate::encoding::SaveFormat::default();
        assert_eq!(service.document_save_format(&id), default);
        assert!(!service.find(&id).expect("doc").is_dirty());

        // 切编码（UTF-8 → GBK）：真变 + 标脏（保存会产出不同字节）
        let gbk = crate::encoding::SaveFormat {
            encoding: crate::encoding::TextEncoding::Gbk,
            ..default
        };
        assert!(service.set_document_save_format(&id, gbk));
        assert_eq!(service.document_save_format(&id), gbk);
        assert!(service.find(&id).expect("doc").is_dirty(), "切编码 = 未保存的改动");
        assert!(!service.set_document_save_format(&id, gbk), "同值重设 = 未变");

        // 保存：格式前移，干净
        service.mark_saved(&id);
        assert!(!service.find(&id).expect("doc").is_dirty());

        // 再切回默认：又脏，直到保存
        assert!(service.set_document_save_format(&id, default));
        assert!(service.find(&id).expect("doc").is_dirty());
        service.mark_saved(&id);
        assert!(!service.find(&id).expect("doc").is_dirty());

        // 不存在的文档：读回退默认、设置 false
        let ghost = DocumentId::new("ghost-doc");
        assert_eq!(service.document_save_format(&ghost), default);
        assert!(!service.set_document_save_format(&ghost, gbk));
    }

    /// 【2c】显示空白 / 隐藏字符：独立开关、重复设置同值 = 未变、不影响脏状态
    #[test]
    fn show_flags_are_independent_and_do_not_mark_dirty() {
        let (mut service, id) = opened_file(r"D:\a.sql", "select 1");

        assert!(!service.document_show_whitespaces(&id));
        assert!(!service.document_show_invisible(&id));

        assert!(service.set_document_show_whitespaces(&id, true));
        assert!(service.document_show_whitespaces(&id));
        assert!(!service.document_show_whitespaces(&id) == false);
        assert!(!service.set_document_show_whitespaces(&id, true), "同值重设 = 未变");
        // 视图偏好不影响脏状态
        assert!(!service.find(&id).expect("doc").is_dirty());

        assert!(service.set_document_show_invisible(&id, true));
        assert!(service.document_show_invisible(&id));
        assert!(!service.document_show_whitespaces(&id) == false, "两个开关独立");
        assert!(!service.set_document_show_invisible(&id, true), "同值重设 = 未变");

        let ghost = DocumentId::new("ghost-doc");
        assert!(!service.document_show_whitespaces(&ghost));
        assert!(!service.document_show_invisible(&ghost));
        assert!(!service.set_document_show_whitespaces(&ghost, true));
    }
}
