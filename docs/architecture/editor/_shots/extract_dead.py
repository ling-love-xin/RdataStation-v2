import sys, json, re
s = sys.stdin.read()
m = re.search(r'"deadButtons":\s*\[(.*?)\]\s*,\s*"', s, re.S)
if not m:
    print("NOT_FOUND")
else:
    raw = '[' + m.group(1) + ']'
    try:
        arr = json.loads(raw)
        for it in arr:
            print(it.get("tag"), "|", it.get("text", "")[:30], "|", it.get("reason"), "|", it.get("note",""))
    except Exception as e:
        print("PARSE_ERR", e)
        print(raw[:1200])
