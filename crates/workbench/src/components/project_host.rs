//! 项目视图的宿主桥（M1 / A3）
//!
//! `project` crate 自带项目视图（`project::ui`），但不依赖 workbench 与 settings；
//! 工作台在这里把 `Shared` 的状态句柄、视图重绘、编辑区、排序偏好与打开后刷新
//! 注入为 `ProjectUiHost`。

use std::rc::Rc;

use gpui_kit::*;

use crate::panels::Shared;
use crate::view::WorkbenchView;

/// 重绘桥：把 `WeakEntity<WorkbenchView>` 包成项目视图可调用的 notifier。
///
/// 用弱引用以避免宿主 ↔ 视图的引用环（host 存在 `WorkbenchView` 字段里）。
struct ViewNotifier(WeakEntity<WorkbenchView>);

impl project::ui::ProjectUiNotifier for ViewNotifier {
    fn notify(&self, cx: &mut App) {
        let _ = self.0.update(cx, |_, cx| cx.notify());
    }
}

/// 编辑区桥：把 M1 的未保存草稿拦截接到**编辑器文档**上（B12）
///
/// 旧实现读 `Shared` 的三个镜像字段（`editor_dirty` / `editor_sql` / `editor_clear`）——
/// 那是“编辑区只有一个 SQL 框”时代的产物；现在草稿就是**未命名的编辑器文档**，
/// 真相在 `EditorService` 里，这里只读它（`EditorShared` 是 `Rc` 句柄，不需要 `cx`）。
///
/// 口很窄：有路径的文档是真实文件，项目切换不该碰它们。
struct EditorBridge {
    /// 编辑器文档集合（读脏状态与草稿内容）
    service: editor::shared::EditorShared,
    /// 宿主句柄（只有 `clear` 要它：关面板需要 Dock 与窗口）
    host: WeakEntity<WorkbenchView>,
}

impl EditorBridge {
    /// 未命名且内容非空的文档（= 需要看管的草稿）
    fn drafts(&self) -> Vec<(editor::model::DocumentId, String)> {
        self.service
            .service()
            .documents()
            .iter()
            .filter(|doc| doc.path().is_none() && !doc.content().trim().is_empty())
            .map(|doc| (doc.id().clone(), doc.content().to_string()))
            .collect()
    }
}

impl project::ui::ProjectEditorBridge for EditorBridge {
    fn is_dirty(&self) -> bool {
        !self.drafts().is_empty()
    }

    fn sql(&self) -> String {
        // 优先取**当前活动**草稿；否则任意一份（用户看得到哪份就存哪份）
        let active = self.service.service().active_id().cloned();
        let drafts = self.drafts();
        match active {
            Some(id) => drafts
                .iter()
                .find(|(draft, _)| draft == &id)
                .map(|(_, sql)| sql.clone())
                .or_else(|| drafts.first().map(|(_, sql)| sql.clone()))
                .unwrap_or_default(),
            None => drafts.first().map(|(_, sql)| sql.clone()).unwrap_or_default(),
        }
    }

    fn clear(&self, window: &mut Window, cx: &mut App) {
        // 关掉未命名文档 = 草稿已处理（已另存到项目根 / 用户选择丢弃）
        let _ = self
            .host
            .update(cx, |view, cx| view.close_untitled_editor_documents(window, cx));
    }

    /// `clear` 之后草稿已不存在（未命名文档已关）——不需要另一个“清脏”动作
    fn mark_clean(&self) {}
}

/// 组装项目视图宿主（在 `WorkbenchView::new` 中调用一次）。
pub fn build_host(
    shared: &Shared,
    entity: WeakEntity<WorkbenchView>,
    editor_service: editor::shared::EditorShared,
) -> project::ui::ProjectUiHost {
    let save_sort: Rc<dyn Fn(project::ui::ProjectSort, &mut App)> =
        Rc::new(|sort, cx| settings::SettingsService::set_project_sort_mode(sort.key(), cx));

    let on_opened = {
        let shared = shared.clone();
        let view = entity.clone();
        Rc::new(move |cx: &mut App| {
            refresh_after_open(&shared, cx);
            // M6：资产库列表随项目切换重新取数——面板可能正开着，不重取会一直
            // 显示上一项目的存档（“切了项目但内容没变”是最难发现的一类错）。
            if let Some(view) = view.upgrade() {
                view.update(cx, |this, cx| this.request_resources_refresh(cx));
            }
        })
    };

    project::ui::ProjectUiHost::new(
        shared.project_ui.clone(),
        shared.project.clone(),
        Rc::new(ViewNotifier(entity.clone())),
    )
    .with_editor(Rc::new(EditorBridge {
        service: editor_service,
        host: entity,
    }))
    .with_sort_saver(save_sort)
    .with_on_opened(on_opened)
}

/// 项目切换时的 mock 临时表清理。
///
/// 内存库是**进程级单例**：上一项目生成的 `temp_mock_*` 不切项目就一直在（架构 §9-I1/I2），
/// 既是内存占用（换目标表名就多一张），也让「窗口 = 项目」的隔离打折扣。
/// 这里先取消在跑的任务（避免刚清完又被写回），再删除全部 mock 临时表，
/// 最后让面板作废旧预览（`gen_info` 里的表名已失效）并重读生成历史
/// （历史随项目走，上一个项目的记录不能再摆在这个项目下）。
fn clear_mock_temp_tables(shared: &Shared, cx: &mut App) {
    let panel = shared.mock_panel.borrow().clone();
    let live = || panel.as_ref().and_then(|panel| panel.upgrade());
    if live().is_some_and(|panel| panel.read(cx).is_running()) {
        crate::services::mock_jobs::cancel();
    }
    // **非阻塞**清理：出口任务（落库 / 导出）不可取消且整段持有内存库连接锁，
    // 在 UI 线程上同步等锁 = 界面假死到那个任务结束。拿不到锁就记一笔，
    // 等它收尾那一拍再清（`mock_host::take_job_done`，那时锁已经空了）；
    // 若那个任务迟迟不回来（或面板已被关掉），下一次切项目会再试一遍。
    let cleared = match crate::services::mock_generator::try_clear_temp_tables() {
        Some(cleared) => cleared.len(),
        None => {
            shared.pending_temp_cleanup.set(true);
            0
        }
    };
    if let Some(panel) = live() {
        panel.update(cx, |panel, cx| {
            panel.forget_generated(cleared, cx);
            // 候选清单是「当前项目」的派生视图：旧项目的分析库表名不能继续摆在新项目下
            panel.refresh_sources(cx);
            panel.refresh_history(cx);
        });
    }
}

/// 打开项目后的宿主刷新：连接列表、选中项、导航缓存与结果归属都归零，
/// 避免残留上一项目的数据。
fn refresh_after_open(shared: &Shared, cx: &mut App) {
    // 项目打开/切换后：列表按作用域合一（全局 + 该项目 P_/GP_）。
    let root = shared.project.borrow().as_ref().map(|p| p.root.clone());

    // M8：告知洞察规则监听器当前项目——监听器每轮读这个值现算目录，
    // 从而自动跟随项目切换（否则会一直看着启动时那个项目）。
    insight::set_watched_project_root(root.clone());

    // M7：清掉上一项目的 mock 临时表（进程级内存库不会随项目切换释放）。
    clear_mock_temp_tables(shared, cx);

    let (conns, notice) =
        crate::services::workspace_loader::load_connections_for_scope(root.as_deref());
    *shared.connections.borrow_mut() = conns;
    *shared.notice.borrow_mut() = notice;
    let has = !shared.connections.borrow().is_empty();
    shared.selected.set(if has { Some(0) } else { None });
    shared.invalidate_nav_cache();
}

#[cfg(test)]
mod tests {
    // 显式列举依赖（不要 `use super::*`：父模块的 `use gpui_kit::*` 会跟着进来，
    // `#[gpui_kit::test]` 展开出的裸 `#[test]` 会解析到它自己，无限递归）
    use super::{clear_mock_temp_tables, Shared};
    use gpui_kit::TestAppContext;
    use mock::mock_view::{MockColumnSpec, MockDraft, MockRunOptions};
    use mock::models::{ColumnDataType, ColumnDef, GeneratorConfig, Locale};

    fn draft(table: &str) -> MockDraft {
        MockDraft {
            table_name: table.to_string(),
            columns: vec![MockColumnSpec {
                id: 0,
                def: ColumnDef {
                    name: "id".to_string(),
                    data_type: ColumnDataType::Integer,
                    generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                    nullable_ratio: 0.0,
                    unique: true,
                    dependency: None,
                },
                confidence: "high".to_string(),
                sample_value: String::new(),
            }],
            options: MockRunOptions::new(10, Some(1), Locale::ZhCn),
        }
    }

    /// 项目切换：清掉本进程的 mock 临时表（上一项目的试算结果不该继续占内存）。
    ///
    /// 只断言**自己那张表**：内存库是进程级的，别的 lib 用例可能同时也在建表。
    #[gpui_kit::test]
    fn project_switch_clears_mock_temp_tables(cx: &mut TestAppContext) {
        let shared = Shared::with_connections(Vec::new(), None);
        // 纯生成不碰库（`None`）：临时表落在进程级内存库上，本用例只关心它被清掉
        let info = crate::services::mock_generator::generate_at(None, &draft("t_switch"), None)
            .expect("生成应当成功");
        assert!(
            mock::MockEngine::temp_tables()
                .expect("列临时表")
                .contains(&info.temp_table_name),
            "前置条件：临时表应当已建好"
        );

        cx.update(|cx| clear_mock_temp_tables(&shared, cx));

        assert!(
            !mock::MockEngine::temp_tables()
                .expect("列临时表")
                .contains(&info.temp_table_name),
            "切项目后不应再有上一项目的临时表"
        );
    }
}
