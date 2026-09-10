# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\global\016_add_driver_properties.sql")
t = p.read_text(encoding="utf-8")
old = """-- driver_properties 存储 JSON 键值对，格式：{"key":"value", ...}
-- 前端在新建连接时从该字段获取默认属性填充 DriverPropsTab

ALTER TABLE drivers ADD COLUMN driver_properties TEXT;
"""
new = """-- driver_properties 存储 JSON 键值对，格式：{"key":"value", ...}
-- 前端在新建连接时从该字段获取默认属性填充 DriverPropsTab
--
-- 注意：drivers 表由 008_add_data_source_module.sql 建表时已含 driver_properties 列，
-- 此处不再 ALTER（避免 duplicate column），仅做 capabilities / 默认属性数据补充。
"""
assert old in t
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("016 fixed")
