# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\connection_service.rs")
t = p.read_text(encoding="utf-8")

# 1) hook：add_connection 成功后注册 Secret（联邦加速）
old = """        // 添加到连接管理器
        self.manager
            .add_connection(conn_id.clone(), Arc::clone(&db), info, driver_config)
            .await?;

        // NOTE: 元数据缓存不在连接时立即创建，改为懒加载。
        // 首次查询 schema / table / column 时通过 L2 cache write 路径自动创建。"""
new = """        // 添加到连接管理器
        self.manager
            .add_connection(conn_id.clone(), Arc::clone(&db), info, driver_config)
            .await?;

        // M3 本地加速：连接建立成功后注册 DuckDB Secret（联邦查询直连源库）。
        // 失败仅告警，不影响连接本身（加速是增强而非依赖）。
        crate::services::secret_integration::ensure_secret_registered(&conn_id, db_type, &url);

        // NOTE: 元数据缓存不在连接时立即创建，改为懒加载。
        // 首次查询 schema / table / column 时通过 L2 cache write 路径自动创建。"""
assert old in t
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("hook inserted")
