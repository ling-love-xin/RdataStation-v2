import sqlite3

PATHS = [
    ("GLOBAL", "C:/Users/sling/AppData/Roaming/RdataStation/system/global.db"),
    ("PROJECT", "D:/data/A1/A1/.RSmeta/project.db"),
]

TABLES = [
    "global_connections",
    "connections",
    "connection_groups",
    "connection_group_members",
    "connection_tags",
    "navigator_state",
    "connection_drafts",
]

for label, path in PATHS:
    print("=" * 24, label, path)
    conn = sqlite3.connect("file:" + path + "?mode=ro", uri=True)
    cur = conn.cursor()
    tables = [r[0] for r in cur.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
    print("tables:", tables)
    for t in TABLES:
        if t not in tables:
            continue
        cols = [r[1] for r in cur.execute("PRAGMA table_info(" + t + ")")]
        n = cur.execute("SELECT COUNT(*) FROM " + t).fetchone()[0]
        print(" -", t, "rows =", n)
        print("   cols:", cols)
        if t in ("global_connections", "connections"):
            for row in cur.execute("SELECT id, name, driver, host, port, database, username, is_active FROM " + t):
                print("     ", row)
    conn.close()
