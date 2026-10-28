//! 编辑器宿主面板 —— 环境与脚本（render_env_picker/set_env/env_for/run_script/drain_script_notes）
//!
//! 自 host.rs 拆分（2026-10-29）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为；本文件不得出现新的逻辑。


// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法


use crate::execution::{self, ExecTarget};
// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）
use crate::store::ResultPlacement;


use super::*;
impl EditorHostPanel {

    /// 【2b】通用模式的环境选择器（工具栏右侧）：环境名 + 版本 ▾；未接端口 / 空列表如实置灰。
    pub(crate) fn render_env_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let envs = self.envs();
        let current = self.env_for();
        // 装没装是 EnvPort 的 `installed`（命令可达），在闭包外一次算好（闭包是 `move`）
        let env_installed_states: std::collections::HashMap<String, bool> = envs
            .iter()
            .map(|env| (env.id.clone(), self.shared.env_installed(&env.id)))
            .collect();
        let label = match &current {
            Some(id) => envs
                .iter()
                .find(|env| env.id == *id)
                .map(|env| env.display_name())
                .unwrap_or_else(|| format!("{id}（不在列表）")),
            None => "选择环境 ▾".to_string(),
        };
        Button::new("editor-env-picker")
            .ghost()
            .small()
            .debug_selector(|| "editor-env-picker".to_string())
            .label(label)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for env in &envs {
                    let id = env.id.clone();
                    let entity = entity.clone();
                    let checked = Some(id.as_str()) == current.as_deref();
                    let installed = env_installed_states.get(env.id.as_str()).copied().unwrap_or(false);
                    let mut item = PopupMenuItem::new(env.display_name())
                        .checked(checked)
                        .on_click(move |_, _window, app| {
                            entity.update(app, |panel, cx| {
                                panel.set_env(Some(id.clone()), cx);
                            });
                        });
                    if !installed {
                        item = item.disabled(true);
                    }
                    menu = menu.item(item);
                }
                // 分隔 + 「管理环境…」：列表可能为空（没探测到任何解释器），
                // 入口**不置灰**——这正是新增自定义环境的那条路。
                if !envs.is_empty() {
                    menu = menu.separator();
                }
                let manage_entity = entity.clone();
                let manage = PopupMenuItem::new("管理环境…")
                    .on_click(move |_, window, app| {
                        manage_entity.update(app, |panel, cx| {
                            panel.open_manage_environments(window, cx);
                        });
                    });
                menu = menu.item(manage);
                menu
            })
    }

    /// 【2b】通用模式的运行按钮：未选环境 / 环境未装 / 只读 → 置灰 + 悬停原因。
    pub(crate) fn render_run_script_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let enabled = self
            .env_for()
            .map(|id| self.shared.env_installed(&id) && !self.editor_read_only())
            .unwrap_or(false);
        Button::new("editor-run-script")
            .small()
            .debug_selector(|| "editor-run-script".to_string())
            .label("▶ 运行")
            .when(!enabled, |b| b.disabled(true))
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.run_script(cx));
            })
    }

    /// 【2b】绑定 / 解绑本文档的执行环境（`None` = 未选 → 运行不亮）
    pub(crate) fn set_env(&mut self, env: Option<String>, cx: &mut Context<Self>) {
        self.shared
            .update(|service| service.set_env(&self.document, env));
        cx.notify();
    }

    /// 【2b】本文档选中的执行环境 id（`None` = 未选）
    pub(crate) fn env_for(&self) -> Option<String> {
        self.shared.service().env_for(&self.document)
    }

    /// 【2b】可用环境列表（宿主注入的 EnvPort；未接端口 = 空，如实）
    pub(crate) fn envs(&self) -> Vec<crate::env::Environment> {
        self.shared.envs()
    }

    /// 【2b】打开「管理环境」对话框（环境配置表：增删改路径 / 参数）
    ///
    /// 增删改经端口直呼宿主（全局库 `script_environments` 表）；删除成功后回调
    /// [`Self::delete_env`]：把文档里选中了这条环境的绑定清掉，选择器不会显示一条
    /// 已不存在的 id。
    pub(crate) fn open_manage_environments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(port) = self.shared.env_port() else {
            self.set_message(Some("环境端口未接入".to_string()), cx);
            return;
        };
        let entity = cx.entity();
        crate::view::manage_env::open_manage_environments(window, cx, port, {
            let entity = entity.clone();
            move |id, window, app| {
                entity.update(app, |panel, cx| panel.delete_env(&id, window, cx));
            }
        });
    }

    /// 【2b】删除环境后的收尾：本文档正选中这条 → 解绑（选择器回「选择环境」）
    pub(crate) fn delete_env(&mut self, id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        if self.env_for().as_deref() == Some(id) {
            self.set_env(None, cx);
        }
    }

    /// 【2b】运行脚本：把**当前文档内容**交给选中的环境执行（spawn + stdin + 超时 + 杀进程树
    /// 都在执行器侧）。缺环境 / 未选中 / 没内容 / 只读都如实拒绝，不假装能跑。
    pub(crate) fn run_script(&mut self, cx: &mut Context<Self>) {
        let Some(env_id) = self.env_for() else {
            self.set_message(Some("未选中执行环境——先在工具栏选一个".to_string()), cx);
            return;
        };
        if !self.shared.env_installed(&env_id) {
            self.set_message(Some("所选环境未安装（命令不可达）".to_string()), cx);
            return;
        }
        if self.editor_read_only() {
            self.set_message(Some("文档只读，不能执行脚本".to_string()), cx);
            return;
        }
        let code = self.editor_text(cx);
        if code.trim().is_empty() {
            self.set_message(Some("文档是空的，没有可执行的脚本".to_string()), cx);
            return;
        }
        let target = ExecTarget::Script(crate::env::ScriptRequest { env_id, code });
        match self
            .shared
            .submit(self.document.clone(), &target, ResultPlacement::NewSet, execution::RunOptions::default())
        {
            Ok(()) => {
                if self.pending == 0 {
                    self.running_since = Some(std::time::Instant::now());
                }
                self.pending += 1;
                self.set_message(None, cx);
                self.ensure_exec_pump(cx);
            }
            Err(error) => self.set_message(Some(error.message().to_string()), cx),
        }
    }

    /// 【2b】收脚本执行的结论（主线程轮询取走）：变成一张输出卡片钉在结果区
    pub(crate) fn drain_script_notes(&mut self, cx: &mut Context<Self>) {
        let mut mine_arrived = 0usize;
        for note in self.shared.drain_script_notes() {
            if note.document != self.document {
                continue;
            }
            mine_arrived += 1;
            let env_name = self
                .envs()
                .into_iter()
                .find(|env| env.id == note.env_id)
                .map(|env| env.display_name())
                .unwrap_or_else(|| note.env_id.clone());
            let card = match note.result {
                Ok(outcome) => crate::view::results::script_card::ScriptCard {
                    env_name,
                    result: Ok(crate::view::results::script_card::ScriptOutcomeView {
                        stdout: outcome.stdout.clone(),
                        stderr: outcome.stderr.clone(),
                        exit_code: outcome.exit_code,
                        timed_out: outcome.timed_out,
                        elapsed_ms: self
                            .running_since
                            .map(|s| s.elapsed().as_millis() as u64)
                            .unwrap_or(0),
                    }),
                },
                Err(reason) => crate::view::results::script_card::ScriptCard {
                    env_name,
                    result: Err(reason),
                },
            };
            self.script_card = Some(card);
        }
        // 脚本回执不走 `drain_exec`（它是独立通道），所以 `pending` 在这里按本文档的份数收口
        if mine_arrived > 0 {
            self.pending = self.pending.saturating_sub(mine_arrived);
            if self.pending == 0 {
                self.running_since = None;
            }
            // Dock 化：脚本卡真值写宿主后投影进面板（面板渲染读投影字段；
            // 不投影则面板的 script_card 永远是 None，卡片渲染不出来）
            self.sync_result_view(cx);
        }
        cx.notify();
        cx.emit(EditorHostEvent::StatusChanged);
    }

    // ===== 【2d】分析笔记本：单元动作（状态在 `EditorShared` 的笔记本持有器） =====
}
