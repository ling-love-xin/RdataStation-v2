"""一次性：把 `scratchpad_view.rs`（4101 行）拆成「模块根 + 7 子模块」（**纯位移**）。

拆分范式与 `database/src/nav_view.rs`（V13）完全一致：
1. **文件名不变**：保留 `scratchpad_view.rs` 作模块根（Rust 2018 允许 `foo.rs` + `foo/` 并存），
   文档与 skills 里的路径引用零改动；
2. 子模块开头 `use super::*;`（把根的作用域带下来），搬出的项加 `pub(super)`，
   根用 `use self::<模块>::*;` 收回来 —— **调用点一字未改**；
3. `pub fn` / `pub struct` 是跨 crate 公开面，不降级；根用 `pub use` 重导保持路径稳定。

已执行，**勿重复运行**（以当前文件为输入再写一遍会得到嵌套结果）。
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "crates/scratchpad/src/scratchpad_view.rs"
OUT = ROOT / "crates/scratchpad/src/scratchpad_view"

ITEM_RE = re.compile(r"^(pub(?:\(super\))?\s+)?(struct|enum|fn|impl|const|type|trait)\b")
DOC_RE = re.compile(r"^(\s*)(///|#\[)")
USE_RE = re.compile(r"^use ")
DECL_NAME_RE = re.compile(r"^(pub(?:\(super\))?\s+)?(struct|enum|fn|const|type|trait)\s+(\w+)")
METHOD_RE = re.compile(r"^    (pub(?:\(super\))?\s+)?fn\s+(\w+)")

# 顶层项 → 目标子模块（"root" = 留在模块根）。
DEST: dict[str, str] = {
    "ScratchpadEdit": "primitives",
    "ScratchpadTemplate": "primitives",
    "scratchpad_apply_template_ext": "primitives",
    "ScratchpadUndo": "primitives",
    "ScratchpadClipboardMode": "primitives",
    "ScratchpadClipboard": "primitives",
    "ScratchpadSearchMode": "primitives",
    "ScratchpadSort": "primitives",
    "scratchpad_sort_label": "primitives",
    "scratchpad_cycle_sort": "primitives",
    "scratchpad_sort_entries": "primitives",
    "scratchpad_entry_matches": "primitives",
    "flatten_scratchpad": "primitives",
    "scratchpad_basename": "primitives",
    "scratchpad_shows_dirty_dot": "primitives",
    "scratchpad_meta_label": "primitives",
    "scratchpad_relative_time": "primitives",
    "scratchpad_size_label": "primitives",
    "scratchpad_split_name": "primitives",
    "join_scratchpad_rel": "primitives",
    "scratchpad_icon_color": "primitives",
    "ScratchpadSearchHit": "search",
    "ScratchpadSearchView": "search",
    "ScratchpadDiffView": "search",
    "search_view_from_payload": "search",
    "render_scratchpad_search_pane": "search",
    "render_scratchpad_diff_pane": "search",
    "scratchpad_diff_marker": "search",
    "scratchpad_hit_line": "search",
    "ScratchpadDragGhost": "dnd",
    # 留在根：状态类型 + 行输入模型 + 视图协议（字段可见性因此无需改动）。
    "ScratchpadConflict": "root",
    "ScratchpadViewState": "root",
    "ScratchpadListScroll": "root",
    "ScratchpadRowColors": "root",
    "ScratchpadRowCtx": "root",
    "ScratchpadView": "root",
}

# `impl` 块 → 目标（按 `impl 头:首个方法名` 判定；同型 impl 有多块）。
IMPL_DEST: dict[str, str] = {
    "impl ScratchpadTemplate:label": "primitives",
    "impl ScratchpadRowCtx:is_edit_row": "rows",
    "impl ScratchpadViewState:kind_of": "root",
    "impl Default for ScratchpadListScroll:default": "root",
    "impl ScratchpadListScroll:handle": "root",
    "impl ScratchpadView:new": "root",
    "impl Focusable for ScratchpadView:focus_handle": "root",
    "impl Render for ScratchpadView:render": "root",
    "impl gpui_kit::Render for ScratchpadDragGhost:render": "dnd",
    "impl ScratchpadView:request_scratchpad_load": "actions",
}

# 混装的大 impl 块：按方法切成若干段，每段包回一个 `impl ScratchpadView { ... }`。
# （行渲染 / 动作 / 主视图三段在同一块里，见 HEAD 2256-3853）。
RUN_TABLE: dict[str, list[tuple[str, str]]] = {
    "impl ScratchpadView:scratchpad_row": [
        ("scratchpad_row", "rows"),
        ("pick_scratchpad_imports", "actions"),
        ("render_scratchpad", "chrome"),
    ],
}

ROOT_DOC = """//! 草稿箱面板（M5）：文件树 / 内联编辑 / 剪贴板 / 回收站 / 内容搜索 / 替换。
//!
//! 本 crate（`scratchpad`）自带视图；宿主能力经 [`ScratchpadHost`] 注入（`ScratchpadView::new`），
//! 与 `mock` / `insight` / `analytics_resource` / `database` 同形。
//!
//! ## 模块地图（2026-09-20 拆分；总入口仍是本文件）
//!
//! 拆分前是一个 4101 行的单文件——改一处 UI 要在一屏里找半天，而且两个写入者同时改极易冲突
//! （本文件有并发写损坏史，见 `database-nav-dev-plan.md` 的「并行写冲突记录」）。现在按
//! 「这一行长什么样 / 状态怎么变 / 外壳」分家：
//!
//! | 文件 | 职责 |
//! | --- | --- |
//! | `scratchpad_view.rs`（本文件） | `ScratchpadView` 结构与协议 + 状态类型 + 行输入模型 |
//! | `scratchpad_view/primitives.rs` | 纯函数与值模型（模板 / 排序 / 压平 / 文案 / 脏点 / 类型色点） |
//! | `scratchpad_view/search.rs` | 内容搜索 / 替换 / 冲突 Diff（中央编辑区两个面板，跨 crate 公开面） |
//! | `scratchpad_view/rows.rs` | 草稿树的行（行高预算 / 单行渲染 / 内联编辑行 / 空态） |
//! | `scratchpad_view/chrome.rs` | 面板外壳（工具栏 / 搜索行 / 树装配 / 引用 / 回收站 / 撤销栏 / 状态行） |
//! | `scratchpad_view/actions.rs` | 状态变更与后台接线（加载 / 监控 / 编辑 / 删除 / 剪贴板 / 引用） |
//! | `scratchpad_view/dnd.rs` | 拖拽幽灵（拖入编辑器时跟着鼠标的胶囊） |
//! | `scratchpad_view/tests.rs` | 单测（自本文件整体搬出） |
//!
//! 两条拆分规则（后续扩展请沿用）：
//! 1. **文件名不变**：保留 `scratchpad_view.rs` 作模块根（Rust 2018 允许 `foo.rs` + `foo/` 并存），
//!    文档与 skills 里的路径引用因此零改动；
//! 2. **跨模块项加 `pub(super)`**：子模块能看见祖先的私有项，但根与兄弟模块看不见子模块的私有项；
//!    `pub` 是**跨 crate 公开面**（`new` / `render_scratchpad` / `ensure_scratchpad_pump` /
//!    两个中央区渲染函数），拆分时不得降级。状态类型与行输入模型（`ScratchpadViewState` /
//!    `ScratchpadRowCtx` / `ScratchpadRowColors`）留在根，字段可见性因此无需改动。
//!
//! 依赖分工（下沉后不再有 `Shared`）：
//! - 项目根 / 只读判定 / 提示 / 宿主重绘 / 搜索结果落地 / 在编辑器中打开 → [`ScratchpadHost`]；
//! - 草稿库、回收站、文件监控 → 本 crate（`store` / `trash` / `watch`）；
//! - 重操作（读盘 / 导入 / 搜索 / 替换）→ [`crate::jobs`] 的工作线程，本模块只入队与回填。
//!
//! 结构设计：`docs/architecture/layout/panels-coupling-plan.md` §9。

"""

MOD_DECLS = """// ===== 子模块（拆分见 `tools/split_scratchpad_view.py`） =====
//
// 为什么保留 `scratchpad_view.rs` 这个文件名并另建同名目录：文档与 skills 里有多处路径引用，
// 改文件名等于同步改文档；Rust 2018 允许 `foo.rs` + `foo/` 并存，于是**路径稳定**、拆分零外部改动。
// 脚本是一次性迁移工具（已执行，**勿重复运行**）。

mod actions;
mod chrome;
mod dnd;
mod primitives;
mod rows;
mod search;

use primitives::*;
use dnd::*;
use search::*;

// 跨 crate 公开面：路径保持 `scratchpad_view::{...}` 不变（`lib.rs` 与编辑器都在用）。
pub use search::{
    ScratchpadDiffView, ScratchpadSearchView, render_scratchpad_diff_pane,
    render_scratchpad_search_pane,
};

"""

# 跨模块读字段的结构体：字段放宽到 `pub(super)`（根与兄弟模块要读写它们）。
FIELDS_PUB_SUPER = {
    "ScratchpadUndo",
    "ScratchpadClipboard",
    "ScratchpadSearchHit",
    "ScratchpadSearchView",
    "ScratchpadDragGhost",
}

HEADERS = {
    "primitives": """//! 纯函数与值模型（无 `self`、无 I/O、可单测）。
//!
//! 为什么要独立成模块：模板 / 排序 / 压平 / 名称与元信息文案 / 脏点判据 / 类型色点与面板状态
//! 无关，此前混在 4100 行的单一文件里，改一处要在一屏里找半天；搬出后根只留
//! 「状态 + 行输入模型 + 视图协议」。
//!
//! 可见性：搬出的项加 `pub(super)`（= 在 `crate::scratchpad_view` 及其子孙模块可见），
//! 根用 `use self::primitives::*;` 收回来，**调用点一字未改**。

use super::*;
""",
    "search": """//! 内容搜索 / 替换与冲突 Diff：**落中央编辑区**的两个面板（视图类型 + 渲染）。
//!
//! 与侧栏的分工（架构决策 D7）：草稿箱只投载荷（`ScratchpadHost::show_diff` /
//! `Shared::scratchpad_search`），重结果由编辑区渲染——240px 的侧栏装不下带上下文的命中列表。
//! 因此本模块的两个渲染函数是**跨 crate 公开面**（`pub`，由 workbench 的编辑器调用）。

use super::*;
""",
    "dnd": """//! 拖拽幽灵：把草稿行拖进编辑器时跟着鼠标的胶囊（载荷类型在 `shared::drag`）。

use super::*;
""",
    "rows": """//! 草稿树的**行**：行高预算、单行渲染、内联编辑行、空态。
//!
//! 与面板外壳（`chrome.rs`）的边界：列表本体与「每一种行长什么样」在这里；
//! 面板头 / 工具栏 / 搜索行 / 引用 / 回收站 / 状态行在外壳。
//!
//! 行高是**硬契约**：虚拟列表（`v_virtual_list`）按给定高度定义布局、不回写实测值，
//! 所以 `scratchpad_row_height` 的估算与行渲染成对维护（估小会压行）。

use super::*;
""",
    "chrome": """//! 面板外壳：面板头 / 工具栏 / 搜索行 / 草稿树装配 / 引用 / 回收站 / 撤销栏 / 状态行。
//!
//! 这是 `render_scratchpad` 的落点——草稿箱唯一的主视图（虚拟列表的装配也在这里，
//! 行本身在 `rows.rs`）。

use super::*;
""",
    "actions": """//! 状态变更与后台接线：加载 / 懒加载 / 监控轮询 / 冲突检测 / 编辑提交 / 删除与撤销 /
//! 剪贴板 / 外部引用 / 打开位置。
//!
//! 纪律：**渲染期零 I/O**——本模块的方法只入队后台任务（[`crate::jobs`]）并回填结果；
//! 少数单次系统调用（新建 / 重命名 / 引用增删改，微秒~毫秒级）保持同步回填（判据见
//! `scratchpad-architecture.md` §13.1 K1c）。

use super::*;
""",
}

TEST_HEADER = """//! 草稿箱面板单测（自 `scratchpad_view.rs` 整体搬出，仅整体减一级缩进）。
//!
//! 搬出的理由：测试与生产代码混在一屏里，改实现要跨过两百多行测试才看到下一个函数。
//! 窗口级交互（真点击、真轮询）不在这一层，见 `scratchpad-user-guide.md` §9 验收清单。

"""


def split_chunks(lines: list[str]) -> list[dict]:
    """切成顶层项块（含项上方的文档 / 属性行）；`mod tests` 之前的部分才参与切分。"""
    items_end = len(lines)
    for i, line in enumerate(lines):
        if line.startswith("mod tests"):
            # 连同上一行的 `#[cfg(test)]` 一起切掉。
            items_end = i - 1 if lines[i - 1].strip() == "#[cfg(test)]" else i
            break

    starts: list[tuple[int, int]] = []  # (块首行, 声明行)
    for i, line in enumerate(lines[:items_end]):
        if ITEM_RE.match(line):
            j = i
            while j > 0 and DOC_RE.match(lines[j - 1]):
                j -= 1
            starts.append((j, i))
    out: list[dict] = []
    for k, (start, decl) in enumerate(starts):
        end = starts[k + 1][0] if k + 1 < len(starts) else items_end
        out.append(
            {
                "start": start,
                "decl": decl,
                "off": decl - start,
                "end": end,
                "lines": lines[start:end],
            }
        )
    return out


def first_method(block: list[str], off: int) -> str:
    for line in block[off:]:
        m = METHOD_RE.match(line)
        if m:
            return m.group(2)
    return ""


def key_of(block: list[str], off: int) -> tuple[str, str]:
    decl = block[off].strip()
    if decl.startswith("impl"):
        head = decl.split("{")[0].strip()
        return ("impl", f"{head}:{first_method(block, off)}")
    m = DECL_NAME_RE.match(decl)
    if not m:
        return ("?", decl)
    return (m.group(2), m.group(3))


def method_offsets(block: list[str], off: int) -> list[tuple[int, int]]:
    """块内每个方法的 (文档行偏移, 声明行偏移)。"""
    out: list[tuple[int, int]] = []
    for i in range(off + 1, len(block)):
        if METHOD_RE.match(block[i]):
            j = i
            while j > off + 1 and block[j - 1].strip().startswith("///"):
                j -= 1
            out.append((j, i))
    return out


def split_runs(block: list[str], off: int, runs: list[tuple[str, str]]) -> list[tuple[str, list[str]]]:
    """把一条 impl 块按 RUN_TABLE 切成 [(目标, 段行), ...]（段自带 `impl ... {`/`}` 壳）。"""
    offsets = method_offsets(block, off)
    names = [METHOD_RE.match(block[decl]).group(2) for _doc, decl in offsets]
    picked: list[tuple[int, str]] = []
    for method, dest in runs:
        idx = names.index(method)
        picked.append((offsets[idx][0], dest))
    out: list[tuple[str, list[str]]] = []
    close = max(i for i, ln in enumerate(block) if ln.rstrip("\n") == "}")
    for k, (start, dest) in enumerate(picked):
        end = picked[k + 1][0] if k + 1 < len(picked) else close
        body = [re.sub(r"^    fn\s", "    pub(super) fn ", ln) for ln in block[start:end]]
        out.append((dest, [block[off] + "\n"] + body + ["}\n"]))
    return out


def relax_visibility(block: list[str], off: int) -> list[str]:
    """搬出根的项：项声明、impl 内方法与关联常量加 `pub(super)`（`pub` 不降级）。"""
    out = list(block)
    out[off] = re.sub(
        r"^(pub(?:\(super\))?\s+)?(?=(struct|enum|fn|const|type|trait)\b)",
        lambda m: m.group(1) or "pub(super) ",
        out[off],
    )
    # trait impl（`impl Trait for T`）的方法不能加可见性限定符。
    is_trait_impl = " for " in out[off].split("{")[0]
    for i in range(off + 1, len(out)):
        if not is_trait_impl:
            out[i] = re.sub(r"^    fn\s", "    pub(super) fn ", out[i])
            out[i] = re.sub(r"^    const\s", "    pub(super) const ", out[i])
        # 字段可见性：跨模块读写的结构体（奇数字段）放宽一档。
        m = re.match(
            r"^(struct|enum)\s+(\w+)", re.sub(r"^pub(?:\(super\))?\s+", "", out[off].strip())
        )
        if m and m.group(2) in FIELDS_PUB_SUPER:
            out[i] = re.sub(r"^(    )(\w+:\s)", r"\1pub(super) \2", out[i])
    return out


def main() -> None:
    lines = SRC.read_text(encoding="utf-8").splitlines(keepends=True)
    blocks = split_chunks(lines)

    # ---- 文件头：模块文档 + use 块（use 块原样带走） ----
    first_item = blocks[0]["start"]
    head = lines[:first_item]
    use_start = next(i for i, ln in enumerate(head) if USE_RE.match(ln))
    use_end = max(i for i, ln in enumerate(head) if USE_RE.match(ln))
    use_block = head[use_start : use_end + 1]
    for i, ln in enumerate(use_block):
        if ln.strip() == "":
            use_block[i] = "\n"
    uses = "".join(use_block)

    # ---- 测试体（在改写根之前取出；去包装 + 整体减一级缩进） ----
    tests_start = next(i for i, ln in enumerate(lines) if ln.startswith("mod tests")) + 1
    tests_end = max(i for i in range(len(lines) - 1, 0, -1) if lines[i].rstrip() == "}")
    tests_body = [
        ln[4:] if ln.startswith("    ") else ln for ln in lines[tests_start:tests_end]
    ]

    # ---- 分桶 ----
    buckets: dict[str, list[str]] = {name: [] for name in HEADERS}
    buckets["root"] = []
    for block in blocks:
        kind, key = key_of(block["lines"], block["off"])
        if key in RUN_TABLE:
            for dest, text in split_runs(block["lines"], block["off"], RUN_TABLE[key]):
                buckets[dest].extend(text)
                print(f"{dest:10s} run    {key}", file=sys.stderr)
            continue
        table = IMPL_DEST if kind == "impl" else DEST
        dest = table.get(key)
        if dest is None:
            sys.exit(f"未登记的项：{kind} {key}（第 {block['decl'] + 1} 行）")
        buckets[dest].extend(
            block["lines"] if dest == "root" else relax_visibility(block["lines"], block["off"])
        )
        print(f"{dest:10s} {kind:6s} {key}", file=sys.stderr)

    # ---- 写子模块 ----
    OUT.mkdir(parents=True, exist_ok=True)
    total = 0
    for name, header in HEADERS.items():
        text = header + "\n" + "".join(buckets[name])
        if not text.endswith("\n"):
            text += "\n"
        (OUT / f"{name}.rs").write_text(text, encoding="utf-8", newline="\n")
        n = text.count("\n")
        total += n
        print(f"写入 {name}.rs（{n} 行）", file=sys.stderr)

    tests_text = TEST_HEADER + "".join(tests_body)
    (OUT / "tests.rs").write_text(tests_text, encoding="utf-8", newline="\n")
    print(f"写入 tests.rs（{tests_text.count(chr(10))} 行）", file=sys.stderr)

    # ---- 写根 ----
    root = "".join(
        [ROOT_DOC, uses, "\n", MOD_DECLS, "".join(buckets["root"]), "\n#[cfg(test)]\nmod tests;\n"]
    )
    SRC.write_text(root, encoding="utf-8", newline="\n")
    print(f"写入根（{root.count(chr(10))} 行）", file=sys.stderr)


if __name__ == "__main__":
    main()
