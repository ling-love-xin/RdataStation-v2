//! 编辑器宿主面板 —— 格式化/转译/来源选择/编辑器设置（format_document/transpile_document/render_sources_picker/set_channel/set_schema/缩进·换行·编码·不可见字符）
//!
//! 自 host.rs 拆分（2026-10-29）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为；本文件不得出现新的逻辑。


use gpui_kit::component::input::{
    RangeDecoration,
    RangeDecorationStyle,
};
// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法


use crate::channel::ExecChannel;
use crate::format;
use crate::sources;
// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）
use crate::translate;


use super::*;
impl EditorHostPanel {

    /// 【B10】格式化整篇 / 选区（`Ctrl+Shift+F`、工具栏「⇤ 格式化」）
    ///
    /// 计划（格式化哪一段 / 光标去哪 / 有几条没动）由纯函数 [`crate::format::plan`] 算；
    /// 这里只做三件事：**只读拒绝**、落一次**可撤销**的替换（`replace_all` 保留撤销历史）、
    /// 把结果如实说给用户（“改了 N 条” / “M 条解析不了，原样保留”）。
    /// 【格式化增强】从设置解析格式化选项（关键字大小写 + 缩进宽度）。
    ///
    /// 设置异常（未知档位 / 越界）回退 sqlglot 原生（preserve / 2），不猜。
    /// `cx` 借了 `Context`（Deref 到 `App`），读设置是只读，放这里安全。
    fn format_options(cx: &mut Context<Self>) -> engine::sql::FormatOptions {
        let editor = editor_settings_or_default(cx);
        let keyword_case = match editor.format_keyword_case.as_str() {
            "upper" => engine::sql::KeywordCase::Uppercase,
            "lower" => engine::sql::KeywordCase::Lowercase,
            _ => engine::sql::KeywordCase::Preserve,
        };
        let indent_width = editor.format_indent_width.clamp(2, 8) as u8;
        engine::sql::FormatOptions {
            keyword_case,
            indent_width,
        }
    }

    /// 【格式化增强】保存时自动格式化是否开启（设置在设置页；测试环境回退默认关）。
    pub(crate) fn format_on_save_enabled(&self, cx: &Context<Self>) -> bool {
        editor_settings_or_default(cx).format_on_save
    }

    pub(crate) fn format_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor_read_only() {
            self.set_message(Some("只读文档不能格式化".to_string()), cx);
            return;
        }
        if !self.execution_allowed() {
            // 文本模式是“记事本”：不解析、不上色，自然也不格式化（能力表说了算）
            self.set_message(Some("文本模式不解析 SQL".to_string()), cx);
            return;
        }
        let (text, selection) = self.editor_snapshot(cx);
        let dialect = format::dialect_of(&self.connection_db_type());
        let options = Self::format_options(cx);
        let plan = format::plan(&text, selection, dialect, options);

        if !plan.changes(&text) {
            // 没改动也要说清楚是“本来就是格式化好的”还是“解析不了”——两者感受完全不同
            let message = if plan.kept_verbatim > 0 {
                format!(
                    "{} 条语句解析不了（可能是还没写完），已原样保留；其余本来就是格式化好的",
                    plan.kept_verbatim
                )
            } else {
                "已经是格式化后的样子".to_string()
            };
            self.set_message(Some(message), cx);
            return;
        }

        self.apply_rewrite(
            plan.text.clone(),
            plan.selection.clone(),
            plan.cursor,
            window,
            cx,
        );
        // 内核的 Change 事件会把内容写回服务层（脏状态跟着变），这里只补一句结果
        let mut message = format!("已格式化 {} 条语句", plan.formatted);
        if plan.kept_verbatim > 0 {
            message.push_str(&format!("；{} 条解析不了，原样保留", plan.kept_verbatim));
        }
        self.set_message(Some(message), cx);
    }

    /// 【B10】方言转译整篇 / 选区（工具栏「⋯ 更多 ▾ ▸ 转译为」）
    ///
    /// 与格式化同一条路（计划在 [`crate::translate::plan`]，落地在 [`Self::apply_rewrite`]），
    /// 但**源方言是硬前提**：转译的方向错不得，所以未绑定连接 / 驱动认不出时直接拒绝并说明，
    /// 不拿起 Ansi 乱翻（那会把 `\``a\`` 与 `# 注释` 静默改成别的意思）。
    pub(crate) fn transpile_document(
        &mut self,
        target: engine::sql::SqlDialect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_read_only() {
            self.set_message(Some("只读文档不能转译".to_string()), cx);
            return;
        }
        if !self.execution_allowed() {
            self.set_message(Some("文本模式不解析 SQL".to_string()), cx);
            return;
        }
        let source = match self.source_dialect() {
            Ok(source) => source,
            Err(reason) => {
                self.set_message(Some(reason), cx);
                return;
            }
        };
        if source == target {
            self.set_message(Some("目标方言与源相同，没什么可转译的".to_string()), cx);
            return;
        }

        let label = translate::label_of(target);
        let (text, selection) = self.editor_snapshot(cx);
        let plan = translate::plan(&text, selection, source, target);
        if !plan.changes(&text) {
            let message = if plan.kept_verbatim > 0 {
                format!(
                    "{} 条语句转译不了（可能是还没写完），已原样保留",
                    plan.kept_verbatim
                )
            } else {
                format!("已经是 {label} 能直接用的写法")
            };
            self.set_message(Some(message), cx);
            return;
        }

        self.apply_rewrite(
            plan.text.clone(),
            plan.selection.clone(),
            plan.cursor,
            window,
            cx,
        );
        let mut message = format!("已转译为 {label}（{} 条语句）", plan.transpiled);
        if plan.kept_verbatim > 0 {
            message.push_str(&format!("；{} 条解析不了，原样保留", plan.kept_verbatim));
        }
        self.set_message(Some(message), cx);
    }

    /// 把一次“整篇 / 选区改写”的结果落到内核（格式化与转译共用）
    ///
    /// - 用 `replace_all`（进撤销栈）——`set_value` 会清空撤销历史，绝不能用于这两个动作；
    /// - 落完把选区 / 光标恢复回去：用户按的是“排一下 / 翻一下”，不是“跳回文首”。
    fn apply_rewrite(
        &mut self,
        new_text: String,
        selection: Option<std::ops::Range<usize>>,
        cursor: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |state, cx| {
            state.replace_all(new_text, window, cx);
            match selection.clone() {
                Some(range) => state.set_selected_range(range, cx),
                None => {
                    // 光标按“第几条语句里的第几个字节”映射过去，并滑到可见区
                    use gpui_kit::component::input::RopeExt as _;
                    let position = state.text().offset_to_position(cursor);
                    state.set_cursor_position(position, window, cx);
                    state.set_selected_range(cursor..cursor, cx);
                }
            }
        });
    }

    /// 本文档的**源方言**（转译用）
    ///
    /// 未绑定连接 / 驱动认不出 → `Err(原因)`：转译的方向错不得，宁可不做也不猜。
    pub(crate) fn source_dialect(&self) -> Result<engine::sql::SqlDialect, String> {
        let db_type = self.connection_db_type();
        if db_type.trim().is_empty() {
            return Err("未绑定连接：转译要知道源方言（先选一个连接）".to_string());
        }
        format::dialect_of_known(&db_type)
            .ok_or_else(|| format!("认不出驱动「{db_type}」的方言，无法确定转译方向"))
    }

    /// 本文档绑定连接的驱动类型（`None` / 未绑定 → 空串 → 方言用 Ansi）
    ///
    /// 方言只能从**连接**来（同一个连接两种引擎时不能猜）；编辑器不知道驱动，
    /// 是宿主填在 `ConnectionOption.db_type` 里的。
    fn connection_db_type(&self) -> String {
        let bound = self
            .with_document(|doc| doc.connection().map(str::to_string))
            .flatten();
        bound
            .and_then(|id| {
                self.shared
                    .connection_options()
                    .into_iter()
                    .find(|option| option.id == id)
                    .map(|option| option.db_type)
            })
            .unwrap_or_default()
    }

    /// 本文档绑定连接的驱动类型（供测试断言方言选择）
    pub fn connection_db_type_for_test(&self) -> String {
        self.connection_db_type()
    }

    /// 【T1.6】源清单选择器（工具栏右侧，**只在联邦档出现**）
    ///
    /// 内容是 [`crate::sources::menu_entries`] 算的（纯函数）：汇总 + 逐行源（失败行带原话）
    /// + 动作（重挂全部 / 设为主源 / 逐源重挂）。快照是**宿主注入的内存读**；动作走旁路
    /// 线程（`SourceAction`），回执回到状态栏。
    pub(crate) fn render_sources_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let snapshot = self.sources_snapshot();
        let entries = sources::menu_entries(snapshot.as_ref());
        let label = sources::button_label(snapshot.as_ref());

        Button::new("editor-sources")
            .ghost()
            .small()
            .debug_selector(|| "editor-sources".to_string())
            .label(label)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for entry in entries.iter() {
                    let entity = entity.clone();
                    menu = match entry {
                        // 信息行与源行都是“看”的：不可点（禁用态在菜单里就是灰字）
                        sources::SourceMenuEntry::Info(text) => {
                            menu.item(PopupMenuItem::new(text.clone()).disabled(true))
                        }
                        sources::SourceMenuEntry::Source(row) => {
                            menu.item(PopupMenuItem::new(row.text()).disabled(true))
                        }
                        sources::SourceMenuEntry::Action { label, action } => {
                            let action = action.clone();
                            menu.item(PopupMenuItem::new(label.clone()).on_click(
                                move |_, _window, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.run_source_action(action.clone(), cx);
                                    });
                                },
                            ))
                        }
                    };
                }
                menu
            })
    }

    /// 这个文档的联邦源清单快照（**渲染路径可调**：宿主给的是内存读）
    pub fn sources_snapshot(&self) -> Option<sources::SourcesSnapshot> {
        let conn_id = self.bound_connection()?;
        self.shared.sources_snapshot(&conn_id)
    }

    /// 【T1.6】源清单里的动作（重挂 / 换主源）
    ///
    /// 与「重新挂载源库」同一条路：旁路线程 + 回执（不占执行位）。没绑连接时直说原因。
    pub(crate) fn run_source_action(
        &mut self,
        action: crate::execution::SourceAction,
        cx: &mut Context<Self>,
    ) {
        if !self.channel().runs_locally() {
            self.set_message(
                Some("现在不是本地跑的那两档，源清单动作没意义".to_string()),
                cx,
            );
            return;
        }
        let channel = self.channel();
        let action_label = action.label();
        match self.shared.request_source_action(
            self.document.clone(),
            self.bound_connection(),
            channel,
            action,
        ) {
            Ok(()) => {
                self.refresh_pending += 1;
                self.set_message(Some(format!("正在{action_label}…")), cx);
                self.ensure_exec_pump(cx);
            }
            Err(reason) => self.set_message(Some(reason), cx),
        }
    }

    /// 【B13】重新挂载源（表清单刷新）
    ///
    /// 加速档重挂那一条源；联邦档逐源全挂（源清单里的是行级动作）。动作在旁路线程上做，
    /// 回执由轮询泵取回。
    pub(crate) fn refresh_source(&mut self, cx: &mut Context<Self>) {
        if !self.channel().runs_locally() {
            self.set_message(
                Some("现在不是本地加速档，没有可重新挂载的源".to_string()),
                cx,
            );
            return;
        }
        self.run_source_action(crate::execution::SourceAction::RefreshAll, cx);
    }

    /// 【B13】还没回执的重新挂载数（供测试断言）
    pub fn refresh_pending_for_test(&self) -> usize {
        self.refresh_pending
    }

    /// 【B7 切片二】还没回执的 DuckDB 导出数（供测试断言）
    pub fn export_pending_for_test(&self) -> usize {
        self.export_pending
    }

    /// 【B13】切换执行通道（「执行位置 ▾」调用）
    ///
    /// 门控是**第二道闸**（菜单已经置灰了）：不可用的通道就算被程序叫到也不给切——
    /// 否则界面会停在一个“看着能用、一按就错”的状态。切完旧结果**保留**（原型 §5.7 规则 2：
    /// 不删用户的东西，只标灰 + 顶部一行说清它来自哪档）。
    pub(crate) fn set_channel(&mut self, channel: ExecChannel, cx: &mut Context<Self>) {
        if channel != ExecChannel::Source {
            let availability = self
                .shared
                .channel_availability(self.bound_connection().as_deref());
            let gate = availability.for_channel(channel);
            if !gate.available {
                let reason = gate.reason.unwrap_or_else(|| "不可用".to_string());
                self.set_message(Some(format!("{}不可用：{reason}", channel.label())), cx);
                return;
            }
        }
        let changed = self
            .shared
            .update(|service| service.set_channel(&self.document, channel));
        // 结果区的投影跟着换（旧结果标灰与顶部提示都在那边算）；它会重写状态栏提示，
        // 所以“已切到 X”这句必须放在它**之后**说
        self.sync_result_view(cx);
        if !changed {
            return; // 本来就在这档：不啥都留痕（切了才留）
        }
        let mut message = format!("执行位置已切到{}", channel.label());
        if channel.runs_locally() {
            message.push_str("（源库以只读方式挂载；作用源库的写语句会被拒）");
        }
        if self.tx_open && !channel.allows_transactions() {
            message.push_str("；源库上还有未提交的事务（切回源库可见）");
        }
        self.set_message(Some(message), cx);
    }

    /// 【T2c-1】切换文档绑定的 schema（`None` = 用连接默认 schema）
    pub(crate) fn set_schema(&mut self, schema: Option<String>, cx: &mut Context<Self>) {
        let changed = self
            .shared
            .update(|service| service.set_schema(&self.document, schema));
        if changed {
            cx.notify();
            cx.emit(EditorHostEvent::StatusChanged);
        }
    }

    /// 【T2c-4】重设文档缩进宽度：文档级状态 + 内核 `set_tab_size` 双写（Tab 键缩进真变）
    pub(crate) fn set_tab_size(&mut self, size: usize, cx: &mut Context<Self>) {
        let changed = self
            .shared
            .set_document_tab_size(&self.document, size);
        if changed {
            // 内核 TabSize 随文档状态走（0.7.1 `set_tab_size` 公开在 EditorState 别名上）
            self.editor
                .update(cx, |state, cx| {
                    state.set_tab_size(
                        gpui_kit::component::input::TabSize {
                            tab_size: size,
                            hard_tabs: false,
                        },
                        cx,
                    );
                });
            cx.notify();
        }
    }

    /// 【T2c-4】重设缩进用制表符还是空格：文档状态 + 内核 TabSize 双写
    pub(crate) fn set_hard_tabs(&mut self, hard: bool, cx: &mut Context<Self>) {
        let changed = self
            .shared
            .set_document_hard_tabs(&self.document, hard);
        if changed {
            let size = self.shared.document_tab_size(&self.document);
            self.editor
                .update(cx, |state, cx| {
                    state.set_tab_size(
                        gpui_kit::component::input::TabSize {
                            tab_size: size,
                            hard_tabs: hard,
                        },
                        cx,
                    );
                });
            cx.notify();
        }
    }

    /// 【2026-10-27】从内容检测缩进方式（VSCode「从内容中检测缩进方式」）：
    /// 只推断 制表符/空格宽度并写 `tab_size` + `hard_tabs`，**不置脏**（不触碰正文）。
    pub(crate) fn detect_indent_from_content(&mut self, cx: &mut Context<Self>) {
        let text = self.editor_text(cx);
        let (hard, size) = crate::indent::detect_indent(&text);
        self.set_hard_tabs(hard, cx);
        self.set_tab_size(size.max(1), cx);
        self.set_message(
            Some(format!(
                "已按内容检测缩进：{}（宽度 {}）",
                if hard { "制表符" } else { "空格" },
                size.max(1)
            )),
            cx,
        );
    }

    /// 【2026-10-27】将缩进转换为空格 / 制表符（VSCode「将缩进转换为…」）：读全文 → 重写
    /// 前导缩进 → 写回文档 + 内核重载。**会标脏**（正文变了），符合 VSCode 行为。
    pub(crate) fn convert_indent(
        &mut self,
        to_hard_tabs: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.editor_text(cx);
        let size = self.shared.document_tab_size(&self.document).max(1);
        let converted = crate::indent::convert_indent(&text, to_hard_tabs, size);
        if converted != text {
            self.shared
                .update(|service| service.set_content(&self.document, converted));
            self.reload_from_document(window, cx);
            self.set_message(
                Some(format!(
                    "缩进已转换为{}（宽度 {}）",
                    if to_hard_tabs { "制表符" } else { "空格" },
                    size
                )),
                cx,
            );
        }
    }

    /// 【2026-10-27】裁剪尾随空格（VSCode「裁剪尾随空格」）：去每行行尾空白，写回 + 内核重载。
    pub(crate) fn trim_trailing_whitespace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor_text(cx);
        let trimmed = crate::indent::trim_trailing_whitespace(&text);
        if trimmed != text {
            self.shared
                .update(|service| service.set_content(&self.document, trimmed));
            self.reload_from_document(window, cx);
            self.set_message(Some("已裁剪尾随空格".to_string()), cx);
        }
    }

    /// 【2c】重设文档保存编码（UTF-8 / UTF-8 (BOM) / GBK / UTF-16 LE / UTF-16 BE）
    ///
    /// 只改**保存契约**：内核文本永远是 Unicode，切换编码不会实时重新解码（无从重解码），
    /// 只让下次保存按新编码产出字节。`is_dirty` 由 `format != saved_format` 得出，切换即标脏。
    pub(crate) fn set_encoding(
        &mut self,
        encoding: crate::encoding::TextEncoding,
        cx: &mut Context<Self>,
    ) {
        let current = self.shared.document_save_format(&self.document);
        if current.encoding == encoding {
            return;
        }
        let changed = self
            .shared
            .set_document_save_format(&self.document, current.with_encoding(encoding));
        if changed {
            cx.notify();
        }
    }

    /// 【2c】重设文档保存换行符（LF / CRLF）
    ///
    /// 同上：只改保存契约；load 时 CRLF 原样进内核（编辑语义不变），保存时才按这里
    /// 的 eol 统一生成。**切换不标脏**（2026-10-27：eol 是显示/保存契约级设置，不影响文本，
    /// 脏判据只比较编码——见 `service::is_dirty`）。
    pub(crate) fn set_eol(&mut self, eol: crate::encoding::LineEnding, cx: &mut Context<Self>) {
        let current = self.shared.document_save_format(&self.document);
        if current.eol == eol {
            return;
        }
        let changed = self
            .shared
            .set_document_save_format(&self.document, current.with_eol(eol));
        if changed {
            cx.notify();
        }
    }

    /// 【2c】显示空白字符（空格 `•` / 制表符 `→`）：文档状态 + 内核 `set_show_whitespaces` 双写
    pub(crate) fn set_show_whitespaces(
        &mut self,
        show: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self
            .shared
            .set_document_show_whitespaces(&self.document, show);
        if changed {
            self.editor.update(cx, |state, cx| {
                state.set_show_whitespaces(show, window, cx);
            });
            cx.notify();
        }
    }

    /// 【2c】显示隐藏字符标注（**按类别**）：文档位掩码 + 内核 RangeDecoration 双写
    ///
    /// 启用时懒创建标注集合（`.set` 会把当前正文里所有**启用类别**的隐藏字符刷一遍）；
    /// 全关时清空集合。双栏面板每勾一类调用一次；¶ 总开关走 [`Self::set_show_invisible`]。
    pub(crate) fn set_invisible_categories(&mut self, mask: u8, cx: &mut Context<Self>) {
        let changed = self.shared.set_document_invisible_mask(&self.document, mask);
        if !changed {
            return;
        }
        if mask != 0 {
            if self.invisible_decorations.is_none() {
                // 懒创建：句柄借内核实体，面板 drop 随内核一起释放
                self.invisible_decorations = Some(self.editor.update(cx, |state, cx| {
                    state.create_range_decorations_collection(Vec::new(), cx)
                }));
            }
            let ranges = self.invisible_ranges_for(cx);
            self.refresh_invisible_decorations(ranges, cx);
        } else if let Some(collection) = &self.invisible_decorations {
            collection.clear(cx);
        }
        cx.notify();
    }

    /// 【2c】旧 bool 总开关：全开（所有类别） / 全关
    pub(crate) fn set_show_invisible(&mut self, show: bool, cx: &mut Context<Self>) {
        self.set_invisible_categories(if show { crate::hidden::FULL_MASK } else { 0 }, cx);
    }

    /// 当前文档启用类别的隐藏字符区间（双栏面板 / 内容变化重算共用同一口径）
    pub(crate) fn invisible_ranges_for(&self, cx: &mut Context<Self>) -> Vec<std::ops::Range<usize>> {
        let mask = self.shared.document_invisible_mask(&self.document);
        let enabled: Vec<_> = crate::hidden::all_categories()
            .into_iter()
            .filter(|c| mask & crate::hidden::category_bit(*c) != 0)
            .collect();
        crate::hidden::hidden_ranges_filtered(&self.editor_text(cx), &enabled)
    }

    /// 【2c】把当前隐藏字符区间刷进标注集合（`None` = 没开显示，什么都不做）
    pub(crate) fn refresh_invisible_decorations(
        &mut self,
        ranges: Vec<std::ops::Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let Some(collection) = &self.invisible_decorations else {
            return;
        };
        let color = cx.theme().colors.warning.opacity(0.3);
        let decorations = ranges
            .into_iter()
            .map(|range| {
                RangeDecoration::new(range)
                    .with_style(RangeDecorationStyle::Fill)
                    .with_color(color)
            })
            .collect::<Vec<_>>();
        collection.set(decorations, cx);
    }
}
