"""nav_view.rs 拆分 · 阶段 1：把纯函数 / 拖拽类型 / 测试模块搬进 `nav_view/` 子模块。

为什么这样切（而不是一次切 7 个文件）：
- **保留 `nav_view.rs` 作为模块根**（Rust 2018：`foo.rs` + `foo/` 目录并存），
  文档里 90 余处 `crates/database/src/nav_view.rs` 路径引用继续有效；
- 阶段 1 只搬**跨模块表面是函数/方法**的东西（搬出去的加 `pub(super)` 即可），
  **状态类型留在根**——子模块能看见祖先模块的私有项，所以零字段可见性改动；
- 阶段 2 再切 `impl NavView`（那一步只需要给方法加 `pub(super)`）。

解析手法：按「顶层候选行」（列 0 的 `#[` / `///` / 项关键字）切块，
每个块 = 文档注释 + 属性 + 项 + 主体，比数花括号稳。

用法：python tools/split_nav_view.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

SRC = Path("crates/database/src/nav_view.rs")
DIR = Path("crates/database/src/nav_view")

ITEM_START = re.compile(
    r"^(pub\(crate\)\s+|pub\(super\)\s+|pub\s+)?(struct|enum|impl|fn|mod|const|type|trait|static)\b"
)
CAND = re.compile(r"^(#\[|///|//!|pub |fn |struct |enum |impl |mod |const |type |trait |static )")

# 搬进 primitives.rs 的纯函数（名字白名单；跨模块只暴露函数，不搬状态类型）
PRIMITIVE_FNS = {
    "nav_badge_hover_card",
    "nav_type_label",
    "nav_type_category_label",
    "nav_type_short_label",
    "nav_type_badge",
    "nav_icon",
    "nav_kind_icon",
    "nav_disclosure",
    "nav_active_bar",
    "nav_qualified_name",
    "insight_schema_target",
    "nav_data_target",
    "nav_reorder",
    "nav_primary_scope",
    "nav_step",
    "nav_order_members",
    "nav_node_matches",
    "nav_search_hit_ref",
    "nav_search_hit_property",
    "nav_search_hit_kind",
    "nav_object_type_label",
    "nav_search_query_ready",
    "nav_relative_time",
    "nav_merge_page",
    "nav_name_highlight",
    "nav_subline_at",
    "nav_subline",
    "nav_row_height",
    "parse_nav_search",
}

# 搬进 dnd.rs 的拖拽载荷 / 落点 / 幽灵
DND_ITEMS = {
    "NavDragPayload",
    "NavDragGhost",
    "NavConnDragPayload",
    "NavGroupDragPayload",
    "ConnDropTarget",
}

MOD_HEADERS = {
    "primitives": (
        "//! 导航面板的纯函数与视觉原语（无 `self`、无 I/O、可单测）。\n"
        "//!\n"
        "//! 为什么要独立成模块：类型徽标映射 / 类别图标 / 展开指示 / 激活条 / 相对时间 /\n"
        "//! 命中高亮这些都是**纯计算**，与面板状态无关；它们此前混在 7800 行的单一文件里，\n"
        "//! 改一处要在一屏里找半天。搬出后根文件只留「状态 + 协议 + 导航锚点」。\n"
        "//!\n"
        "//! 可见性：搬出的项加 `pub(super)`（= 在 `crate::nav_view` 及其子孙模块可见），\n"
        "//! 根模块用 `use self::primitives::*;` 收回来，**调用点一字未改**。"
    ),
    "dnd": (
        "//! 导航面板的拖拽载荷、落点与拖拽幽灵。\n"
        "//!\n"
        "//! 载荷是**跨 crate 的公开类型**（编辑区要认 `NavDragPayload` 才能接住表 / 视图拖拽），\n"
        "//! 所以在根模块 `pub use` 回去，外部路径 `database::nav_view::NavDragPayload` 不变。"
    ),
}

ROOT_TAIL = """
// ===== 子模块（拆分见 `tools/split_nav_view.py` + `tools/split_nav_view_impl.py`） =====
//
// 为什么保留 `nav_view.rs` 这个文件名并另建同名目录：文档与 skills 里有 90 余处
// `crates/database/src/nav_view.rs` 与函数级引用，改文件名等于同步改几十份文档。
// Rust 2018 允许 `foo.rs` + `foo/` 并存，于是**路径稳定**、拆分零外部改动。
// 两个脚本是**一次性迁移工具**（已执行；保留作追溯，勿重复运行）。

mod dnd;
mod primitives;

pub use dnd::{NavConnDragPayload, NavDragPayload, NavGroupDragPayload};
use dnd::*;
use primitives::*;
"""


def indent_visibility(chunk: str) -> str:
    """搬出模块的项加 `pub(super)`（自由函数 / 固有方法 / 类型声明 / **结构体字段**）。

    为何一律加：子模块能看见祖先的私有项，但**根**与**兄弟模块**看不见子模块的私有项。
    过标无害，漏标就是编译错。

    三个必须避开的坑（前两版都踩了）：
    1. 字段规则只能作用于**结构体体内**——否则会把多行函数签名的参数
       （`    name_of: impl Fn(&str) -> String,`）误标成参数可见性，签名直接被改坏；
    2. 前缀要插在**缩进之后**（`    pub(super) fn`），不是行首；
    3. **trait impl 的方法不能标可见性**（`impl Render for X { fn render }`），
       E0449；只有固有 impl（`impl NavView {`，即行内无 ` for `）才标。
    """
    out: list[str] = []
    in_struct = False
    struct_depth = 0
    impl_kind: str | None = None
    impl_depth = 0
    for ln in chunk.split("\n"):
        if re.match(r"^\s*(//|#)", ln):
            out.append(ln)
            continue

        impl_decl = re.match(r"^unsafe\s+impl\b|^impl\b", ln)
        if impl_decl:
            impl_kind = "trait" if " for " in ln else "inherent"
            impl_depth = ln.count("{") - ln.count("}")
            if impl_depth <= 0:
                impl_kind = None
            out.append(ln)
            continue

        if impl_kind is not None:
            fn_decl = re.match(r"^(pub\(crate\)\s+|pub\(super\)\s+|pub\s+)?fn\s", ln.strip())
            if fn_decl and re.match(r"^    fn\s", ln) and impl_kind == "inherent":
                ln = "    pub(super) " + ln.strip()
            impl_depth += ln.count("{") - ln.count("}")
            if impl_depth <= 0:
                impl_kind = None
            out.append(ln)
            continue

        decl = re.match(
            r"^(pub\(crate\)\s+|pub\(super\)\s+|pub\s+)?(struct|enum|const|type|static|union)\b", ln
        )
        if decl:
            if decl.group(1) is None:
                ln = "pub(super) " + ln
            if decl.group(2) == "struct":
                in_struct = True
                struct_depth = ln.count("{") - ln.count("}")
            out.append(ln)
            continue

        fn_decl = re.match(r"^(pub\(crate\)\s+|pub\(super\)\s+|pub\s+)?fn\s", ln)
        if fn_decl:
            out.append(ln if fn_decl.group(1) else "pub(super) " + ln)
            continue

        if in_struct:
            if re.match(r"^    \w+\s*:\s", ln) and not re.match(r"^    pub", ln):
                ln = "    pub(super) " + ln.strip()
            struct_depth += ln.count("{") - ln.count("}")
            if struct_depth <= 0:
                in_struct = False
        out.append(ln)
    return "\n".join(out)


def chunks(lines: list[str]) -> list[tuple[int, int, str, str]]:
    """→ [(起, 止, 类型, 名字)]，行号 1-based 闭区间；块含其文档注释与属性。"""
    starts: list[int] = []
    for i, ln in enumerate(lines):
        if CAND.match(ln):
            starts.append(i)
    # 把连续的候选行并成一块（文档 + 属性 + 项）
    bounds: list[int] = []
    for ix, s in enumerate(starts):
        if ix == 0 or s != starts[ix - 1] + 1:
            bounds.append(s)
    bounds.append(len(lines))
    out = []
    for k in range(len(bounds) - 1):
        a, b = bounds[k], bounds[k + 1] - 1
        name = None
        kind = None
        for i in range(a, min(b + 1, len(lines))):
            m = ITEM_START.match(lines[i])
            if m:
                kind = m.group(2)
                rest = lines[i][m.end():].strip()
                name = rest.split("{")[0].split("(")[0].strip() or "<anon>"
                break
        if name is None:
            kind = "comment"
            name = "<comment>"
        out.append((a + 1, b + 1, kind, name))
    return out


def main() -> int:
    # 一次性迁移脚本：已经拆过就别再跑（再跑会把子模块目录覆盖成基于已拆文件的内容）。
    if "mod primitives;" in SRC.read_text(encoding="utf-8"):
        print(
            "!! 已经拆分过（根文件里有 `mod primitives;`）——这是一次性迁移脚本，勿重复运行。",
            file=sys.stderr,
        )
        return 3
    lines = SRC.read_text(encoding="utf-8").split("\n")
    all_chunks = chunks(lines)

    keep: list[str] = []
    mod_keep: list[str] = []  # mod tests 声明
    moved: dict[str, list[tuple[int, int, str]]] = {"primitives": [], "dnd": [], "tests": []}

    # 头部（模块文档 `//!` + `use`）到第一个**真项**之前；
    # 前置的纯注释块也算头部（否则 `//!` 会被留在项中间 → E0753）。
    head_end = 0
    for a, b, kind, name in all_chunks:
        if kind != "comment":
            head_end = a - 1
            break
    head = "\n".join(lines[:head_end])
    first_item = head_end + 1

    for a, b, kind, name in all_chunks:
        if b < first_item:
            continue
        # `impl Render for NavDragGhost` 这类：按被实现的类型判归属。
        impl_of = name.split(" for ")[-1].strip() if kind == "impl" else ""
        if kind == "mod" and name == "tests":
            moved["tests"].append((a, b, kind, name))
            mod_keep.append("#[cfg(test)]\nmod tests;")
            continue
        if impl_of in DND_ITEMS:
            moved["dnd"].append((a, b, kind, name))
            continue
        if kind == "fn" and (name in PRIMITIVE_FNS or name in DND_ITEMS):
            moved["primitives"].append((a, b, kind, name))
            continue
        if name in DND_ITEMS:
            moved["dnd"].append((a, b, kind, name))
            continue
        keep.append("\n".join(lines[a - 1 : b]))

    if not moved["primitives"] or not moved["dnd"] or not moved["tests"]:
        print("!! 切分结果异常：某个目标模块为空", file=sys.stderr)
        return 2

    DIR.mkdir(exist_ok=True)
    for name, header in MOD_HEADERS.items():
        body = "\n\n".join(indent_visibility("\n".join(lines[a - 1 : b])) for a, b, _, _ in moved[name])
        (DIR / f"{name}.rs").write_text(f"{header}\n\nuse super::*;\n\n{body}\n", encoding="utf-8")
        print(f"写入 {DIR / (name + '.rs')}：{sum(b - a + 1 for a, b, _, _ in moved[name])} 行")

    # 测试模块：方法/自由函数不需要 pub(super)（子模块能看祖先私有项），但要挂 use super::*
    tests_body = "\n".join(lines[moved["tests"][0][0] - 1 : moved["tests"][0][1]])
    tests_body = re.sub(r"^mod tests \{", "", tests_body, count=1, flags=re.M).rstrip()
    if tests_body.endswith("}"):
        tests_body = tests_body[: tests_body.rfind("}")].rstrip()
    (DIR / "tests.rs").write_text(
        "//! 导航面板单测（自 `nav_view.rs` 整体搬出，**逐字未改**）。\n"
        "//!\n"
        "//! 搬出的理由：测试占原文 1426 行（约 19%），与生产代码混在一屏里，\n"
        "//! 改实现要跨过一千多行测试才能看到下一个函数。\n\n"
        + tests_body
        + "\n",
        encoding="utf-8",
    )
    print(f"写入 {DIR / 'tests.rs'}：{moved['tests'][0][1] - moved['tests'][0][0] + 1} 行")

    root = head.rstrip() + "\n" + ROOT_TAIL + "\n\n" + "\n\n".join(keep) + "\n\n" + "\n".join(mod_keep) + "\n"
    SRC.write_text(root, encoding="utf-8")
    print(f"重写 {SRC}：{len(root.split(chr(10)))} 行（原 {len(lines)} 行）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
