# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\src\secret.rs")
t = p.read_text(encoding="utf-8")
t = t.replace(
    "self.conn.execute_batch(&sql)?;",
    "self.conn\n            .execute_batch(&sql)\n            .map_err(|e| SecretError::Sql(e.to_string()))?;",
)
t = t.replace(
    "self.conn\n            .execute_batch(&format!(\"DROP SECRET IF EXISTS {name}\"))?;",
    "self.conn\n            .execute_batch(&format!(\"DROP SECRET IF EXISTS {name}\"))\n            .map_err(|e| SecretError::Sql(e.to_string()))?;",
)
p.write_text(t, encoding="utf-8")
print("map_err fixed")
