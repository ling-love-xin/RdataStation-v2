# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\plugin\src\sidecar\manager.rs")
t = p.read_text(encoding="utf-8")
old = """                    _ = tokio::time::sleep(interval) => {
                        if let Some(port) = *port_clone.lock().unwrap_or_else(|e| e.into_inner()) {"""
new = """                    _ = tokio::time::sleep(interval) => {
                        let port_value = *port_clone.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(port) = port_value {"""
assert old in t
t = t.replace(old, new)
# config_clone unused -> 前缀下划线
t = t.replace("let config_clone = self.config.clone();", "let _config_clone = self.config.clone();")
p.write_text(t, encoding="utf-8")
print("manager.rs fixed")
