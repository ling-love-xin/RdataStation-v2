import sqlite3

G = "C:/Users/sling/AppData/Roaming/RdataStation/system/global.db"
c = sqlite3.connect("file:" + G + "?mode=ro", uri=True)
cur = c.cursor()


def dump(sql, label):
    print("=" * 16, label)
    try:
        cols = [d[0] for d in cur.execute(sql).description]
        print("cols:", cols)
        for row in cur.execute(sql):
            print("  ", row)
    except Exception as e:  # noqa: BLE001
        print("ERR", e)


dump("SELECT id, name, db_type, is_file, enabled FROM drivers ORDER BY id", "drivers")
dump("SELECT id, name, category FROM data_source_types ORDER BY id", "data_source_types")
c.close()

P = "D:/data/A1/A1/.RSmeta/project.db"
c = sqlite3.connect("file:" + P + "?mode=ro", uri=True)
cur = c.cursor()
dump("SELECT * FROM project_drivers LIMIT 20", "project_drivers")
dump("SELECT id, name, sort_order FROM connection_groups", "groups")
dump("SELECT group_id, connection_id, sort_order FROM connection_group_members", "members")
dump("SELECT connection_id, tag FROM connection_tags", "tags")
dump("SELECT conn_id, scope, expanded_keys FROM navigator_state", "nav_state")
c.close()
