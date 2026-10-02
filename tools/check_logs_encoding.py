# -*- coding: utf-8 -*-
"""检查 global.db 的 app_logs 表内容编码（诊断日志乱码用）。"""
import sqlite3

con = sqlite3.connect(r'D:\RdataStation\RDS\RdataStation-v2\target\debug\data\system\global.db')
cur = con.cursor()
cur.execute("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '%log%'")
tables = [r[0] for r in cur.fetchall()]
print('log tables:', tables)
for t in tables:
    try:
        cur.execute(f'SELECT * FROM "{t}" ORDER BY rowid DESC LIMIT 3')
        rows = cur.fetchall()
        print(f'--- {t} ---')
        for r in rows:
            # 逐字段打印 repr，判断中文是否正常
            print([repr(x) for x in r])
    except Exception as e:
        print(t, 'err', repr(e))
con.close()
