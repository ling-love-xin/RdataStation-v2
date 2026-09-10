# -*- coding: utf-8 -*-
import pathlib
# engine driver/mod.rs re-export
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\driver\mod.rs")
t = p.read_text(encoding="utf-8")
if "pub use missing_driver::MissingDriver;" not in t:
    t = t.replace("pub mod missing_driver;", "pub mod missing_driver;\npub use missing_driver::MissingDriver;")
    p.write_text(t, encoding="utf-8")
    print("engine: re-export added")
else:
    print("engine: re-export already present")

# project Cargo.toml 加 rusqlite
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\project\Cargo.toml")
t = p.read_text(encoding="utf-8")
if "rusqlite" not in t:
    t = t.replace('tracing = "0.1.41"', 'tracing = "0.1.41"\nrusqlite = { version = "0.32.1", features = ["bundled", "chrono", "serde_json"] }')
    p.write_text(t, encoding="utf-8")
    print("project: rusqlite added")
else:
    print("project: rusqlite already present")
