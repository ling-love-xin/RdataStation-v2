//! 草稿箱 → 编辑器：**宿主消费分支**的窗口级用例（2026-09-21，USIT 第 1 轮补的账）。
//!
//! 链路：草稿行（双击 / `Enter` / 右键「打开」）→ `ScratchpadHost::open_in_editor`（只给绝对路径）
//! → `Shared::request_open_in_editor`（请求槽）→ `WorkbenchView::render` 消费 → 建/激活编辑面板。
//! 前两跳由 `components::scratchpad_host.rs` 的接口用例钉住；**本用例钉住最后一跳**——
//! 它此前没有自动化，因为要的是**真工作台**，而 `WorkbenchView::new` 带了四个真机副作用
//! （会话解析 / 项目写锁 / 项目名册读盘 / 规则目录监听，见 `view.rs::new_for_test` 的文档）。
//!
//! 走的是 `new_for_test`（临时项目目录 + 空连接列表）：**只有装配档位不同**，
//! 之后的请求槽 / render 消费 / 面板创建与生产完全同一条路。
//!
//! 不覆盖：草稿行上的**双击**本身——`click_count >= 2` 由平台给，headless 造不出来
//! （`scratchpad_view/rows.rs` 的 `on_click` 分支仍属人工看一眼）。
//!
//! 注意：不使用 `use gpui_kit::*` 通配导入（会把 gpui 的 `test` 宏带入作用域，
//! 与 `#[gpui_kit::test]` 冲突）。

use std::path::PathBuf;

use gpui_kit::component::Root;
use gpui_kit::component::dock::Panel as _;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};

use rds_workbench::view::WorkbenchView;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_draft_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// 把工作台装进窗口（根视图包 `Root`：`WorkbenchView::render` 要挂对话框层）。
fn open_workbench(
    project: project::ui::OpenProject,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &mut VisualTestContext) {
    let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| WorkbenchView::new_for_test(Some(project), cx));
        *slot_in.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow().clone().expect("工作台已创建");
    (view, cx)
}

#[gpui_kit::test]
fn an_open_request_lands_in_a_sql_editor_tab(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    // 面板装配会读设置 global（`SettingsService::*`）：注入默认值，不读用户磁盘配置。
    cx.update(|cx| {
        if !cx.has_global::<settings::model::Settings>() {
            cx.set_global(settings::model::Settings::default());
        }
    });

    // 真项目骨架：草稿箱根 = 项目下的可见目录
    let root = temp_dir("open");
    let draft = root.join("scratchpad").join("a.sql");
    std::fs::create_dir_all(draft.parent().expect("父目录")).expect("建草稿目录");
    std::fs::write(&draft, "select 1;\n").expect("写草稿");

    // `OpenProject` 是 non_exhaustive：从构造器建，再补名字（与 `project_session::from_recent_projects` 同口径）
    let mut project = project::ui::OpenProject::from_root(root.clone());
    project.name = "临时项目".to_string();
    let (view, cx) = open_workbench(project, cx);

    // 首帧：完成装配（端口 / 侧栏 / 中央 Dock / 启动那份未命名文档）
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let shared = cx.update(|_, cx| view.read(cx).shared_for_test());
    assert!(
        shared.project_root().is_some(),
        "临时项目要真的进会话（草稿箱根从它来）"
    );
    assert!(
        cx.update(|_, cx| view.read(cx).editor_panels_for_test())
            .len()
            == 1,
        "首帧只应有启动那份未命名文档"
    );

    // 草稿箱发起打开（生产里由 `ScratchpadHost::open_in_editor` 落到同一个槽）
    cx.update(|_, _| shared.request_open_in_editor(draft.clone()));
    // 渲染一帧：宿主 render 消费请求（生产里由宿主重绘桥 defer 到下一帧）
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.update(|_, _| shared.take_open_in_editor()).is_none(),
        "请求应已被消费（取出即清空）"
    );

    let panels = cx.update(|_, cx| view.read(cx).editor_panels_for_test());
    assert_eq!(panels.len(), 2, "草稿应当新开一个编辑面板");
    let opened = cx.update(|_, cx| {
        panels.iter().find_map(|panel| {
            let panel = panel.read(cx);
            (panel.document_path().as_deref() == Some(draft.as_path())).then(|| {
                (
                    panel.tab_name(cx).map(|name| name.to_string()),
                    panel.document_mode(),
                )
            })
        })
    });
    let (title, mode) = opened.expect("要有一个面板指向那份草稿");
    assert_eq!(title.as_deref(), Some("a.sql"), "标签名跟文件名走");
    assert_eq!(
        mode,
        Some(editor::model::EditorMode::Sql),
        "`.sql` 后缀按扩展名判定成 SQL 档（高亮 / 格式化 / 执行都在这档）"
    );

    // 真的进了元素树，而不只是状态里多了一份文档：
    // 工具栏按模式分层，SQL 档才有「格式化」；这条同时证明上面那个 mode 断言不是空转。
    assert!(
        cx.debug_bounds("editor-format").is_some(),
        "SQL 档的编辑面板要真渲染出来（含工具栏）"
    );

    let _ = std::fs::remove_dir_all(&root);
}
