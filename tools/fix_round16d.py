# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\src\known_hosts.rs")
t = p.read_text(encoding="utf-8")
old = """            let hosts = parts[0];
            let key_b64 = parts[2].split_whitespace().next().unwrap_or("");

            let public_key = match PublicKey::from_openssh(key_b64) {"""
new = """            let hosts = parts[0];
            // known_hosts 真实格式：`host keytype base64key`（keytype 与 key 需合并还原完整 OpenSSH 格式）
            let key_openssh = parts[1..]
                .join(" ")
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ");

            let public_key = match PublicKey::from_openssh(&key_openssh) {"""
assert old in t
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("known_hosts parse fixed")
