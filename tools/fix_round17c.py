# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\secret_integration.rs")
t = p.read_text(encoding="utf-8")

old = """/// Secret 名称仅保留安全字符（DuckDB 标识符）
fn sanitize_secret_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "conn".to_string()
    } else {
        cleaned
    }
}"""
new = """/// Secret 名称净化（DuckDB 标识符：字母/数字/下划线，连字符等其他字符转下划线）
fn sanitize_secret_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "conn".to_string()
    } else {
        cleaned
    }
}"""
assert old in t
t = t.replace(old, new)

old2 = """        assert_eq!(sanitize_secret_name("conn-001"), "conn-001");"""
new2 = """        assert_eq!(sanitize_secret_name("conn-001"), "conn_001");"""
assert old2 in t
t = t.replace(old2, new2)

p.write_text(t, encoding="utf-8")
print("sanitize fixed")
