#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""校验「主题包」候选（docs/architecture/theme/palette-candidates/*-theme.json）。

用法: python tools/audit_theme_packs.py

复用的口径不另立一套：直接 import tools/theme_audit.py 的 schema 键集合 / PAIRS /
PRODUCT_PAIRS / HIGHLIGHT_NAMES / ratio，保证与现役资产审计完全同源（那边改口径这里自动跟上）。

检查项（每个包 × Light/Dark）:
  1. 140 键覆盖：缺键 / schema 不接受的未知键
  2. 对比度：theme_audit.PAIRS（52 对，正文 4.5 / 非文本 3）
  3. 产品 token：13 角色齐全 + PRODUCT_PAIRS（9 对，按包内产品 token 合成背景）
  4. 语法：41 个合法名覆盖（点号名可回退前缀）+ 每个带 color 的条目对比度
"""

import contextlib
import importlib.util
import io
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent
PACK_DIR = ROOT / "docs/architecture/theme/palette-candidates"
BASELINE = ROOT / "assets/themes/rds-theme.json"

_spec = importlib.util.spec_from_file_location("theme_audit", HERE / "theme_audit.py")
ta = importlib.util.module_from_spec(_spec)
with contextlib.redirect_stdout(io.StringIO()):
    _spec.loader.exec_module(ta)  # 它的打印是给现役资产用的，这里只取口径

ratio = ta.ratio
schema_keys = ta.schema_keys
hl_names = ta.hl_names

PRODUCT_KEYS = [
    "activity_bar.background", "activity_bar.active_border", "activity_bar.icon.active",
    "activity_bar.icon.inactive", "title_bar.slot.background", "quick_open.group.header",
    "search.match.background", "staged.drop", "result.odd_row.background",
    "result.cell.modified.background", "result.cell.error.background",
    "result.local_row_background", "result.header.border",
]

rows = []
fails = []
names_seen = set()


def check_pack(theme_path, product_path):
    data = json.loads(theme_path.read_text(encoding="utf-8"))
    pdata = json.loads(product_path.read_text(encoding="utf-8"))
    pack = theme_path.name.replace("-theme.json", "")
    stat = {"pack": pack, "keys": "", "pairs": 0, "product": 0, "syn_uncovered": 0,
            "syn_fail": 0, "unknown": 0, "missing": 0, "format": 0}

    # 0) 结构与格式（载入前的门槛：两套主题、明暗各一、字段齐、hex 合法、alpha 不超 clamp 上限）
    modes = [t.get("mode") for t in data["themes"]]
    if sorted(modes) != ["dark", "light"]:
        fails.append((pack, "-", "主题套数/模式", str(modes)))
    names_seen.add("__dummy__")
    for theme in data["themes"]:
        for field in ("name", "mode", "colors", "highlight"):
            if field not in theme:
                fails.append((pack, theme.get("name", "?"), "缺字段", field))
        if theme.get("name") in names_seen:
            fails.append((pack, theme.get("name"), "主题名重复", "跨包重名会让按名查找/持久化串包"))
        names_seen.add(theme.get("name"))
    CLAMP = {
        "list.active.background": 0.2, "table.active.background": 0.2,
        "accent.background": 0.2, "drop_target.background": 0.2,
        "selection.background": 0.3,
    }

    def hex_ok(v):
        v = v.lstrip("#")
        return len(v) in (6, 8) and all(c in "0123456789ABCDEFabcdef" for c in v)

    for theme in data["themes"]:
        bad_fmt = [k for k, v in theme["colors"].items() if not hex_ok(v)]
        bad_hl = [k for k, v in theme["highlight"]["syntax"].items()
                  if "color" in v and not hex_ok(v["color"])]
        if bad_fmt or bad_hl:
            stat["format"] = len(bad_fmt) + len(bad_hl)
            fails.append((pack, theme["name"], "hex 非法", (bad_fmt + bad_hl)[:6]))
        for k, lim in CLAMP.items():
            v = theme["colors"].get(k, "")
            if len(v.lstrip("#")) == 8:
                a = int(v.lstrip("#")[6:8], 16) / 255
                if a > lim + 1e-6:
                    fails.append((pack, theme["name"], f"{k} alpha 超限", f"{a:.2f} > {lim}"))

    for theme in data["themes"]:
        name, mode = theme["name"], theme["mode"]
        colors = theme["colors"]
        missing = [k for k in schema_keys if k not in colors]
        unknown = [k for k in colors if k not in schema_keys]
        stat["missing"] = max(stat["missing"], len(missing))
        stat["unknown"] = max(stat["unknown"], len(unknown))
        stat["keys"] = f"{len(colors)}/{len(schema_keys)}"
        if missing:
            fails.append((pack, name, "缺键", missing))
        if unknown:
            fails.append((pack, name, "未知键", unknown))

        # 2) 主题对比度
        bad = 0
        for fg, bg, need in ta.PAIRS:
            if fg not in colors or bg not in colors:
                fails.append((pack, name, "PAIRS 缺 token", f"{fg} on {bg}"))
                continue
            r = ratio(colors[fg], colors[bg])
            if r < need:
                bad += 1
                fails.append((pack, name, f"{fg} on {bg}", f"{r:.2f} < {need}"))
        stat["pairs"] = bad

        # 3) 产品 token
        roles = pdata.get(mode, {})
        miss_roles = [k for k in PRODUCT_KEYS if k not in roles]
        if miss_roles:
            fails.append((pack, name, "产品角色缺", miss_roles))
        merged = dict(colors)
        merged.update(roles)
        pbad = 0
        for fg, bg, need, why in ta.PRODUCT_PAIRS:
            if fg not in merged or bg not in merged:
                fails.append((pack, name, "产品 token 缺", f"{fg} on {bg}"))
                continue
            r = ratio(merged[fg], merged[bg])
            if r < need:
                pbad += 1
                fails.append((pack, name, f"产品 {fg} on {bg}", f"{r:.2f} < {need}  [{why}]"))
        stat["product"] = pbad

        # 4) 语法
        hl = theme["highlight"]
        syn = hl["syntax"]
        editor_bg = hl["editor.background"]

        def covered(n):
            return n in syn or ("." in n and n.split(".")[0] in syn)

        uncovered = [n for n in hl_names if not covered(n)]
        stat["syn_uncovered"] = max(stat["syn_uncovered"], len(uncovered))
        if uncovered:
            fails.append((pack, name, "语法未覆盖", uncovered))
        sbad = 0
        for k in sorted(syn):
            c = syn[k].get("color")
            if not c:
                continue
            need = 3.0 if k.startswith("comment") else 4.5
            r = ratio(c, editor_bg)
            if r < need:
                sbad += 1
                fails.append((pack, name, f"syntax {k}", f"{r:.2f} < {need}"))
        for fg, bg, need in (("editor.foreground", "editor.background", 4.5),
                             ("editor.active_line_number", "editor.active_line.background", 4.5),
                             ("editor.line_number", "editor.background", 3.0)):
            if fg in hl and bg in hl:
                r = ratio(hl[fg], hl[bg])
                if r < need:
                    sbad += 1
                    fails.append((pack, name, fg, f"{r:.2f} < {need}"))
        stat["syn_fail"] = sbad
    rows.append(stat)
    return stat


def main():
    packs = sorted(PACK_DIR.glob("*-theme.json"))
    if not packs:
        print(f"没有找到包文件：{PACK_DIR}", file=sys.stderr)
        return 2
    for tp in packs:
        pp = tp.with_name(tp.name.replace("-theme.json", "-product-tokens.json"))
        if not pp.exists():
            fails.append((tp.name, "-", "缺产品 token 文件", pp.name))
            continue
        check_pack(tp, pp)

    print(f"{'包':16s} {'140键':9s} {'主题PAIRS':10s} {'产品PAIRS':9s} {'语法未覆盖':9s} {'语法不达标':9s}")
    for r in rows:
        print(f"{r['pack']:16s} {r['keys']:9s} {r['pairs']:^10d} {r['product']:^9d} "
              f"{r['syn_uncovered']:^9d} {r['syn_fail']:^9d}")

    # 映射结构自检：现役资产里「同值」的键，候选展开后是否被拉开。
    # 按现役资产的同值分组，检查候选（coral-graphite，与现役资产最接近）给这组键的实际色值，
    # 通道最大差 ≥ 4 视为「有意拉开」，逐条列出供人工确认（不是失败项）。
    _pack_spec = importlib.util.spec_from_file_location("gtp", HERE / "gen_theme_packs.py")
    gtp = importlib.util.module_from_spec(_pack_spec)
    with contextlib.redirect_stdout(io.StringIO()):
        _pack_spec.loader.exec_module(gtp)
    base = json.loads(BASELINE.read_text(encoding="utf-8"))
    cg = json.loads((PACK_DIR / "coral-graphite-theme.json").read_text(encoding="utf-8"))
    cg_colors = {t["mode"]: t["colors"] for t in cg["themes"]}

    def rgb(h):
        h = h.lstrip("#")[:6]
        return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))

    print("\n=== 映射结构自检：现役资产「同值组」在候选里被拉开（通道差 ≥ 4）的 ===")
    split = 0
    for theme in base["themes"]:
        mode = theme["mode"]
        groups = {}
        for k, v in theme["colors"].items():
            groups.setdefault(v.upper(), []).append(k)
        print(f"\n--- {theme['name']} ---")
        for v, keys in sorted(groups.items()):
            if len(keys) < 2:
                continue
            vals = [cg_colors[mode][k] for k in keys]
            spread = max(max(abs(a - b) for a, b in zip(rgb(x), rgb(y)))
                         for x in vals for y in vals)
            if spread < 4:
                continue
            split += 1
            roles = sorted({gtp.KEY_ROLES[k] for k in keys})
            print(f"  原值 {v}  {len(keys)} 键  Δ={spread}  -> {roles}")
            for k in keys:
                print(f"      {k:36s} {cg_colors[mode][k]}  ({gtp.KEY_ROLES[k]})")
    print(f"\n被拉开的组数 {split}（按现役资产的同值聚合口径核对，确认这些区分是有意为之）")

    print("\n=== 不达标明细 ===")
    if fails:
        for p in fails:
            print(" ", p)
        print(f"\n合计 {len(fails)} 项不达标")
        return 1
    print("  无")
    print(f"\n{len(rows)} 个包 × 明暗全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
