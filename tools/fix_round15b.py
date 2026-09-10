# -*- coding: utf-8 -*-
import pathlib

s = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\project\src\store.rs")
t = s.read_text(encoding="utf-8")

# 1) create 补 queries 目录（v1 原缺陷：dirs 数组缺 queries，但测试与文档声明其存在）
old = """        let dirs = [
            meta_dir.clone(),
            meta_dir.join(PROJECT_METADATA_DIR_NAME),
            meta_dir.join("config"),
        ];"""
new = """        let dirs = [
            meta_dir.clone(),
            meta_dir.join(PROJECT_METADATA_DIR_NAME),
            meta_dir.join("config"),
            meta_dir.join("queries"),
        ];"""
assert old in t, "dirs block not found"
t = t.replace(old, new)

# 2) 共享测试 INSERT 用真实列（connections 无 kind 列）
old2 = """                "INSERT INTO connections(id, name, kind) VALUES ('conn-global-001', '供应商主数据', 'duckdb')","""
new2 = """                "INSERT INTO connections(id, name, db_type) VALUES ('conn-global-001', '供应商主数据', 'duckdb')","""
assert old2 in t, "insert stmt not found"
t = t.replace(old2, new2)

s.write_text(t, encoding="utf-8")
print("store.rs fixed (queries dir + insert columns)")
