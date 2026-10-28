//! 编辑器宿主面板 —— 会话持久化与模式切换（session_snapshot/save_session_now/restore_session/sync_completion_provider/sync_lsp_session/reload_from_document/request_mode_switch）
//!
//! 自 host.rs 拆分（2026-10-29）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为；本文件不得出现新的逻辑。


// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法


use crate::channel::{self, ExecChannel};
use crate::mode::{self, CellGranularity};
use crate::model::EditorMode;
// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）
use crate::view::completion;
use crate::view::dialogs;


use super::*;
impl EditorHostPanel {

    /// 当前会话快照（关文档 / 退出时保存用）
    ///
    /// 未命名文档没有稳定标识，返回 `None`（不存——重启后无法把它认回来）。
    pub fn session_snapshot(&self, cx: &App) -> Option<crate::session::SavedSession> {
        let (path, mode, channel, connection, schema) = self.with_document(|doc| {
            let path = doc.path()?.to_string_lossy().into_owned();
            Some((
                path,
                doc.mode(),
                doc.channel(),
                doc.connection().map(str::to_string),
                doc.schema().map(str::to_string),
            ))
        })??;
        let id = crate::session::session_id_for_path(std::path::Path::new(&path));

        let (cursor, selection) = {
            let state = self.editor.read(cx);
            let range = state.selected_range();
            let selection = (range.end > range.start).then_some((range.start, range.end));
            (range.start, selection)
        };

        Some(crate::session::SavedSession {
            id,
            path: Some(path),
            mode,
            channel,
            connection,
            content: self.editor_text(cx),
            cursor,
            selection,
            // 【T2c-6】工具栏布局：面板权威状态的编码串（改动即存，不设“值得存”阈值）
            toolbar: Some(self.toolbar.encode()),
            // 【T2c-1 余项】schema 绑定随会话走（`None` = 未选 → 连接默认）
            schema,
            // 【T2c-4 余项】缩进随会话走（文档级权威值）
            tab_size: Some(self.shared.document_tab_size(&self.document)),
            hard_tabs: Some(self.shared.document_hard_tabs(&self.document)),
            // 【结果集 #296】列序随会话走（面板自持；空 = 没拖过 → None 不落库）
            result_column_order: {
                let order = self.result_panel.read(cx).column_order();
                (!order.is_empty()).then_some(order)
            },
        })
    }

    /// 文档状态栏快照：全局状态栏「活动文档上下文段」的只读数据源。
    ///
    /// 与编辑器内部旧状态栏同一套数据口径（模式 / 光标 / 连接 / 通道 / 事务…）；
    /// 渲染路径零副作用（不触发 I/O、不写 Shared）。
    pub fn status_snapshot(&self, cx: &App) -> Option<EditorStatusSnapshot> {
        if self.closed {
            return None;
        }
        let mode = self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General);
        let (line, column, cursor_count) = {
            let state = self.editor.read(cx);
            let cursor = state.cursor_position();
            (
                cursor.line as usize + 1,
                cursor.character as usize + 1,
                state.cursor_count(),
            )
        };
        let communicating = self
            .with_document(|doc| doc.capabilities().execute)
            .unwrap_or(false);
        let bound = self
            .with_document(|doc| doc.connection().map(str::to_string))
            .flatten();
        let connection = if communicating {
            Some(self.shared.connection_status_text(bound.as_deref()))
        } else {
            None
        };
        let current_channel = self.channel();
        let channel = communicating
            .then(|| channel::status_text(current_channel).to_string());
        let dialect = communicating
            .then(|| {
                bound.as_deref().and_then(|conn_id| {
                    self.shared
                        .connection_options()
                        .iter()
                        .find(|option| option.id == conn_id)
                        .map(|option| crate::connection::dialect_label(&option.db_type))
                })
            })
            .flatten();
        let tx_available = communicating
            && self.shared.has_transactions()
            && current_channel.allows_transactions();
        let tx = tx_available.then(|| self.tx_text());
        let is_general = self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General)
            == EditorMode::General;
        let env = is_general
            .then(|| {
                self.env_for()
                    .and_then(|id| self.envs().into_iter().find(|env| env.id == id))
                    .map(|env| format!("环境 {}", env.display_name()))
            })
            .flatten();
        let file_type = is_general
            .then(|| {
                self.document_path()
                    .as_deref()
                    .and_then(crate::mode::script_file_type)
                    .map(|t| format!("类型 {}", t.label()))
            })
            .flatten();
        let lsp = is_general
            .then(|| {
                self.shared.lsp_wants(&self.document).then(|| {
                    if self.lsp_alive {
                        "LSP 就绪".to_string()
                    } else {
                        "LSP 未就绪".to_string()
                    }
                })
            })
            .flatten();
        let schema = self
            .with_document(|doc| doc.schema().map(str::to_string))
            .flatten();
        // 【§2.5 分析】会话 / 内核 / 单元计数（含 stale）：有真实数据源才摆。
        // 变量数暂无内核变量表数据源——按“零 UI 造数据”口径不显示（内核支持后再接）。
        let (session, kernel, units) = if mode == EditorMode::Analysis {
            let notebook = self.notebook_state();
            let session = match notebook.session.default_connection.as_deref() {
                Some(conn_id) => {
                    let name = self
                        .shared
                        .connection_options()
                        .iter()
                        .find(|option| option.id == conn_id)
                        .map(|option| option.name.clone())
                        .unwrap_or_else(|| conn_id.to_string());
                    Some(format!("会话 {name}"))
                }
                None => Some("会话 默认会话".to_string()),
            };
            let kernel = self
                .kernel_running
                .as_ref()
                .map(|run| format!("内核 {}", run.kernel_id));
            let units = Some({
                let total = notebook.cells.len();
                let stale = notebook.stale_count();
                if stale > 0 {
                    format!("{total} 单元（{stale} stale）")
                } else {
                    format!("{total} 单元")
                }
            });
            (session, kernel, units)
        } else {
            (None, None, None)
        };
        Some(EditorStatusSnapshot {
            mode,
            dirty: self.is_dirty(),
            read_only: self.read_only_flags(),
            statements: self.statements,
            line,
            column,
            cursor_count,
            tab_size: self.shared.document_tab_size(&self.document),
            hard_tabs: self.shared.document_hard_tabs(&self.document),
            format: self.shared.document_save_format(&self.document),
            executing: self.pending > 0,
            elapsed: self.running_since.map(|since| since.elapsed()),
            connection,
            dialect,
            channel,
            schema,
            env,
            file_type,
            lsp,
            tx,
            session,
            kernel,
            units,
            message: self.message.clone(),
        })
    }

    /// 保存会话（关文档 / 退出时调用；未命名或空文档自动跳过）
    pub fn save_session_now(&self, cx: &App) {
        let Some(session) = self.session_snapshot(cx) else {
            return;
        };
        if !crate::session::worth_saving(&session) {
            return;
        }
        if let Err(error) = self.shared.save_session(&session) {
            // 事件路径上不弹窗：落到 stderr 由日志承接，不静默吞掉
            eprintln!("[editor] 会话保存失败（{}）：{error}", session.id);
        }
    }

    /// 恢复会话：把光标/选区写回内核（模式与内容由宿主开文档时给出）
    pub fn restore_session(
        &mut self,
        session: &crate::session::SavedSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 会话可能比现在的文档长（内容被截断 / 外部修改）：光标钳到当前文本末尾
        let len = self.editor_text(cx).len();
        let clamp = |offset: usize| offset.min(len);
        let start = clamp(
            session
                .selection
                .map(|(start, _)| start)
                .unwrap_or(session.cursor),
        );
        let end = clamp(
            session
                .selection
                .map(|(_, end)| end)
                .unwrap_or(session.cursor),
        );

        self.editor.update(cx, |state, cx| {
            // `set_cursor_position` 是内核里唯一“滚到指定偏移”的公开入口（会顺带聚焦）
            let position = {
                use gpui_kit::component::input::RopeExt as _;
                state.text().offset_to_position(start)
            };
            state.set_cursor_position(position, window, cx);
            state.set_selected_range(start..end, cx);
        });
        // 【B13】通道也随会话回来；**已失效就回退源库并说清原因**（原型 §5.7：绑定过但
        // 现已失效——比如之后再打开时那个连接没开本地加速——不能静静地接着用它）
        self.restore_channel(session.channel, cx);
        // 【B1】连接绑定同理：那条连接可能已经被删了 / 不在当前项目
        self.restore_connection(session.connection.as_deref(), cx);
        // 【T2c-6】工具栏布局：旧会话没存（`None`）或解码失败 → 保持默认，不覆盖当前
        if let Some(code) = session.toolbar.as_deref() {
            if let Some(layout) = crate::toolbar::ToolbarLayout::decode(code) {
                self.toolbar = layout;
            }
        }
        // 【T2c-1 余项】schema 绑定：必须在连接恢复之后（schema 列表按连接查）
        self.restore_schema(session.schema.as_deref(), cx);
        // 【T2c-4 余项】缩进双写（文档状态 + 内核 TabSize）；先宽度后制表符（后者读前者）
        if let Some(size) = session.tab_size {
            self.set_tab_size(size, cx);
        }
        if let Some(hard) = session.hard_tabs {
            self.set_hard_tabs(hard, cx);
        }
        // 【结果集 #296】列序随会话回来：注入面板（新结果按列名匹配重排）。
        // 面板在文档打开时已构造，这里 update 一次即可。
        if let Some(order) = session.result_column_order.as_deref() {
            if !order.is_empty() {
                self.result_panel.update(cx, |panel, _cx| {
                    panel.set_column_order(order.to_vec());
                });
            }
        }
        cx.notify();
    }

    /// 【T2c-1 余项】把会话里的 **schema 绑定**写回文档
    ///
    /// 判据同 `restore_connection`：schema 列表里没有 = schema 被删 / 换库 → **回退到
    /// 「使用连接默认」并把原因说出来**。schema 列表按**当前连接**查（连接恢复在前）。
    /// 端口未接 / 连接未绑 = **不校验也不丢**（宁可不猜也不把用户的绑定抹掉）。
    fn restore_schema(&mut self, schema: Option<&str>, cx: &mut Context<Self>) {
        let Some(schema_name) = schema else {
            return;
        };
        let conn = self.bound_connection();
        let list = self.shared.schemas(conn.as_deref());
        let apply = |service: &mut crate::service::EditorService| {
            service.set_schema(&self.document, Some(schema_name.to_string()))
        };
        if list.is_empty() {
            // 端口未接 / 连接未绑：不校验也不丢（与 restore_connection 同语义）
            self.shared.update(apply);
            return;
        }
        if list.iter().any(|info| info.schema == schema_name) {
            self.shared.update(apply);
            return;
        }
        self.shared.update(|service| {
            service.set_schema(&self.document, None)
        });
        self.set_message(
            Some(format!(
                "schema {schema_name} 不在当前连接里：已回退到连接默认"
            )),
            cx,
        );
    }

    /// 【B1 余项】把会话里的**连接绑定**写回文档
    ///
    /// 判据用宿主给的连接列表快照：列表里没有 = 连接被删 / 换了项目 → **回退到「跟随当前连接」
    /// 并把原因说出来**（留一个指向不存在连接的绑定，会让用户执行时才发现发错了地方）。
    /// 端口未接 = **不校验也不丢**（测试与嵌入场景；宁可不猜也不把用户的绑定抹掉）。
    /// 不主动建连：启动恢复不该拨号，执行时由执行器按绑定去建/去用。
    fn restore_connection(&mut self, connection: Option<&str>, cx: &mut Context<Self>) {
        let Some(conn_id) = connection else {
            return;
        };
        if !self.shared.has_connections() {
            self.shared.update(|service| {
                service.set_connection(&self.document, Some(conn_id.to_string()))
            });
            return;
        }
        if self
            .shared
            .connection_options()
            .iter()
            .any(|option| option.id == conn_id)
        {
            self.shared.update(|service| {
                service.set_connection(&self.document, Some(conn_id.to_string()))
            });
            return;
        }
        self.set_message(
            Some(format!(
                "连接 {conn_id} 不在当前连接列表：已回退到「跟随当前连接」"
            )),
            cx,
        );
    }

    /// 【B13】把会话里的通道写回文档（不可用 → 留源库档 + 状态栏给原因）
    fn restore_channel(&mut self, channel: ExecChannel, cx: &mut Context<Self>) {
        if channel == ExecChannel::Source {
            return;
        }
        let availability = self
            .shared
            .channel_availability(self.bound_connection().as_deref());
        let gate = availability.for_channel(channel);
        if gate.available {
            self.shared
                .update(|service| service.set_channel(&self.document, channel));
            self.sync_result_view(cx);
            return;
        }
        let reason = gate.reason.unwrap_or_else(|| "不可用".to_string());
        self.set_message(
            Some(format!(
                "{}通道已失效：{reason}（回退源库）",
                channel.label()
            )),
            cx,
        );
    }

    /// 按当前文档的能力与档位装 / 摘补全 provider（**唯一开关处**：新建与切模式都走它）
    ///
    /// 三选一（纯逻辑在 [`crate::lsp::host::choose_completion`]）：
    /// - SQL 模式 → 本地 SQL 目录补全（`completion_enabled`：能力表 + 只读 + 大文件档位）；
    /// - 通用 / 分析模式且语言服务健康可编辑 → LSP 补全（请求转发给语言服务）；
    /// - 都没有 → 摘干净（两个 provider 都摘，不残留）。
    pub(crate) fn sync_completion_provider(&self, state: &mut gpui_kit::component::input::EditorState) {
        let sql_enabled = self.shared.completion_enabled(&self.document);
        let lsp_available = self.lsp_alive
            && self.lsp.is_some()
            && !self.editor_read_only()
            && !self
                .with_document(|doc| doc.tier().disables_completion())
                .unwrap_or(true);
        let mode = self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General);
        match crate::lsp::host::choose_completion(mode, lsp_available, sql_enabled) {
            crate::lsp::CompletionChoice::Lsp => {
                if let Some(session) = self.lsp.as_ref() {
                    let provider = std::rc::Rc::new(crate::lsp::bridge::LspCompletionProvider::new(
                        session.client.clone(),
                        session.uri.clone(),
                    ));
                    crate::lsp::bridge::install(state, provider);
                    // 【#21】hover 与补全同源（语言服务）：一起挂、一起摘。
                    // 弹层由内核自带（150ms 防抖 + 自动渲染/收起），这里只补数据源。
                    let hover = std::rc::Rc::new(crate::lsp::bridge::LspHoverProvider::new(
                        session.client.clone(),
                        session.uri.clone(),
                    ));
                    crate::lsp::bridge::install_hover(state, hover);
                    // 【A1】通用模式语言服务的 code action：转发 `textDocument/codeAction`
                    // 到同一会话（诊断 / 补全 / hover 同源）。Ctrl+. 由内核绑定，挂上即弹层。
                    crate::lsp::sql_code_action::install(
                        state,
                        std::rc::Rc::new(crate::lsp::bridge::LspCodeActionProvider::new(
                            session.client.clone(),
                            session.uri.clone(),
                        )),
                    );
                }
            }
            crate::lsp::CompletionChoice::Sql => {
                completion::install(state, self.shared.clone(), self.document.clone());
                // 【#54 ②】SQL 模式 hover 走自研语义层（函数签名+中文描述+示例；
                // 表/列 hover 待目录注入）。方言 = 文档绑定连接的驱动（`sql_dialect_for_document`，
                // 与跳转/语义诊断同一口径）；未绑定 / 驱动未知 → Ansi（解析失败返回 None，安全）。
                let dialect = self.sql_dialect_for_document();
                crate::lsp::bridge::install_hover(
                    state,
                    crate::lsp::sql_hover::SqlHoverProvider::new(dialect),
                );
                // 【#54 C6/C7】跳转定义与 hover 同源（自研语义层，同一方言）：一起挂。
                // 内核 hover 预取自动出「下划线 + Ctrl+Click hitbox」（definitions.rs），
                // 键盘 GoToDefinition 同槽位；目标编码成 rds://navigator/<限定表名> 虚拟
                // scheme，由 workbench 侧 show_document handler 定位导航树（返回 true
                // 自处理 / false 内核默认文档内移动）。
                crate::lsp::bridge::install_definition(
                    state,
                    crate::lsp::sql_definition::SqlDefinitionProvider::new(dialect),
                );
                // 【#54 C6/C7】show_document handler：内核跳转对 `rds://navigator/…`
                // 虚拟 URI 先问宿主（端口已注入则解析成导航树定位并返回 true）；未注入
                // / 其他 scheme → None（内核走默认文档内移动，无副作用）。
                crate::lsp::bridge::install_show_document(state, self.shared.show_document_port());
                // 【A1】SQL 改写型 code action（ADD_WHERE / ADD_LIMIT）：挂内核
                // `code_action_providers` 槽位（多槽 Vec，push 不顶掉别的）。Ctrl+. 已由
                // 内核绑定（`ToggleCodeActions`），挂上即可弹层。通道参与判定（ADD_WHERE
                // 只在源库档给：加速 / 联邦对源库只读挂载，改写无意义）。文档 URI 用
                // 磁盘路径（无路径 = 未命名文档，用 rds://editor/<id> 占位——内核应用
                // 编辑走编辑器本地坐标，URI 只用于 WorkspaceEdit 结构完整性）。
                let uri = self
                    .with_document(|doc| {
                        doc.path()
                            .map(|path| path.to_string_lossy().into_owned())
                            .unwrap_or_else(|| format!("rds://editor/{}", doc.id()))
                    })
                    .unwrap_or_else(|| "rds://editor/untitled".to_string());
                crate::lsp::sql_code_action::install(
                    state,
                    crate::lsp::sql_code_action::SqlCodeActionProvider::new(
                        dialect,
                        self.channel(),
                        uri,
                    ),
                );
                // 【A2】颜色字面量色块：识别 `#rgb` / `#rrggbb` / `#rrggbbaa`（含字符串 /
                // 注释 / x'…' 内）。内核 100ms 防抖自动调 `document_colors()` 并渲染色块，
                // 这里只补数据源。SQL 模式挂（自研语义层不参与颜色）；通用模式走语言服务
                // 的 LSP 分支**不挂**（语言服务真 LSP 自己报颜色，别双重）。
                crate::lsp::document_color::install(state);
            }
            crate::lsp::CompletionChoice::None => {
                crate::lsp::bridge::uninstall(state);
                crate::lsp::bridge::uninstall_hover(state);
                crate::lsp::bridge::uninstall_definition(state);
                crate::lsp::bridge::uninstall_show_document(state);
                crate::lsp::sql_code_action::uninstall(state);
                crate::lsp::document_color::uninstall(state);
                completion::uninstall(state);
            }
        }
    }

    /// 【LSP】按当前文档重新向宿主要服务（建面板 / 切模式 / 泵每拍都调用）
    ///
    /// 会话有变化才动作：换会话先给旧服务发 `didClose`（尽力），清旧诊断，再 `didOpen`
    /// 新会话并置健康；没变化 = 无操作（不重复 didOpen）。服务不可得 = 如实摘干净。
    ///
    /// **两个判据都不可省**：① `uri` / `language_id` 相同也可能是**换了连接**（server 崩溃
    /// 重启后 uri 不变），所以并上 `Arc::ptr_eq`；② 末尾**无条件**按「会话健康」或「本该有
    /// 服务」决定是否保持泵——`.py` 文档初始化期间正是「没会话但想要」，early return 会错过它。
    pub(crate) fn sync_lsp_session(&mut self, cx: &mut Context<Self>) {
        let session = self.shared.lsp_service(&self.document);
        let same = match (&self.lsp, &session) {
            (Some(old), Some(new)) => {
                old.uri == new.uri
                    && old.language_id == new.language_id
                    && std::sync::Arc::ptr_eq(&old.client, &new.client)
            }
            (None, None) => true,
            _ => false,
        };
        if !same {
            // 旧会话收尾（尽力）：关文档 + 清诊断（服务端缓存别留垃圾）
            if let Some(old) = self.lsp.take() {
                let _ = old.client.did_close(&old.uri);
            }
            self.lsp_diagnostics.clear();
            self.lsp_version = 0;
            self.lsp_alive = false;
            if let Some(session) = session {
                let text = self.editor_text(cx);
                let _ = session.client.did_open(&session.uri, &session.language_id, &text);
                self.lsp_version = 1;
                self.lsp_alive = true;
                self.lsp = Some(session);
            }
            // 会话变了（挂或摘）都要整体重画一遍三源诊断：摘了不清内核里旧的 LSP 标记
            self.apply_error_marks(self.error_reason.clone(), cx);
            // provider 的摘 / 挂跟着会话走（泵里重连也靠这里——不只 `new` / `sync_mode` 那两次）
            self.editor.update(cx, |state, _cx| {
                self.sync_completion_provider(state);
            });
        }
        // 会话健康，或「本该有服务但还没起 / 已被停」→ 保持轮询（重启后自动重连靠这条）
        if self.lsp_alive || self.shared.lsp_wants(&self.document) {
            self.ensure_exec_pump(cx);
        }
    }

    /// 把文档内容推回内核（另存为 / 外部修改后重新加载时调用）
    pub fn reload_from_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.with_document(|doc| doc.content().to_string()) else {
            return;
        };
        // 【2d/T2d-5】分析模式：正文是 `.rdsnote` JSON（会话 + 单元）——解析进笔记本持有器，
        // 渲染读它；解析失败如实报（不假装打开成功）。其它模式照旧推内核文本。
        let mode = self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General);
        if mode == EditorMode::Analysis {
            if text.trim().is_empty() {
                self.shared.set_notebook_state(&self.document, crate::cell::NotebookState::new());
            } else {
                match crate::cell::NotebookFile::from_json(&text) {
                    Ok(file) => {
                        let (session, cells) = file.into_state();
                        self.shared.set_notebook_state(
                            &self.document,
                            crate::cell::NotebookState {
                                session,
                                cells,
                            },
                        );
                    }
                    Err(reason) => {
                        self.set_message(Some(format!("打开 .rdsnote 失败：{reason}")), cx);
                    }
                }
            }
        }
        self.statements = count_statements(&text);
        let value_changed = {
            let before = self.editor.read(cx).value().to_string();
            let changed = before != text;
            self.editor.update(cx, |state, cx| {
                if changed {
                    state.set_value(text.clone(), window, cx);
                }
                // **`set_value` 不发 `InputEvent::Change`**（内核里 `emit_events = false`，`state.rs:901`），
                // 所以整篇换文本这条路不会经过事件订阅——折叠候选必须在这里显式重算，
                // 否则重新加载后候选还指着旧文本的行号。
                sync_fold(&self.shared, &self.document, state, window, cx);
            });
            changed
        };
        // 【LSP】同上：整篇换文本不走 Change 事件，didChange 也要在这里显式补发（版本自增）
        if value_changed && self.lsp_alive {
            if let Some(session) = self.lsp.as_ref() {
                self.lsp_version += 1;
                let _ = session.client.did_change(&session.uri, self.lsp_version, &text);
            }
        }
        // 【2c】同上：隐藏字符标注也走不到 Change 事件（区间是字节偏移，整篇换文本必失效），
        // 开着显示就显式重算一遍；没开则集合未建，直接跳过。
        if self.shared.document_show_invisible(&self.document) {
            let ranges = self.invisible_ranges_for(cx);
            self.refresh_invisible_decorations(ranges, cx);
        }
        cx.notify();
    }

    // ===== 动作（A10）=====
    //
    // 动作只改共享状态或向内核写文本，不直接操作 Dock（关闭走 TabGroup 的请求路径）。

    /// 请求切换到目标模式（工具栏模式指示器调用）
    pub(crate) fn request_mode_switch(
        &mut self,
        to: EditorMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        if from == to {
            return;
        }

        let plan = self.switch_plan(from, to, CellGranularity::default(), cx);
        let Some(kind) = plan.confirm else {
            // 免确认（文本 → SQL）：直接切
            self.apply_mode_switch(plan, window, cx);
            return;
        };

        let entity = cx.entity();
        dialogs::open_switch_confirm(
            window,
            cx,
            kind,
            from,
            to,
            plan.note,
            move |granularity, window, cx| {
                entity.update(cx, |panel, cx| {
                    panel.confirm_mode_switch(to, granularity, window, cx);
                });
            },
        );
    }

    /// 确认后的落地（对话框回调调用）
    ///
    /// “从哪个模式切过来”在这里**重新读**而不是用弹窗前的快照：粒度是确认时才拿到的输入，
    /// 计划必须带着它重算（拿弹窗前那份就等于丢掉用户的选择）。
    pub fn confirm_mode_switch(
        &mut self,
        to: EditorMode,
        granularity: CellGranularity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        let plan = self.switch_plan(from, to, granularity, cx);
        self.apply_mode_switch(plan, window, cx);
    }

    /// 算一份切换计划（读取当前文本与“有没有结果”，交给纯函数判定）
    fn switch_plan(
        &self,
        from: EditorMode,
        to: EditorMode,
        granularity: CellGranularity,
        cx: &App,
    ) -> mode::SwitchPlan {
        mode::plan_switch(
            from,
            to,
            &self.editor_text(cx),
            self.result_panel.read(cx).toolbar().is_some(),
            granularity,
        )
    }

    /// 落地一次模式切换（**已确认**）：内容变换 → 模式变更 → 视图刷新
    ///
    /// 内容先写回 `EditorService`，再让面板从文档重载（`reload_from_document`）——
    /// 内容只有一条真值路径，不直接把变换结果塞进内核（否则服务层与内核就分家了）。
    fn apply_mode_switch(
        &mut self,
        plan: mode::SwitchPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rewritten = match plan.content {
            mode::SwitchContent::Unchanged => None,
            mode::SwitchContent::Sql(text) => Some(text),
            mode::SwitchContent::Cells(cells) => Some(mode::cells_to_text(&cells)),
        };
        if let Some(text) = rewritten {
            let statements = count_statements(&text);
            self.shared
                .update(|service| service.set_content(&self.document, text));
            self.statements = statements;
        }
        self.shared
            .update(|service| service.set_mode(&self.document, plan.to));
        self.sync_mode(window, cx);
        self.reload_from_document(window, cx);
        self.set_message(plan.note.map(str::to_string), cx);
    }

    // ===== 执行（A14）=====
    //
    // 四个入口（Ctrl+Enter / Ctrl+Shift+Enter / 工具栏主按钮 / 执行族菜单）共用 `execute`：
    // 目标解析 → 提交 → 轮询回填。界面不等 I/O。
}
