"""一次性：把 `render_scratchpad` 的「草稿树」段抽成私有方法 `render_scratchpad_tree`。

与 `extract_scratchpad_bottom.py` / `extract_scratchpad_sections.py` 同一套做法（**纯位移**）：
段体逐字搬出，用到的局部量在方法开头重新投影；`rows` 按值传进去（父视图后续不再用它），
`loading` / `filter` 是两处标量，直接传参。

计数（文件 / 文件夹）留在父视图：它们是底部状态行的输入，与树段无关。

已执行，勿重复运行（行号写完即失效）。
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "crates/scratchpad/src/scratchpad_view/chrome.rs"

MARKER_AND_BODY = (110, 200)  # 「// ── 草稿树」标记行 .. `panel = panel.child(drafts);`
BODY = (117, 199)  # 段体（内联新建行定位 起 .. 树装配完 止）

CALL = """        // ── 草稿树（面板唯一滚动区）── 本段在 `render_scratchpad_tree` 里。
        let row_count = rows.len();
        let file_count = rows
            .iter()
            .filter(|(_, e)| e.kind == ScratchpadEntryKind::File)
            .count();
        let folder_count = row_count - file_count;
        let drafts = self.render_scratchpad_tree(rows, loading, &filter, window, cx);
        panel = panel.child(drafts);
"""

METHOD_HEAD = '''impl ScratchpadView {
    /// 草稿树：内联新建行的落点、行高表、虚拟列表与空态（面板唯一滚动区）。
    ///
    /// 为什么单独成方法：这是面板的主体，`render_scratchpad` 里它最长的一段脚手架；
    /// 搬出来之后主视图只剩「状态投影 + 装配顺序」（行为不变：同一元素树，只换了落点）。
    fn render_scratchpad_tree(
        &self,
        rows: Vec<(usize, ScratchpadEntry)>,
        loading: bool,
        filter: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let hover_bg = theme.colors.list_hover;
        let selected_bg = theme.colors.list_active;
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let folder_color = theme.colors.warning;
        let primary = theme.colors.primary;
        let info = theme.colors.info;
        let success = theme.colors.success;
        let active_border = theme.colors.list_active_border;

        let entity = cx.entity();
        let row_count = rows.len();
        let (selected, expanded, loaded_children, edit) = {
            let view = self.scratchpad.borrow();
            (
                view.selected.clone(),
                view.expanded.clone(),
                view.children.clone(),
                view.edit.clone(),
            )
        };

'''


def main() -> None:
    lines = SRC.read_text(encoding="utf-8").splitlines(keepends=True)
    m_start, m_end = MARKER_AND_BODY
    b_start, b_end = BODY
    body = "".join(lines[b_start - 1 : b_end])
    if not body.lstrip().startswith("// 内联新建的文件/文件夹行定位"):
        sys.exit("段体起点不对")
    if not body.rstrip().endswith("}"):
        sys.exit("段体终点不对")

    out = (
        "".join(lines[: m_start - 1])
        + CALL
        + "".join(lines[m_end:])
        + "\n"
        + METHOD_HEAD
        + body
        + "\n        drafts\n    }\n}\n"
    )
    SRC.write_text(out, encoding="utf-8", newline="\n")
    print(f"搬出 {b_end - b_start + 1} 行；chrome.rs 现在 {out.count(chr(10))} 行", file=sys.stderr)


if __name__ == "__main__":
    main()
