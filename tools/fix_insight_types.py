# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\persistence\insight_types.rs")
t = p.read_text(encoding="utf-8")
t = t.replace(r"//! TODO(migration): 自 v1 \core/services/result_service.rs\ 抽取；",
              "//! TODO(migration): 自 v1 core/services/result_service.rs 抽取；")
t = t.replace("use serde::{Deserialize, Serialize};\n", "")
p.write_text(t, encoding="utf-8")
print("patched")
