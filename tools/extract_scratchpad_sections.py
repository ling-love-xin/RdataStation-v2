"""一次性：把 `render_scratchpad` 的「面板头 + 工具栏（含冲突条）」与「搜索行」抽成私有方法。

与 `extract_scratchpad_bottom.py` 同一套做法（**纯位移**）：搬出的代码逐字未改，
用到的局部量在方法开头重新投影一次，父函数只留「装配顺序」。

已执行，勿重复运行（行号写完即失效）。
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "crates/scratchpad/src/scratchpad_view/chrome.rs"

SECTIONS = [
    {
        "start": 93,
        "end": 330,
        "first": "// ── 工具栏",
        "method": "render_scratchpad_header",
        "returns": "toolbar",
        "call": "        let toolbar = self.render_scratchpad_header(cx);\n",
        "doc": """    /// 面板头与工具栏：新建 / 新建文件夹 / 导入 / 引用 / 排序 / 刷新 + 选中时的剪贴板行，
    /// 以及 C-4 的冲突条（同一份草稿在编辑器里有未保存修改、磁盘上又被外部改了）。
    ///
    /// 为什么单独成方法：`render_scratchpad` 是面板唯一主视图——把「一行按钮怎么摆」这类
    /// 局部细节搬出来，主视图只剩装配顺序（行为不变：同一元素树，只换了落点）。
""",
        "prologue": """        let theme = cx.theme();
        let hover_bg = theme.colors.list_hover;
        let muted = theme.colors.muted_foreground;
        let warning = theme.colors.warning;
        let danger = theme.colors.danger;

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();
        let (selected, sort, sort_desc, has_clipboard, conflicts) = {
            let view = self.scratchpad.borrow();
            let conflicts: Vec<(String, std::path::PathBuf, bool)> = view
                .conflicts
                .iter()
                .map(|c| (c.relative.clone(), c.absolute.clone(), c.diff.is_some()))
                .collect();
            (
                view.selected.clone(),
                view.sort,
                view.sort_desc,
                view.clipboard.is_some(),
                conflicts,
            )
        };
""",
    },
    {
        "start": 332,
        "end": 469,
        "first": "// ── 搜索",
        "method": "render_scratchpad_search",
        "returns": "search_row",
        "call": "        let search_row = self.render_scratchpad_search(cx);\n",
        "doc": """    /// 搜索行：文件名过滤 / 内容搜索的模式 chip + 输入框 + 内容模式下的 `.*` 与 `Aa`。
    ///
    /// 搜索状态在点击回调里改（`view.search_mode` 等），这里只做投影与渲染。
""",
        "prologue": """        let theme = cx.theme();
        let hover_bg = theme.colors.list_hover;
        let selected_bg = theme.colors.list_active;
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();
""",
    },
]


def main() -> None:
    lines = SRC.read_text(encoding="utf-8").splitlines(keepends=True)
    out: list[str] = []
    methods: list[str] = []
    skip_until = -1

    for ix, line in enumerate(lines, start=1):
        if ix <= skip_until:
            continue
        section = next((s for s in SECTIONS if s["start"] == ix), None)
        if section is None:
            out.append(line)
            continue
        body = lines[section["start"] - 1 : section["end"]]
        text = "".join(body)
        if section["first"] not in text[:200]:
            sys.exit(f"第 {ix} 行不是 {section['method']} 的起点：{text[:80]!r}")
        if not text.rstrip().endswith("}"):
            sys.exit(f"{section['method']} 抽出的结尾不是 }}：{text.rstrip()[-40:]!r}")
        out.append(section["call"])
        skip_until = section["end"]
        methods.append(
            "\nimpl ScratchpadView {\n"
            + section["doc"]
            + f"    fn {section['method']}(&self, cx: &mut Context<Self>) -> Div {{\n"
            + section["prologue"]
            + "\n"
            + text
            + f"\n        {section['returns']}\n    }}\n}}\n"
        )

    SRC.write_text("".join(out) + "".join(methods), encoding="utf-8", newline="\n")
    print(f"抽出 {len(SECTIONS)} 段；chrome.rs 现在 {SRC.read_text(encoding='utf-8').count(chr(10))} 行", file=sys.stderr)


if __name__ == "__main__":
    main()
