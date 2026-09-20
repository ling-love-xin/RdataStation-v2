"""一次性：把 `render_scratchpad` 的「外部引用 + 回收站」两块抽成私有方法（**纯位移**）。

`render_scratchpad` 是面板唯一主视图、接近千行；这两块与「数据从哪来」无关，搬出去后
主视图只剩装配顺序。搬出的代码逐字未改，只是在方法开头把用到的局部量重新投影一次
（`cx.theme()` 的颜色、`self.scratchpad` 的状态、`cx.entity()`），调用点换成一次方法调用。

已执行，勿重复运行（行号写完即失效）。
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "crates/scratchpad/src/scratchpad_view/chrome.rs"

# 1-based 行号。
REF_BLOCK = (611, 846)  # 「外部引用」注释行 .. 回收站块收尾的 `}`
CALL_SITE = (608, 609)  # 「底部固定区」注释行 .. `let mut body = div()...`

METHOD = '''impl ScratchpadView {
    /// 底部固定区的「外部引用 + 回收站」两块（不随草稿树滚动）。
    ///
    /// 为什么单独成方法：`render_scratchpad` 是面板唯一主视图，接近千行——把与「数据从哪来」
    /// 无关的两块搬出来，主视图只剩装配顺序（行为不变：同一元素树，只换了落点）。
    fn render_scratchpad_refs_and_trash(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let primary = theme.colors.primary;
        let hover_bg = theme.colors.list_hover;
        let ref_color = theme.colors.info;

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();
        let (external_refs, trash, trash_expanded, edit) = {
            let view = self.scratchpad.borrow();
            (
                view.external_refs.clone(),
                view.trash.clone(),
                view.trash_expanded,
                view.edit.clone(),
            )
        };

        let mut body = div().v_flex().w_full().gap_1().px_1().pb_1();
[EXTRACTED]        body
    }
}

'''

CALL = """        // ── 底部固定区（引用 / 回收站 / 撤销栏 / 状态；不随草稿树滚动）──
        // 引用与回收站两块抽成方法（`render_scratchpad_refs_and_trash`）：主视图只留装配顺序。
        let body = self.render_scratchpad_refs_and_trash(cx);
"""


def main() -> None:
    lines = SRC.read_text(encoding="utf-8").splitlines(keepends=True)
    ref_start, ref_end = REF_BLOCK
    call_start, call_end = CALL_SITE
    extracted = lines[ref_start - 1 : ref_end]
    text = "".join(extracted)
    if "if !external_refs.is_empty()" not in text[:400]:
        sys.exit("抽取起点不对：前几行里没有 `external_refs` 判断")
    if not text.rstrip().endswith("}"):
        sys.exit("抽取终点不对：块尾不是 `}`")

    out = (
        "".join(lines[: call_start - 1])
        + CALL
        + "".join(lines[ref_end:])
        + "\n"
        + METHOD.replace("[EXTRACTED]", text)
    )
    SRC.write_text(out, encoding="utf-8", newline="\n")
    print(
        f"抽出 {len(extracted)} 行；chrome.rs 现在 {out.count(chr(10))} 行",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
