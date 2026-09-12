import sqlite3

G = "C:/Users/sling/AppData/Roaming/RdataStation/system/global.db"
c = sqlite3.connect("file:" + G + "?mode=ro", uri=True)
cur = c.cursor()
cols = [d[1] for d in cur.execute("PRAGMA table_info(drivers)")]
print("drivers cols:", cols)
for row in cur.execute("SELECT * FROM drivers"):
    print("  ", row)
c.close()
