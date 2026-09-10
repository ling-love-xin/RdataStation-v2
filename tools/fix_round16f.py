# -*- coding: utf-8 -*-
import pathlib

# 1) secret 断言 POSTGRES
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\src\secret.rs")
t = p.read_text(encoding="utf-8")
t = t.replace(
    'assert_eq!(list[0].secret_type.to_uppercase(), "DUCKDB");',
    'assert_eq!(list[0].secret_type.to_uppercase(), "POSTGRES");',
)
p.write_text(t, encoding="utf-8")
print("secret assert fixed")

# 2) known_hosts parse 支持 2 段/3 段
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\src\known_hosts.rs")
t = p.read_text(encoding="utf-8")
old = """            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() < 3 {
                continue;
            }

            let hosts = parts[0];
            // known_hosts 真实格式：`host keytype base64key`（keytype 与 key 需合并还原完整 OpenSSH 格式）
            let key_openssh = parts[1..]
                .join(" ")
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ");

            let public_key = match PublicKey::from_openssh(&key_openssh) {"""
new = """            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            let hosts = parts[0];
            // known_hosts 两种格式均支持：
            // - 完整格式：`host keytype base64key`（ssh-ed25519 AAAA...）
            // - 简写格式：`host base64key`
            let key_openssh = parts[1..].join(" ");

            let public_key = match PublicKey::from_openssh(&key_openssh) {"""
assert old in t
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("known_hosts 2/3-seg parse fixed")
