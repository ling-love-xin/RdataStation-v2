# 从 gpui-kit 的 schema 抽取 ThemeConfigColors 的全部合法 JSON 键名，
# 产出 assets/themes/theme-keys.json —— 供启动期做未知键告警（未知键会被 serde 静默忽略，
# 表现为「该 token 静默回退默认蓝主题」，不报错、极难排查）。
#
# 用法: python tools/gen_theme_keys.py
import json
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "vendor/gpui-component/src/theme/schema.rs"
OUT = ROOT / "assets/themes/theme-keys.json"

src = SCHEMA.read_text(encoding="utf-8")
start = src.index("pub struct ThemeConfigColors {")
depth = 0
end = start
for j in range(start, len(src)):
    if src[j] == "{":
        depth += 1
    elif src[j] == "}":
        depth -= 1
        if depth == 0:
            end = j
            break
body = src[start:end]

# 注意：base.* 系列字段是**私有**的（`blue: Option<String>`，无 `pub`），
# 只按 `pub` 匹配会漏掉 12 个键 —— 这里对可选 pub 都匹配。
keys: list[str] = []
pending = None
for line in body.split("\n"):
    m = re.search(r'#\[serde\(rename = "([^"]+)"\)\]', line)
    if m:
        pending = m.group(1)
    if re.match(r"\s*(?:pub )?\w+:", line) and pending:
        keys.append(pending)
        pending = None

keys = sorted(set(keys))
payload = {
    "$comment": (
        "由 tools/gen_theme_keys.py 从 vendor/gpui-component/src/theme/schema.rs 生成，请勿手改。"
        "主题 JSON 的 colors 里出现本清单之外的键会被 serde 忽略并静默回退默认色。"
    ),
    "source": "vendor/gpui-component/src/theme/schema.rs (pub struct ThemeConfigColors)",
    "count": len(keys),
    "keys": keys,
}
OUT.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(f"已写出 {OUT.relative_to(ROOT)}  键数 {len(keys)}")
