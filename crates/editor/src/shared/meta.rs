//! 编辑器共享状态 —— 连接/模式/内核/文档设置端口（attach_connections/attach_schemas/table_key/document_* 设置）
//!
//! 自 shared.rs 拆分（2026-10-29）：按能力域切分 `impl EditorShared` 方法块。
//! 端口方法全 pub（跨域互调）；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为。

use std::path::PathBuf;
use std::rc::Rc;

use crate::connection::{ConnectionOption, ConnectionsHandle};
use crate::model::DocumentId;

use super::*;

impl EditorShared {

    /// 【M8】文档未绑定连接时执行会落到哪条连接上（口径在执行器：绑定优先 → 回退活动连接）
    pub fn active_connection(&self) -> Option<String> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .and_then(|queue| queue.runner().active_connection())
    }

    /// 是否接了导出路径选择器（未接时导出要明确报原因）
    pub fn has_export_path_picker(&self) -> bool {
        self.export_path.borrow().is_some()
    }

    /// 弹一次导出路径选择（未注入时返回 `None`，调用方必须先看 [`Self::has_export_path_picker`]）
    pub fn pick_export_path(
        &self,
        format: crate::export::ExportFormat,
        default_name: String,
    ) -> Option<PathBuf> {
        let guard = self.export_path.borrow();
        guard
            .as_ref()
            .and_then(|picker| picker(format, default_name))
    }

    /// 注入连接端口（**宿主调用一次**：workbench 接 M3/M4 的连接列表与自动建连）
    pub fn attach_connections(&self, port: ConnectionsHandle) {
        *self.connections.borrow_mut() = Some(port);
    }

    /// 是否接了连接端口（未接时选择器要说“未接入”，而不是看起来有连接可选）
    pub fn has_connections(&self) -> bool {
        self.connections.borrow().is_some()
    }

    /// 当前可选项快照（**渲染路径可调**：实现必须是内存快照，不做 I/O）
    pub fn connection_options(&self) -> Vec<ConnectionOption> {
        let guard = self.connections.borrow();
        guard
            .as_ref()
            .map(|port| port.options())
            .unwrap_or_default()
    }

    /// 【连接菜单】实时在线连接 id 集合（**事件路径专用**：宿主实现会同步查连接管理器，
    /// 导航建连成功不回写快照 `connected`——菜单“只列在线”必须拿真状态；禁止 render 调用）
    pub fn online_ids(&self) -> std::collections::HashSet<String> {
        let guard = self.connections.borrow();
        guard
            .as_ref()
            .map(|port| port.online_ids())
            .unwrap_or_default()
    }

    /// 【T2c-1】注入 schema 端口（**宿主调用一次**：workbench 从导航缓存读库 / schema 列表）
    pub fn attach_schemas(&self, port: crate::connection::SchemasHandle) {
        *self.schemas.borrow_mut() = Some(port);
    }

    /// 【T2c-1】某连接的库 / schema 列表（**渲染路径可调**：端口实现必须是内存快照；
    /// 未接端口 = 空列表 → 状态栏不显示可切的面包屑，如实降级）
    pub fn schemas(&self, conn_id: Option<&str>) -> Vec<crate::connection::SchemaInfo> {
        let guard = self.schemas.borrow();
        match guard.as_ref() {
            Some(port) => conn_id.map(|id| port.schemas(id)).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// 【T2c-1】某文档绑定的 schema（`None` = 未选或文档不存在）
    pub fn document_schema(&self, id: &DocumentId) -> Option<String> {
        self.service().schema_for(id)
    }

    /// 【B7 切片三】注入表结构端口（**宿主调用一次**：workbench 从导航缓存读键列）
    pub fn attach_table_keys(&self, port: crate::export::TableKeysHandle) {
        *self.table_keys.borrow_mut() = Some(port);
    }

    /// 【#20】注入内核注册表端口（**宿主调用一次**：workbench 把
    /// `rds_kernel::detect_kernels()` 的快照注进来；探测在宿主侧后台线程做一次）
    pub fn attach_kernels(&self, port: Rc<dyn crate::kernel::KernelsPort>) {
        *self.kernel_port.borrow_mut() = Some(port);
    }

    /// 【#20】内核注册表快照（渲染路径可调：端口实现是内存读；未接端口 = 空）
    pub fn kernel_specs(&self) -> Vec<rds_kernel::KernelSpec> {
        let guard = self.kernel_port.borrow();
        guard
            .as_ref()
            .map(|port| port.list())
            .unwrap_or_default()
    }

    /// 【#20】执行源「内核」档的实际内核解析（**纯函数**，见 [`crate::kernel::resolve_kernel`]）：
    /// 显式指定 > 会话默认（上次用过的）> 注册表第一个可用；一个都没有给可读原因。
    pub fn resolve_kernel_for(
        &self,
        document: &crate::model::DocumentId,
        source: Option<&crate::cell::ExecutionSource>,
        language: crate::cell::CellLanguage,
    ) -> Result<rds_kernel::KernelSpec, String> {
        let session_default = self
            .notebook_state(document)
            .session
            .default_kernel
            .clone();
        crate::kernel::resolve_kernel(&self.kernel_specs(), source, session_default.as_deref(), language)
    }

    /// 【B7 切片三】某连接下表的主键列（**只在导出菜单弹出时查一次**——事件路径；
    /// 端口实现读导航 L2 缓存，非每帧调用）。未接端口 / 查不到 = `None`
    /// （菜单不摆 `SQL Updates`，如实降级）
    pub fn table_key_columns(&self, conn_id: Option<&str>, table: &str) -> Option<Vec<String>> {
        let guard = self.table_keys.borrow();
        match (guard.as_ref(), conn_id) {
            (Some(port), Some(conn)) => port.primary_key_columns(conn, table),
            _ => None,
        }
    }

    /// 【T2c-3】文档字节数（状态栏「大小」微标；O(1)）
    pub fn document_len(&self, id: &DocumentId) -> usize {
        self.service().document_len(id)
    }

    /// 【T2c-4】文档缩进宽度（状态栏「Sp」微标 + Tab 键缩进的权威值）
    pub fn document_tab_size(&self, id: &DocumentId) -> usize {
        self.service().document_tab_size(id)
    }

    /// 【T2c-4】重设文档缩进宽度；返回是否真变
    pub fn set_document_tab_size(&self, id: &DocumentId, size: usize) -> bool {
        self.update(|svc| svc.set_document_tab_size(id, size))
    }

    /// 【T2c-4】文档缩进是否用制表符（`true` = Tab 缩进，状态栏显「Tab」）
    pub fn document_hard_tabs(&self, id: &DocumentId) -> bool {
        self.service().document_hard_tabs(id)
    }

    /// 【T2c-4】重设缩进用制表符还是空格；返回是否真变
    pub fn set_document_hard_tabs(&self, id: &DocumentId, hard: bool) -> bool {
        self.update(|svc| svc.set_document_hard_tabs(id, hard))
    }

    /// 【2c】文档是否显示空白字符（空格 `•` / 制表符 `→`）
    pub fn document_show_whitespaces(&self, id: &DocumentId) -> bool {
        self.service().document_show_whitespaces(id)
    }

    /// 【2c】重设显示空白字符；返回是否真变
    pub fn set_document_show_whitespaces(&self, id: &DocumentId, show: bool) -> bool {
        self.update(|svc| svc.set_document_show_whitespaces(id, show))
    }

    /// 【2c】文档是否显示隐藏字符标注（¶ 总开关：任一类别显示）
    pub fn document_show_invisible(&self, id: &DocumentId) -> bool {
        self.service().document_show_invisible(id)
    }

    /// 【2c】文档不可见字符的类别位掩码（双栏面板真相源）
    pub fn document_invisible_mask(&self, id: &DocumentId) -> u8 {
        self.service().document_invisible_mask(id)
    }

    /// 【2c】重设不可见字符类别位掩码；返回是否真变
    pub fn set_document_invisible_mask(&self, id: &DocumentId, mask: u8) -> bool {
        self.update(|svc| svc.set_document_invisible_mask(id, mask))
    }

    /// 【2c】重设显示隐藏字符标注（旧 bool 总开关：全开 / 全关）；返回是否真变
    pub fn set_document_show_invisible(&self, id: &DocumentId, show: bool) -> bool {
        self.update(|svc| svc.set_document_show_invisible(id, show))
    }

    /// 【2c】文档当前保存格式（编码 + 换行）
    pub fn document_save_format(&self, id: &DocumentId) -> crate::encoding::SaveFormat {
        self.service().document_save_format(id)
    }

    /// 【2c】重设文档保存格式（编码 / 换行）；返回是否真变
    pub fn set_document_save_format(
        &self,
        id: &DocumentId,
        format: crate::encoding::SaveFormat,
    ) -> bool {
        self.update(|svc| svc.set_document_save_format(id, format))
    }
}
