# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\plugin\src\sidecar\client.rs")
t = p.read_text(encoding="utf-8")
t = t.replace('format!("HTTP request failed: {}", e))?;', 'format!("HTTP request failed: {}", e)))?;')
t = t.replace('format!("Failed to parse response: {}", e))?;', 'format!("Failed to parse response: {}", e)))?;')
p.write_text(t, encoding="utf-8")
print("fixed 2 syntax bugs")
