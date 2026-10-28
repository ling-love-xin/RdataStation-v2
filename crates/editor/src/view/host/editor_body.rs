//! 编辑器宿主面板 —— 编辑区渲染与提交（render_editor_body/render_write_hint/只读检查/commit_pending/discard_pending_changes/bind_connection）
//!
//! 自 host.rs 拆分（2026-10-29）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为；本文件不得出现新的逻辑。


use gpui_kit::base::text::TextView;
use gpui_kit::component::input::Editor;
// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法


use crate::channel::{self, ExecChannel};
use crate::execution::ExecTarget;
use crate::model::{EditorMode, ReadOnly};
// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）
use crate::view::editor_menu::{editor_context_menu_entries, menu_from_entries};


use super::*;
impl EditorHostPanel {

    /// 编辑区正文：分析模式 = 笔记本（单元卡片）；其它模式 = 内核编辑器
    pub(crate) fn render_editor_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let mode = self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General);
        if mode != EditorMode::Analysis {
            // 【遗留】通用模式 Markdown 文档的渲染态：渲染视图（只读）替代源码编辑器。
            // 判据 = 预览开关 + 文档类型是 Markdown；其余文档一律走编辑器。
            if self.markdown_preview && self.is_markdown_document() {
                let source = self
                    .with_document(|doc| doc.content().to_string())
                    .unwrap_or_default();
                // 外层带 id + test_support 的容器：headless 测试用 find 断言渲染视图出现
                return div()
                    .id("markdown-preview-root")
                    .test_support()
                    .size_full()
                    .child(TextView::markdown("md-preview", source).scrollable(true).size_full())
                    .into_any_element();
            }
            // 需求② 打开即编辑态：不再用欢迎页挡路（点「开始编写」才进编辑），空白文档
            // 直接可编辑（导航打开的 SQL 同理，免点按钮直接进编辑期 SQL 模式）。
            // 写入模式提示作为居中水印叠加在空编辑器背景上，输入任意字符即消失。
            let read_only = self.editor_read_only();
            // 【B2】正文右键菜单：用 `Editor::context_menu` 定制中文菜单（覆盖 Input
            // 自带英文默认菜单）。能力位在闭包里读**捕获的内核实体**判断：
            // - 可编辑（非只读/未禁用）→ 剪切/粘贴可用；有选区 → 复制/剪切可用；
            // - 挂了 definition / code action provider → 显示对应项。
            // 菜单项纯函数化（`editor_context_menu_entries`）便于 headless 断言顺序与禁用态。
            let editor_entity = self.editor.clone();
            let context_menu = move |_menu: gpui_kit::component::native_menu::NativeMenu,
                                      _window: &mut Window,
                                      cx: &mut App| {
                // 先在小作用域里读状态构造条目（`entries` 是拥有型 Vec，
                // 块结束即释放 `cx` 上的不可变借用），再翻译成菜单
                let entries = {
                    let state = editor_entity.read(cx);
                    editor_context_menu_entries(state)
                };
                menu_from_entries(entries)
            };
            let editor = Editor::new(&self.editor)
                .appearance(false)
                .bordered(false)
                .readonly(read_only)
                .context_menu(context_menu)
                .size_full();
            let mut body = div().relative().size_full().pl_0().child(editor);
            if !read_only && self.editor_is_empty() {
                body = body.child(self.render_write_hint(cx));
            }
            return body.into_any_element();
        }
        // 分析模式：笔记本正文（单元卡片列表 / 空态引导卡）。编辑区可滚动。
        let state = self.notebook_state();
        let entity = cx.entity();
        // 连接列表是内存快照（右下角执行源选择器数据源）；render 纯读
        let connections = self.shared.connection_options();
        // 【#20】内核注册表快照（执行源「内核」档的菜单数据源）；render 纯读
        let kernels = self.shared.kernel_specs();
        // render 纯读：单元编辑器实例按引用传入（不能 read 正在被 render 的面板）
        crate::view::analysis::render(
            cx,
            &state,
            &self.document,
            entity,
            &self.cell_editors,
            &connections,
            &kernels,
        )
        .into_any_element()
    }

    /// 写入模式提示的显示条件（需求②）：编辑器可编辑且内容为空——输入任意字符即消失。
    pub(crate) fn editor_is_empty(&self) -> bool {
        self.with_document(|doc| doc.content().trim().is_empty())
            .unwrap_or(true)
    }

    /// 写入模式背景水印（需求②）：空白编辑器的居中弱色提示，替代原欢迎页的「开始编写」
    /// 挡路按钮——打开即编辑态，提示只作背景说明、不拦截输入；内容非空后不再渲染。
    /// 无任何鼠标 / 键盘 handler：gpui 命中测试只认带 handler 的元素，故水印不会挡住
    /// 下方的编辑内核获取焦点（非交互覆盖层的通行做法）。
    fn render_write_hint(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_sm()
                    .italic()
                    .text_color(muted)
                    .child("写入模式 · 直接输入开始编写"),
            )
            .into_any_element()
    }

    /// 【B13】当前通道能不能跑写语句（本地加速档对源库是**只读挂载**，写语句直接拒）
    pub(crate) fn channel_write_check(&self, target: &ExecTarget) -> Result<(), String> {
        // 【B15】本地分析不碰源库（数据是桥接过来的行）——通道闸管的是“写源库对象”，放行
        if matches!(target, ExecTarget::Analysis(_)) {
            return Ok(());
        }
        let channel = self.channel();
        if channel.allows_source_writes() {
            return Ok(());
        }
        for sql in target.statements() {
            channel::statement_allowed(channel, &sql)?;
        }
        Ok(())
    }

    /// 项目只读闸（原型 §1.4 的“连接只读”维度）：写源库对象的语句一律拒
    ///
    /// 与通道闸**互不替代**：通道闸管“在哪儿跑”（本地两档对源库是只读挂载，永远不能写），
    /// 这里管“项目锁着的时候能不能写库”（解锁后就能写）。读语句一律不受影响。
    ///
    /// **强度**：项目锁（另一实例占用 / 归档项目）就属架构 §13 #9 里的“**强只读**”那一档 →
    /// 直接拒；连接策略里的“提醒后放行”那档随连接策略（B1 余项）一起做，现在不假装有。
    /// 判据与通道闸同一份（[`channel::writes_source_object`]）：宁可多拒一句，不能放过去真写。
    pub(crate) fn project_write_check(&self, target: &ExecTarget) -> Result<(), String> {
        // 【B15】本地分析同上：它在本地 DuckDB 上建临时表，与项目锁无关
        if matches!(target, ExecTarget::Analysis(_)) {
            return Ok(());
        }
        if !self.shared.project_read_only() {
            return Ok(());
        }
        for sql in target.statements() {
            if channel::writes_source_object(&sql) {
                return Err("项目为只读模式：写语句被拒（要改数据先解锁项目）".to_string());
            }
        }
        Ok(())
    }

    /// 状态栏用的只读两维度：文档自身的那一维 + **项目锁**（连接只读那一维的真值）
    ///
    /// 合成在这里而不写回文档：项目锁是窗口级状态，写进 `Document` 会留下“解锁了但文档还
    /// 写着只读”的陈旧值。
    pub(crate) fn read_only_flags(&self) -> ReadOnly {
        let document = self
            .with_document(|doc| doc.read_only())
            .unwrap_or_default();
        ReadOnly {
            // 用户只读开关（roLock）并入编辑器维度
            editor: document.editor || self.user_ro_lock,
            connection: document.connection || self.shared.project_read_only(),
        }
    }

    /// 【2026-10-27】用户只读开关（工具栏 roLock）：点击在 只读/编辑 间翻转
    ///
    /// 只读是窗口级合成（见 [`Self::read_only_flags`]），不写回 Document——与项目锁同一
    /// 口径；切换时同步内核 readonly（编辑行为立即受限），状态栏「只读」徽标随之出现。
    pub(crate) fn toggle_user_ro_lock(&mut self, cx: &mut Context<Self>) {
        self.user_ro_lock = !self.user_ro_lock;
        let read_only = self.editor_read_only();
        self.editor.update(cx, |state, cx| {
            state.set_readonly(read_only, cx);
        });
        cx.notify();
    }

    /// 【T2a-6】批量提交当前结果集的变更集（提交前复核四道防线，与编辑判据同一套口径）
    ///
    /// 四道防线（通道 / 项目只读 / 连接只读 / 无行标识）在**提交前**复核而不是只信编辑时的
    /// 判据：用户可能改完几格才切通道 / 锁项目，提交时防线已变——这里把变了的拦下。
    ///
    /// 返回 `Err(可读原因)` = 提交被拦（调用方负责展示）；成功则把这次提交记为在途
    /// （[`CommitInFlight`]），回执回来在轮询泵里组装冲突并落状态栏。
    pub(crate) fn submit_pending_changes(&mut self, _cx: &mut Context<Self>) -> Result<(), String> {
        // ① 提交中不再接新提交（回执回来前按钮保持灰）
        if self.commit_in_flight.is_some() {
            return Err("已有提交在进行中（等回执回来）".to_string());
        }
        // ② 通道闸：写回只走源库通道（加速 / 联邦只读挂载，改不了源库）
        let channel = self.channel();
        if !channel.allows_source_writes() {
            return Err(format!("提交只支持源库通道（当前是 {}）", channel.label()));
        }
        // ③ 项目只读（沿用 project_write_check 的判据：强只读直接拒）
        if self.shared.project_read_only() {
            return Err("项目为只读模式：写语句被拒（要改数据先解锁项目）".to_string());
        }
        // ④ 连接只读
        let connection_read_only = self
            .with_document(|doc| doc.read_only().connection)
            .unwrap_or(false);
        if connection_read_only {
            return Err("连接为只读模式：批量提交被拒".to_string());
        }
        // ⑤ 无行标识 → 整份只读（判据层已拦住编辑；这里防判据变化后仍能提交）
        let entry = self
            .shared
            .results_active(&self.document)
            .ok_or_else(|| "当前没有可提交的结果集".to_string())?;
        let set_index = self
            .shared
            .results()
            .active_index(&self.document)
            .ok_or_else(|| "当前没有可提交的结果集".to_string())?;
        let key_columns = entry
            .key_columns
            .clone()
            .ok_or_else(|| "这份结果没探测到主键：改不了（提交靠主键定位行）".to_string())?;
        let key = crate::export::RowKey::new(key_columns)
            .ok_or_else(|| "主键列为空（不该发生：探测到就不会是空）".to_string())?;

        // ⑥ 生成 UPDATE 文本（含语句→行/列索引）；表名与主键探测同一处猜测（probe_primary_key）
        let table = crate::export::default_table_name(Some(&entry.sql), 0);
        // pending 克隆出借用边界（不拖住 results() 的 Ref）；被删底层行的格级变更不再生成
        // UPDATE（该行已在本地标记删除，#28-B1 排除矩阵：提交批次排除被删行）。同时探测本地
        // 叠加层动作（新增 / 复制 / 删除的本地行）——本期不下发，回执文案据此提示。
        let (pending, local_pending): (Vec<crate::changes::CellChange>, bool) = {
            let results = self.shared.results();
            let deleted: std::collections::BTreeSet<usize> = results
                .local_rows(&self.document, set_index)
                .map(|set| set.deleted_rows().iter().copied().collect())
                .unwrap_or_default();
            let pending = results
                .change_set(&self.document, set_index)
                .map(|changes| {
                    changes
                        .pending()
                        .iter()
                        .filter(|c| !deleted.contains(&c.row))
                        .map(|c| (*c).clone())
                        .collect()
                })
                .unwrap_or_default();
            let local_pending = results
                .local_rows(&self.document, set_index)
                .map(|set| !set.is_empty())
                .unwrap_or(false)
                || results
                    .row_history(&self.document, set_index)
                    .map(|history| history.can_undo() || history.can_redo())
                    .unwrap_or(false);
            (pending, local_pending)
        };
        if pending.is_empty() {
            return Err("没有待提交的变更".to_string());
        }
        let mut by_row: std::collections::BTreeMap<usize, Vec<&crate::changes::CellChange>> =
            std::collections::BTreeMap::new();
        for change in &pending {
            by_row.entry(change.row).or_default().push(change);
        }
        let updates = crate::changes::commit_updates_rows(
            &table,
            &key,
            &entry.columns,
            &entry.rows,
            &by_row,
        )?;

        // ⑦ 请执行器提交；成则记为在途（回执回来前不重复提交）
        self.shared.request_commit_changes(
            self.document.clone(),
            table,
            key,
            entry.sql.clone(),
            updates.sql,
        )?;
        self.commit_in_flight = Some(CommitInFlight {
            set_index,
            per_line: updates.per_line,
            skipped: updates.skipped,
            local_pending,
        });
        Ok(())
    }

    /// 提交中的真值（结果工具栏据此把「提交变更」置灰 / 显示进行中）
    pub fn commit_pending(&self) -> bool {
        self.commit_in_flight.is_some()
    }

    /// 【T2a-7】丢弃全部**待提交**变更（回滚：DBeaver 提交/回滚对的「回滚」侧）
    ///
    /// 清掉变更集里的 Pending 项（Committed 已落库、Conflict 是证据，都保留），
    /// 网格的脏格底色随 [`Self::sync_result_view`] 的投影一起消失。
    pub(crate) fn discard_pending_changes(&mut self, cx: &mut Context<Self>) {
        let index = self.result_panel.read(cx).active();
        let changed = self.shared.update_results(|store| {
            store.mutate_change_set(&self.document, index, |changes| changes.discard_pending())
        });
        if changed {
            self.sync_result_view(cx);
            self.set_message(Some("已撤销未提交的变更".to_string()), cx);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn commit_in_flight_for_test(
        &self,
    ) -> Option<(usize, Vec<(usize, Vec<usize>)>, usize)> {
        self.commit_in_flight.as_ref().map(|inflight| {
            (
                inflight.set_index,
                inflight.per_line.clone(),
                inflight.skipped,
            )
        })
    }

    /// 【B13】通道现在能不能执行（不可用就给可读原因）
    pub(crate) fn channel_ready(&self) -> Result<(), String> {
        let channel = self.channel();
        if channel == ExecChannel::Source {
            return Ok(());
        }
        let availability = self
            .shared
            .channel_availability(self.bound_connection().as_deref());
        let gate = availability.for_channel(channel);
        if gate.available {
            Ok(())
        } else {
            Err(format!(
                "{}不可用：{}",
                channel.label(),
                gate.reason.unwrap_or_else(|| "不可用".to_string())
            ))
        }
    }

    /// 绑定连接（工具栏/状态栏面包屑调用）：先用端口确保已建连，失败就**不绑定**并留原因
    ///
    /// `None` = 解绑（跟随当前连接）。绑定是**文档属性**，同一窗口的多份文档互不影响。
    pub(crate) fn bind_connection(&mut self, conn_id: Option<String>, cx: &mut Context<Self>) {
        let Some(conn_id) = conn_id else {
            self.shared
                .update(|service| service.set_connection(&self.document, None));
            self.set_message(None, cx);
            return;
        };

        match self.shared.ensure_connected(&conn_id) {
            Ok(()) => {
                self.shared.update(|service| {
                    service.set_connection(&self.document, Some(conn_id.clone()));
                });
                let chip = self.shared.connection_status_text(Some(&conn_id));
                self.set_message(Some(format!("已绑定 {chip}")), cx);
            }
            Err(reason) => {
                // 建连失败不绑定：半绑定状态比不绑定更难排查
                self.set_message(Some(format!("连接不可用：{reason}")), cx);
            }
        }
    }
}
