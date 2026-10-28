//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。


use gpui_kit::{
    TestAppContext,
};

use crate::model::EditorMode;
use crate::shared::EditorShared;

use super::{
    open_panel,
    shared_with_document,
};

// ===== 2c：编码 / 换行 / 不可见字符显示（接线层）=====
//
// 纪律：直调面板方法（`set_encoding` / `set_eol` / `set_show_whitespaces` / `set_show_invisible`）
// 断言共享状态与标脏；菜单弹层（重设 ▾）的勾选/点击由状态栏单测 + 人工验收覆盖。

/// 【2c】面板方法切编码 / 换行：文档级保存格式跟着变，且**切换即标脏**（会产出不同字节）
#[gpui_kit::test]
fn panel_encoding_and_eol_switch_marks_dirty(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\encoding.sql", "select 1");
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 默认 UTF-8 + LF，不脏
    let default = crate::encoding::SaveFormat::default();
    assert_eq!(shared.document_save_format(&id), default);
    assert!(!cx.update(|_window, cx| panel.read(cx).is_dirty()));

    // 切编码 → 格式变 + 标脏；同值重设 = 无操作
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_encoding(crate::encoding::TextEncoding::Gbk, cx)
        })
    });
    let format = shared.document_save_format(&id);
    assert_eq!(format.encoding, crate::encoding::TextEncoding::Gbk, "编码要落到文档级保存格式");
    assert_eq!(format.eol, crate::encoding::LineEnding::Lf, "换行不受编码切换影响");
    assert!(cx.update(|_window, cx| panel.read(cx).is_dirty()), "切编码 = 未保存的改动");
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_encoding(crate::encoding::TextEncoding::Gbk, cx)
        })
    });
    assert!(cx.update(|_window, cx| panel.read(cx).is_dirty()), "同值重设不应清脏");

    // 切换行 → 格式变 + 仍脏；LF 同值重设无操作
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_eol(crate::encoding::LineEnding::Crlf, cx)
        })
    });
    let format = shared.document_save_format(&id);
    assert_eq!(format.eol, crate::encoding::LineEnding::Crlf, "换行要落到文档级保存格式");
    assert_eq!(format.encoding, crate::encoding::TextEncoding::Gbk, "编码不受换行切换影响");
    assert!(cx.update(|_window, cx| panel.read(cx).is_dirty()));
}

/// 【2c】GBK 文件：打开按 BOM/UTF-8/GBK 判定链解码 → 保存按文档保存格式原样回写（字节一致）
#[gpui_kit::test]
fn gbk_file_roundtrips_through_open_and_save(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = std::env::temp_dir().join(format!("rds_editor_gbk_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("gbk.sql");

    // GBK 编码的「中文」+ ASCII 前缀（`中` = D6 D0，`文` = CE C4）
    let mut bytes = b"select ".to_vec();
    bytes.extend_from_slice(&[0xD6, 0xD0, 0xCE, 0xC4]);
    std::fs::write(&path, &bytes).expect("写盘");

    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Sql)
        .expect("打开")
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 打开即判出 GBK（判定链：BOM 无 → UTF-8 严格失败 → GBK 兜底）
    assert_eq!(
        shared.document_save_format(&id).encoding,
        crate::encoding::TextEncoding::Gbk,
        "无 BOM 的 GBK 字节要判出 GBK"
    );
    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert_eq!(text, "select 中文", "GBK 字节要解出正确 Unicode 正文");

    // 面板方法保存：按文档保存格式（GBK）写回，字节与源文件一致
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            assert!(panel.save(cx).is_ok(), "有路径的文档保存应成功");
        });
    });
    assert_eq!(
        std::fs::read(&path).expect("读回"),
        bytes,
        "GBK 文件保存后字节必须原样（编码是保存契约）"
    );
    assert!(!cx.update(|_window, cx| panel.read(cx).is_dirty()), "保存后清脏");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 【2c】显示开关：空白字符（双写内核，需 window）与隐藏字符（懒建标注集合）都能切，
/// 且渲染不炸（标注集合的 `.set` / `.clear` 对内核实体是安全的）
#[gpui_kit::test]
fn show_whitespaces_and_invisible_toggle_through_panel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\invisible.sql", "select 1\u{200B}x;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(!shared.document_show_whitespaces(&id));
    assert!(!shared.document_show_invisible(&id));

    // 开空白字符（内核双写：`set_show_whitespaces` 需要 window，从测试的 window 闭包拿）
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_show_whitespaces(true, window, cx)
        })
    });
    assert!(shared.document_show_whitespaces(&id));

    // 开隐藏字符：懒建标注集合并刷当前正文（正文里有一个零宽空格，能算出区间）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_show_invisible(true, cx))
    });
    assert!(shared.document_show_invisible(&id));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 再画一帧 / 关掉：清空集合，状态回落，渲染仍稳
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_show_invisible(false, cx))
    });
    assert!(!shared.document_show_invisible(&id));
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

