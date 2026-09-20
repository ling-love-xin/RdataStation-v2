"""nav_view 拆分 · 阶段 2：把巨型 `impl NavView`（约 5000 行）按方法归属切进子模块。

阶段 1（`tools/split_nav_view.py`）已把纯函数 / 拖拽类型 / 测试模块搬进 `nav_view/`。
本脚本处理**剩下的主体**：一个 `impl NavView` 里塞了 79 个方法。

手法：
- 按「缩进 4 的候选行」（`#[` / `///` / `fn` / `pub … fn`）+ 花括号平衡解析方法边界
  （多行签名：必须见过 `{` 才算闭合，否则会把签名当成整项）；
- 按名字白名单分配到 `rows.rs` / `chrome.rs` / `editors.rs` / `actions.rs`，
  未列名的落 `actions.rs` 并**打印出来**（便于人工核对是否有漏网）；
- 搬出的方法加 `pub(super)`（`crate::nav_view` 及其子孙可见），
  跨模块调用（`self.render_nav_row(..)`）因此照旧；
- 根文件只留「导航锚点」那几个方法 + 模块声明。

用法：
    python tools/split_nav_view_impl.py --dry-run   # 只看分配
    python tools/split_nav_view_impl.py             # 实际切
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

SRC = Path("crates/database/src/nav_view.rs")
DIR = Path("crates/database/src/nav_view")

# ---- 方法归属（与阶段 1 的设计一致：按「这一行长什么样 / 状态怎么变」分家）----
CHROME = {
    "render_nav",
    "nav_source_chip",
    "render_nav_status_bar",
    "nav_filter_summary",
    "build_facet_items",
    "render_nav_body",
    "render_nav_empty_state",
    "render_search_section",
    "render_search_hit",
    "nav_type_from_catalog",
}
ROWS = {
    "collect_nav_rows",
    "collect_connection_rows",
    "collect_node_rows",
    "nav_row_sizes",
    "sync_nav_order",
    "nav_node_loaded",
    "nav_more_numbers",
    "render_nav_row",
    "render_group_header",
    "render_reference_row",
    "render_connection_row",
    "render_nav_node",
    "render_more_row",
    "render_jumped_row",
    "nav_scroll_to_key",
    "nav_type_of",
    "nav_conn_passes_facets",
    "nav_conn_passes_tree_row",
}
EDITORS = {
    "render_group_editor",
    "render_tag_editor",
    "render_copy_editor",
    "commit_nav_tags",
    "commit_copy_connection",
    "cancel_copy_connection",
    "commit_group_rename",
    "save_group_form",
    "delete_group",
    "group_form_seed",
    "next_group_name",
}

HEADERS = {
    "rows": (
        "//! 导航树的**可见行**：扁平化、行高、以及「这一行长什么样」。\n"
        "//!\n"
        "//! 分工：`nav_rows.rs`（`crate::nav_rows`）只管顺序与身份，本模块管渲染与高度；\n"
        "//! 两者成对维护——虚拟列表按给定高度定义布局，估小会压行（`v_virtual_list` 不回写实测值）。\n"
        "//!\n"
        "//! 为什么独立成模块：这里装着六种行的渲染（分组头 / 连接 / 引用 / 树行 / 「加载更多」/\n"
        "//! 「已定位」）与它们的高度估算，是全文件最大的一块；与面板外壳（`chrome.rs`）分开后，\n"
        "//! 改行样式不必跨过整个面板的脚手架。"
    ),
    "chrome": (
        "//! 面板外壳：面板头 / 搜索行 / 归属域 chips / facet 弹层 / 结果区 / 空态 / 底部状态行。\n"
        "//!\n"
        "//! 与 `rows.rs` 的边界：本模块负责「列表之外的一切」——列表本体、虚拟列表装配、\n"
        "//! 行高计算都在 `rows.rs`。"
    ),
    "editors": (
        "//! 行内编辑器：归组 / 标签 / 复制为模板 的渲染与提交路径。\n"
        "//!\n"
        "//! 这些块**参与行高计算**（`NAV_EDITOR_TAG` / `_COPY` / `_GROUP`，与 `rows.rs` 的行高\n"
        "//! 常量成对）：改块高必须同步改 `nav_row_height`，所以两者相邻放着更好找。"
    ),
    "actions": (
        "//! 面板的状态变更：展开 / 刷新 / 定位泵 / 后台结果回填 / 筛选落库 / 拖拽落点。\n"
        "//!\n"
        "//! 与渲染模块的边界：本模块几乎不画东西，只改状态并 `cx.notify()`；\n"
        "//! 渲染期只读的纪律（render 内不做 I/O）在 `chrome.rs` / `rows.rs` 的注释里。"
    ),
}
ROOT_TAIL_MARK = "// ===== 子模块（拆分见 `tools/split_nav_view.py`；路径保持 `nav_view.rs` 不变） ====="


def parse_impl_methods(lines: list[str], a: int, b: int) -> list[tuple[int, int, str]]:
    """impl 块内的方法 → [(起, 止, 名字)]（1-based 闭区间）。a/b 是 impl 的 `{` 与 `}` 所在行。

    先把**连续的候选行并成一组**（文档注释 + 属性 + 签名），否则一行文档注释会被当成
    一个新方法的开头（第一版把 79 个方法数成了 282 个）。
    """
    groups: list[int] = []
    prev = None
    for i in range(a, b - 1):  # a 是 `impl … {` 行，主体从 a+1 开始
        ln = lines[i]
        if re.match(r"^    (#\[|///|fn\s|pub(\(crate\)|\(super\))?\s+fn\s)", ln):
            if prev is None or i != prev + 1:
                groups.append(i)
            prev = i
    methods: list[tuple[int, int, str]] = []
    for k, s in enumerate(groups):
        name = None
        for i in range(s, b - 1):
            m = re.match(r"^    (?:pub(?:\((?:crate|super)\))?\s+)?fn\s+(\w+)", lines[i])
            if m:
                name = m.group(1)
                break
            if i > s + 12:
                break
        if name is None:
            print(f"!! 行 {s + 1} 起的候选组找不到 fn 签名，跳过", file=sys.stderr)
            continue
        # 行号一律按 1-based 闭区间：下一个组的 0-based 起点 G 就是本方法的 1-based 末行；
        # 最后一个方法到 impl 的 `}` 前一行（0-based b-1 → 1-based b）。
        end = groups[k + 1] if k + 1 < len(groups) else b
        methods.append((s + 1, end, name))
    return methods


def main() -> int:
    dry = "--dry-run" in sys.argv
    # 一次性迁移脚本：已经拆过就别再跑。
    if "mod actions;" in SRC.read_text(encoding="utf-8"):
        print(
            "!! 已经拆过 `impl NavView`（根文件里有 `mod actions;`）——一次性脚本，勿重复运行。",
            file=sys.stderr,
        )
        return 3
    lines = SRC.read_text(encoding="utf-8").split("\n")

    # 找巨型 impl NavView（含 `{` 的那行起，到列 0 的 `}` 行止）
    impl_start = None
    impl_end = None
    for i, ln in enumerate(lines):
        if re.match(r"^impl NavView\s*\{", ln):
            j = i
            d = 0
            while j < len(lines):
                d += lines[j].count("{") - lines[j].count("}")
                if d == 0 and j > i:
                    break
                j += 1
            if impl_start is None or (j - i) > (impl_end - impl_start):
                impl_start, impl_end = i, j
    if impl_start is None:
        print("!! 找不到 impl NavView", file=sys.stderr)
        return 2
    print(f"巨型 impl NavView：行 {impl_start + 1}-{impl_end + 1}（{impl_end - impl_start + 1} 行）")

    methods = parse_impl_methods(lines, impl_start, impl_end)
    print(f"解析出 {len(methods)} 个方法")

    def target(name: str) -> str:
        if name in CHROME:
            return "chrome"
        if name in ROWS:
            return "rows"
        if name in EDITORS:
            return "editors"
        return "actions"

    buckets: dict[str, list[tuple[int, int, str]]] = {"chrome": [], "rows": [], "editors": [], "actions": []}
    for a, b, name in methods:
        buckets[target(name)].append((a, b, name))
    for key in ("chrome", "rows", "editors", "actions"):
        total = sum(b - a + 1 for a, b, _ in buckets[key])
        print(f"  {key:<8} {len(buckets[key]):>3} 个方法 / {total:>5} 行")
    if dry:
        for key in ("chrome", "rows", "editors", "actions"):
            print(f"--- {key} ---")
            print("   ", ", ".join(n for _, _, n in buckets[key]))
        return 0

    # 生成子模块：`impl NavView { … }`（方法加 pub(super)）
    def emit(key: str) -> None:
        body = []
        for a, b, name in buckets[key]:
            chunk = "\n".join(lines[a - 1 : b])
            # 只给**没有可见性限定符**的方法加 `pub(super)`；
            # 原本 `pub fn` 的（`new` / `focus_nav_search` / `render_nav` / `reveal_hit` / `reveal_ref`）
            # 是**跨 crate 公开面**（workbench 在调），降级成 `pub(super)` 会直接把外部调用编译错。
            chunk = re.sub(
                r"^    fn\s",
                "    pub(super) fn ",
                chunk,
                count=1,
                flags=re.M,
            )
            body.append(chunk)
        text = f"{HEADERS[key]}\n\nuse super::*;\n\nimpl NavView {{\n" + "\n\n".join(body) + "\n}\n"
        (DIR / f"{key}.rs").write_text(text, encoding="utf-8")
        print(f"写入 {DIR / (key + '.rs')}：{len(text.splitlines())} 行")

    for key in ("rows", "chrome", "editors", "actions"):
        emit(key)

    # 根文件：删掉巨型 impl，补模块声明
    kept = lines[: impl_start] + lines[impl_end + 1 :]
    text = "\n".join(kept)
    mods = "\n".join(["mod actions;", "mod chrome;", "mod editors;", "mod rows;"])
    anchor_line = "mod primitives;"
    idx = text.find(anchor_line)
    if idx < 0:
        print("!! 找不到阶段 1 的模块声明块（`mod primitives;`），中止", file=sys.stderr)
        return 2
    insert_at = idx + len(anchor_line)
    text = text[:insert_at] + "\n" + mods + text[insert_at:]
    SRC.write_text(text.rstrip() + "\n", encoding="utf-8")
    print(f"重写 {SRC}：{len(text.splitlines())} 行")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
