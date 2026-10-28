//! 编辑器共享状态 —— 环境/补全/LSP/通道/来源/项目端口（attach_env/attach_completion/attach_lsp/attach_channels/attach_sources/attach_project/primary_key/column_annotations）
//!
//! 自 shared.rs 拆分（2026-10-29）：按能力域切分 `impl EditorShared` 方法块。
//! 端口方法全 pub（跨域互调）；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为。

use std::sync::Arc;

use crate::channel::{ChannelAvailabilitySet, ChannelsHandle, ExecChannel};
use crate::completion::{Catalog, ColumnAnnotation, CompletionHandle};
use crate::connection::{chip_for, status_text};
use crate::model::DocumentId;
use crate::project::{ProjectHandle, ProjectState};
use crate::sources::SourcesHandle;

use super::*;

impl EditorShared {

    /// 确保某个连接已建连（事件路径调用：可能真的去建连）
    pub fn ensure_connected(&self, conn_id: &str) -> Result<(), String> {
        let guard = self.connections.borrow();
        match guard.as_ref() {
            Some(port) => port.ensure_connected(conn_id),
            None => Err("当前未接入连接列表".to_string()),
        }
    }

    /// 某文档的绑定在状态栏 / 工具栏上的文案（纯函数：端口未接就是“未绑定”）
    pub fn connection_status_text(&self, conn_id: Option<&str>) -> String {
        status_text(conn_id, &self.connection_options())
    }

    /// 某文档的绑定的展示数据（工具栏用；`None` = 未绑定或认不出）
    pub fn connection_chip(
        &self,
        conn_id: Option<&str>,
    ) -> Option<crate::connection::ConnectionChip> {
        chip_for(conn_id, &self.connection_options())
    }

    /// 【B13】注入通道门控端口（**宿主调用一次**：workbench 读连接的本地加速开关与 DuckDB 就绪）
    pub fn attach_channels(&self, port: ChannelsHandle) {
        *self.channels.borrow_mut() = Some(port);
    }

    /// 是否接了通道端口（未接 = 加速 / 联邦都不可用，并给出原因，而不是假装可选）
    pub fn has_channels(&self) -> bool {
        self.channels.borrow().is_some()
    }

    /// 某连接上「本地加速 / 联邦」的可用性（**渲染路径可调**：实现必须是内存快照）
    pub fn channel_availability(&self, conn_id: Option<&str>) -> ChannelAvailabilitySet {
        let guard = self.channels.borrow();
        match guard.as_ref() {
            Some(port) => port.availability(conn_id),
            None => ChannelAvailabilitySet::blocked("尚未接入通道能力"),
        }
    }

    /// 【T1.6】注入源清单端口（**宿主调用一次**：workbench 读引擎的联邦会话快照）
    pub fn attach_sources(&self, port: SourcesHandle) {
        *self.sources.borrow_mut() = Some(port);
    }

    /// 是否接了源清单端口
    pub fn has_sources(&self) -> bool {
        self.sources.borrow().is_some()
    }

    /// 某连接上的联邦源清单（**渲染路径可调**：实现必须是内存快照；`None` = 还没会话）
    pub fn sources_snapshot(&self, conn_id: &str) -> Option<crate::sources::SourcesSnapshot> {
        let guard = self.sources.borrow();
        guard.as_ref()?.snapshot(conn_id)
    }

    /// 注入项目态端口（**宿主调用一次**：workbench 读项目锁）
    pub fn attach_project(&self, port: ProjectHandle) {
        *self.project.borrow_mut() = Some(port);
    }

    /// 项目态（**渲染路径可调**：实现必须是内存读；未接端口 = 不读只）
    pub fn project_state(&self) -> ProjectState {
        let guard = self.project.borrow();
        match guard.as_ref() {
            Some(port) => port.state(),
            None => ProjectState::default(),
        }
    }

    /// 项目现在是不是只读（闸门用的就是它）
    pub fn project_read_only(&self) -> bool {
        self.project_state().read_only
    }

    /// 注入补全端口（**宿主调用一次**：workbench 接连接级元数据缓存）
    pub fn attach_completion(&self, port: CompletionHandle) {
        *self.completion.borrow_mut() = Some(port);
    }

    /// 【2b】注入环境端口（**宿主调用一次**：workbench 接系统解释器探测）
    pub fn attach_env(&self, port: crate::env::EnvHandle) {
        *self.env.borrow_mut() = Some(port);
    }

    /// 【2b】可用环境列表（未接端口 = 空，如实；缺解释器 = 运行不亮）
    pub fn envs(&self) -> Vec<crate::env::Environment> {
        let port = self.env.borrow();
        port.as_ref().map(|port| port.list()).unwrap_or_default()
    }

    /// 【2b】该环境是否已安装（命令可达；运行按钮据此亮/灰）
    pub fn env_installed(&self, env_id: &str) -> bool {
        let port = self.env.borrow();
        port.as_ref()
            .map(|port| port.installed(env_id))
            .unwrap_or(false)
    }

    /// 【2b】环境端口句柄（管理环境 modal 用：增删改要直呼宿主端口；未接 = None）
    pub fn env_port(&self) -> Option<crate::env::EnvHandle> {
        self.env.borrow().clone()
    }

    /// 【LSP】注入语言服务端口（**宿主调用一次**：workbench / 插件宿主接「文档 → 服务」）
    pub fn attach_lsp(&self, port: crate::lsp::LspPortHandle) {
        *self.lsp.borrow_mut() = Some(port);
    }

    /// 【LSP】文档当前可用的语言服务（未接端口 / 宿主判无可服务 = `None`，如实）
    pub fn lsp_service(&self, document: &DocumentId) -> Option<crate::lsp::LspSession> {
        let port = self.lsp.borrow();
        port.as_ref().and_then(|port| port.service(document))
    }

    /// 【LSP】取走**属于本文档**的语言服务事件（未接端口 = 空；端口按 `uri` 认领）
    pub fn drain_lsp_events(&self, document: &DocumentId) -> Vec<crate::lsp::LspEvent> {
        let port = self.lsp.borrow();
        port.as_ref()
            .map(|port| port.drain_for(document))
            .unwrap_or_default()
    }

    /// 【LSP】本文档是否**想要**语言服务（未接端口 = false，如实）
    ///
    /// 与 [`Self::lsp_service`] 的分工：那个只在服务就绪时给会话，这个不管启没启都回"本该配
    /// 服务"。面板据此在服务未起 / 被停 / 崩溃时保持轮询，重启后自动重连。
    pub fn lsp_wants(&self, document: &DocumentId) -> bool {
        let port = self.lsp.borrow();
        port.as_ref()
            .is_some_and(|port| port.wants(document))
    }

    /// 是否接了补全端口（未接 = 只给关键字与函数）
    pub fn has_completion(&self) -> bool {
        self.completion.borrow().is_some()
    }

    /// 【B9 切片二】模板片段（菜单弹出时读；未接端口 = 空）
    ///
    /// 与 [`Self::completion_catalog`] 同一口径：端口实现必须是**内存读**。
    pub fn completion_templates(&self) -> Vec<crate::completion::TemplateSnippet> {
        let port = self.completion.borrow();
        port.as_ref()
            .map(|port| port.templates())
            .unwrap_or_default()
    }

    /// 某文档当前的**候选目录**（**编辑路径可调**：实现必须是内存快照；未绑定连接 = 空目录）
    pub fn completion_catalog(&self, document: &DocumentId) -> Catalog {
        let (connection, schema, channel) = {
            let service = self.service.borrow();
            match service.find(document) {
                Some(doc) => (
                    doc.connection().map(str::to_string),
                    doc.schema().map(str::to_string),
                    doc.channel(),
                ),
                None => return Catalog::default(),
            }
        };
        let port = self.completion.borrow();
        match port.as_ref() {
            Some(port) => port.catalog(connection.as_deref(), channel, schema.as_deref()),
            None => Catalog::default(),
        }
    }

    /// 【T2a-2】行标识探测：表的主键列（**内存读**；结果集回执路径可调）
    ///
    /// 与 [`Self::completion_catalog`] 同一端口与口径：`conn_id` / `channel` 是**结果实际
    /// 跑在哪条连接、哪个通道**（来自 `ResultEntry`，不是文档当前绑定——执行时可能跟随
    /// 当前活动连接）。未绑定连接 / 未接端口 / 宿主探测不到 = `None`（如实不给行标识，
    /// 判据 `NoRowKey` 会把该结果整表置为只读，不编造键）。
    pub fn primary_key_for(
        &self,
        conn_id: Option<&str>,
        channel: ExecChannel,
        table: &str,
    ) -> Option<Vec<String>> {
        let port = self.completion.borrow();
        port.as_ref()
            .and_then(|port| port.primary_key(conn_id, channel, table))
    }

    /// 【P1】结果集字段注释（血缘 + 表列元数据；未接端口 = 全 `None`，渲染只画列名）
    ///
    /// `conn_id` / `channel` 是**结果实际跑在哪条连接、哪个通道**（来自 `ResultEntry`）。
    /// 返回长度与 `column_count` 一致；宿主实现必须同步 + 内存（与 `primary_key_for` 同口径）。
    pub fn column_annotations_for(
        &self,
        conn_id: Option<&str>,
        channel: ExecChannel,
        sql: &str,
        column_count: usize,
    ) -> Vec<Option<ColumnAnnotation>> {
        let port = self.completion.borrow();
        port.as_ref().map_or_else(
            || vec![None; column_count],
            |port| port.column_annotations(conn_id, channel, sql, column_count),
        )
    }

    /// 补全开不开：能力表（文本模式 / 分析模式的每单元由能力表回答）+ 编辑器只读 + 文件档位
    ///
    /// 大文件档位（>50MB）关重能力是 A13 的硬口径——**这里是那条口径的运行时落地**。
    pub fn completion_enabled(&self, document: &DocumentId) -> bool {
        let service = self.service.borrow();
        let Some(doc) = service.find(document) else {
            return false;
        };
        doc.capabilities().completion
            && doc.read_only().can_edit()
            && !doc.tier().disables_completion()
    }

    /// 【#54 B】注入某文档的语义诊断目录快照（幂等：覆盖旧快照）。宿主在「文档
    /// 绑定连接 / Schema 切换 / 导航树目录刷新」时按文档推给 editor；editor 侧不
    /// 主动向 database 要目录。
    ///
    /// `None` = 该文档不参与语义诊断（未绑定连接 / 目录未载好——如实降级，
    /// 宁缺毋滥）。同一文档重复推同目录是零成本覆盖（`HashMap` insert）。
    pub fn set_catalog_snapshot(&self, doc: &crate::model::DocumentId, catalog: Option<Arc<Catalog>>) {
        match catalog {
            Some(catalog) => {
                self.catalog_snapshots.borrow_mut().insert(doc.clone(), catalog);
            }
            None => {
                self.catalog_snapshots.borrow_mut().remove(doc);
            }
        }
    }

    /// 【#54 B】取某文档的目录快照（无条目 = 未注入，该文档扫描不参与语义诊断）
    pub fn catalog_snapshot_for(&self, doc: &crate::model::DocumentId) -> Option<Arc<Catalog>> {
        self.catalog_snapshots.borrow().get(doc).cloned()
    }

    /// 【#54 B】残留快照清理：保留 `alive` 里的文档条目，清掉其余（宿主泵每拍把
    /// 现存文档 id 传进来——关闭文档的目录快照不积内存；文档 id 不复用，残留本
    /// 无害，关一个清一个更干净）
    pub fn retain_catalog_snapshots(&self, alive: &[crate::model::DocumentId]) {
        let mut snapshots = self.catalog_snapshots.borrow_mut();
        snapshots.retain(|doc, _| alive.contains(doc));
    }
}
