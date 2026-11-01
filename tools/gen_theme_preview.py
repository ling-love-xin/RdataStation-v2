#!/usr/bin/env python3
"""由主题资产生成色卡预览页 —— 预览与资产同源，改色后重跑即可。

用法: python tools/gen_theme_preview.py
输入: assets/themes/rds-theme.json
输出: docs/architecture/theme/theme-preview.html

配合守门脚本 `tools/theme_audit.py` 使用：先审计，再生成。
"""

import datetime
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "assets/themes/rds-theme.json"
OUT = ROOT / "docs/architecture/theme/theme-preview.html"

# ===== 对比度（与 theme_audit.py 同口径）=====


def _lin(c):
    c = c / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def _lum(hexstr):
    h = hexstr.lstrip("#")[:6]
    r, g, b = (int(h[i : i + 2], 16) for i in (0, 2, 4))
    return 0.2126 * _lin(r) + 0.7152 * _lin(g) + 0.0722 * _lin(b)


def ratio(a, b):
    la, lb = _lum(a), _lum(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


# ===== 分组：按前缀归类，避免手写 140 个键名 =====

GROUPS = [
    (
        "基础 · 文本",
        lambda k: k
        in {
            "background",
            "foreground",
            "border",
            "input.border",
            "caret",
            "selection.background",
            "overlay",
            "ring",
            "window.border",
        },
    ),
    ("链接", lambda k: k.startswith("link")),
    (
        "布局 chrome",
        lambda k: k.startswith(("sidebar", "title_bar", "status_bar", "tab")),
    ),
    ("容器", lambda k: k.startswith(("popover", "group_box", "accordion"))),
    ("列表与表格", lambda k: k.startswith(("list", "table", "description_list"))),
    ("按钮", lambda k: k.startswith("button")),
    (
        "语义色",
        lambda k: k.startswith(
            ("primary", "secondary", "accent", "muted", "success", "danger", "warning", "info")
        ),
    ),
    (
        "控件",
        lambda k: k.startswith(
            ("scrollbar", "skeleton", "slider", "switch", "progress", "drag", "drop_target")
        ),
    ),
    ("图表与基础色", lambda k: k.startswith(("chart", "base"))),
]

PAIRS = [
    ("foreground", "background"),
    ("muted.foreground", "background"),
    ("sidebar.foreground", "sidebar.background"),
    ("sidebar.accent.foreground", "sidebar.accent.background"),
    ("popover.foreground", "popover.background"),
    ("tab.foreground", "tab.background"),
    ("tab.active.foreground", "tab.active.background"),
    ("primary.foreground", "primary.background"),
    ("primary.foreground", "primary.hover.background"),
    ("primary.foreground", "primary.active.background"),
    ("button.primary.foreground", "button.primary.background"),
    ("button.danger.foreground", "button.danger.background"),
    ("button.success.foreground", "button.success.background"),
    ("button.warning.foreground", "button.warning.background"),
    ("button.info.foreground", "button.info.background"),
    ("success.foreground", "success.background"),
    ("danger.foreground", "danger.background"),
    ("warning.foreground", "warning.background"),
    ("info.foreground", "info.background"),
    ("table.head.foreground", "table.head.background"),
    ("muted.foreground", "status_bar.background"),
    ("link", "background"),
]


def build_data():
    doc = json.loads(SRC.read_text(encoding="utf-8"))
    themes = {t["name"]: t for t in doc["themes"]}
    out = {}
    for name in ("RDS Light", "RDS Dark"):
        colors = themes[name]["colors"]
        groups, used = [], set()
        for gname, match in GROUPS:
            items = [[k, colors[k]] for k in colors if match(k)]
            used.update(k for k, _ in items)
            if items:
                groups.append({"name": gname, "items": items})
        rest = [[k, colors[k]] for k in colors if k not in used]
        if rest:
            groups.append({"name": "其他", "items": rest})
        checks = []
        for fg, bg in PAIRS:
            if fg in colors and bg in colors:
                checks.append({"fg": fg, "bg": bg, "r": round(ratio(colors[fg], colors[bg]), 2)})
        out[name] = {
            "colors": colors,
            "groups": groups,
            "checks": checks,
            "count": len(colors),
            "highlight": themes[name].get("highlight", {}),
        }
    return out, doc


TEMPLATE = """<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>RdataStation 主题色卡 · 由 rds-theme.json 生成</title>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { background: #141414; font-family: "Microsoft YaHei", "Segoe UI", sans-serif; color: #d4d4d4; padding: 20px; }
  h1 { font-size: 16px; color: #fff; margin-bottom: 4px; }
  .sub { font-size: 12px; color: #9a9a9a; margin-bottom: 18px; line-height: 1.6; }
  .cols { display: flex; gap: 18px; align-items: flex-start; flex-wrap: wrap; }
  .panel { flex: 1; min-width: 420px; border: 1px solid #3c3c3c; border-radius: 8px; overflow: hidden; }
  .panel-head { padding: 10px 14px; font-size: 13px; font-weight: 600; border-bottom: 1px solid; display: flex; justify-content: space-between; }
  .panel-head .n { font-size: 11px; font-weight: 400; opacity: .7; }
  .win { margin: 12px 14px 0; border-radius: 6px; overflow: hidden; font-size: 10px; }
  .win-title { display: flex; align-items: center; height: 26px; padding: 0 8px; }
  .win-logo { width: 13px; height: 13px; border-radius: 3px; margin-right: 6px; }
  .win-cutout { padding: 2px 8px; border-radius: 4px; font-size: 9px; }
  .win-body { display: flex; height: 118px; }
  .win-act { width: 20px; display: flex; flex-direction: column; align-items: center; padding-top: 6px; gap: 6px; font-size: 9px; }
  .win-act .on { width: 100%; text-align: center; }
  .win-side { width: 74px; padding: 8px; line-height: 1.8; font-size: 9px; }
  .win-side .sel { display: block; padding: 0 4px; border-radius: 2px; }
  .win-editor { flex: 1; padding: 8px; line-height: 1.7; font-size: 9px; font-family: Consolas, monospace; }
  .win-status { display: flex; align-items: center; height: 19px; padding: 0 8px; font-size: 9px; justify-content: space-between; }
  .section { padding: 12px 14px; }
  .gname { font-size: 11px; font-weight: 600; margin: 10px 0 6px; padding-bottom: 4px; border-bottom: 1px solid; opacity: .85; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(96px, 1fr)); gap: 6px; }
  .sw { border-radius: 5px; overflow: hidden; border: 1px solid; }
  .sw .chip { height: 30px; }
  .sw .meta { padding: 3px 5px; font-size: 9px; line-height: 1.35; }
  .sw .meta .k { display: block; opacity: .8; word-break: break-all; }
  .sw .meta .v { display: block; font-family: Consolas, monospace; opacity: .95; }
  table { width: 100%; border-collapse: collapse; font-size: 10px; margin-top: 6px; }
  td, th { padding: 3px 5px; text-align: left; border-bottom: 1px solid rgba(128,128,128,.25); }
  .ok { color: #89D185; } .bad { color: #F14C4C; font-weight: 600; }
  .foot { margin-top: 18px; font-size: 11px; color: #8a8a8a; line-height: 1.7; }
</style>
</head>
<body>
<h1>RdataStation 主题色卡</h1>
<p class="sub">
  由 <code>assets/themes/rds-theme.json</code> 自动生成 —— 改色后跑 <code>python tools/gen_theme_preview.py</code> 重新生成。<br>
  生成时间 __NOW__ · 色值权威来源 <code>docs/architecture/theme/theme-tokens-complete.md</code>
</p>
<div class="cols" id="cols"></div>
<p class="foot">
  对比度判定：正文 / 控件文字 ≥ 4.5:1。选中态 <code>list.active</code> / <code>table.active</code> 的 alpha 若超过 0.2 会被 schema 的 clamp_alpha 静默压缩（表现为"选中看不见"），资产里已按上限给值。<br>
  状态栏：gpui-kit <code>StatusBar</code> 组件文字硬编码取 <code>muted_foreground</code>，故底色保持中性 —— 珊瑚底配灰字仅 1.08:1，不可行（详见色板文档 §6）。
</p>
<script>
const DATA = __DATA__;

function panel(name, d, isLight) {
  const c = d.colors;
  const chrome = isLight ? '#e0e0e0' : '#3c3c3c';
  const g = k => c[k] || 'transparent';
  let html = '<div class="panel" style="background:' + g('background') + ';border-color:' + g('border') + '">';
  html += '<div class="panel-head" style="background:' + g('sidebar.background') + ';color:' + g('foreground') + ';border-color:' + g('border') + '">'
        + '<span>' + name + '</span><span class="n">' + d.count + ' tokens</span></div>';

  html += '<div class="win" style="border:1px solid ' + g('border') + '">';
  html += '<div class="win-title" style="background:' + g('title_bar.background') + ';color:' + g('sidebar.foreground') + ';border-bottom:1px solid ' + g('title_bar.border') + '">'
        + '<span class="win-logo" style="background:' + g('primary.background') + '"></span>RdataStation'
        + '<span class="win-cutout" style="margin-left:6px;background:' + g('secondary.background') + ';color:' + g('secondary.foreground') + '">挖空槽</span></div>';
  html += '<div class="win-body">';
  html += '<div class="win-act" style="background:' + g('sidebar.background') + ';color:' + g('muted.foreground') + '">'
        + '<span class="on" style="color:' + g('sidebar.accent.foreground') + ';box-shadow:inset 2px 0 0 ' + g('primary.background') + '">▤</span><span>◇</span><span>◈</span></div>';
  html += '<div class="win-side" style="background:' + g('sidebar.background') + ';color:' + g('sidebar.foreground') + ';border-right:1px solid ' + g('sidebar.border') + '">'
        + '<span class="sel" style="background:' + g('list.active.background') + ';color:' + g('foreground') + ';box-shadow:inset 2px 0 0 ' + g('list.active.border') + '">postgres</span>'
        + '<span>mysql</span><span>duckdb</span></div>';
  html += '<div class="win-editor" style="background:' + g('background') + ';color:' + g('foreground') + '">'
        + '<div style="color:' + (d.highlight.syntax && d.highlight.syntax.keyword ? d.highlight.syntax.keyword.color : g('foreground')) + '">SELECT</div>'
        + '<div style="color:' + g('muted.foreground') + '">  * FROM orders</div>'
        + '<div style="background:' + g('selection.background') + '">  WHERE id &gt; 10</div>'
        + '<div style="color:' + g('success.background') + '">✓ 12 rows</div>'
        + '<div style="color:' + g('danger.background') + '">✗ timeout</div></div>';
  html += '</div>';
  html += '<div class="win-status" style="background:' + g('status_bar.background') + ';color:' + g('muted.foreground') + ';border-top:1px solid ' + g('status_bar.border') + '">'
        + '<span>就绪</span><span style="color:' + g('primary.background') + '">Ln 4, Col 12</span></div>';
  html += '</div>';

  html += '<div class="section">';
  for (const grp of d.groups) {
    html += '<div class="gname" style="color:' + g('foreground') + ';border-color:' + chrome + '">' + grp.name + '</div><div class="grid">';
    for (const [k, v] of grp.items) {
      html += '<div class="sw" style="border-color:' + chrome + '">'
            + '<div class="chip" style="background:' + v + '"></div>'
            + '<div class="meta" style="background:' + g('background') + ';color:' + g('foreground') + '">'
            + '<span class="k">' + k + '</span><span class="v">' + v + '</span></div></div>';
    }
    html += '</div>';
  }
  html += '<div class="gname" style="color:' + g('foreground') + ';border-color:' + chrome + '">对比度校验</div>';
  html += '<table><tr><th>前景</th><th>背景</th><th>比值</th></tr>';
  for (const ck of d.checks) {
    const ok = ck.r >= 4.5;
    html += '<tr><td style="color:' + g('foreground') + '">' + ck.fg + '</td><td style="color:' + g('muted.foreground') + '">' + ck.bg + '</td>'
          + '<td class="' + (ok ? 'ok' : 'bad') + '">' + ck.r.toFixed(2) + ':1</td></tr>';
  }
  html += '</table></div></div>';
  return html;
}

document.getElementById('cols').innerHTML =
  panel('RDS Light', DATA['RDS Light'], true) + panel('RDS Dark', DATA['RDS Dark'], false);
</script>
</body>
</html>
"""


def main():
    data, _doc = build_data()
    payload = json.dumps(data, ensure_ascii=False, separators=(",", ":"))
    html = TEMPLATE.replace("__DATA__", payload).replace(
        "__NOW__", datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
    )
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(html, encoding="utf-8")
    for name in ("RDS Light", "RDS Dark"):
        d = data[name]
        bad = [c for c in d["checks"] if c["r"] < 4.5]
        print(
            f"{name}: {d['count']} tokens, {len(d['groups'])} 组, "
            f"对比度 {len(d['checks']) - len(bad)}/{len(d['checks'])} 达标"
            + (f" —— 不达标: {bad}" if bad else "")
        )
    print(f"已生成 {OUT}")


if __name__ == "__main__":
    main()
