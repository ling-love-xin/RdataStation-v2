# -*- coding: utf-8 -*-
"""Round 24：loader 加 delete_connection / delete_connection_at。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\workspace_loader.rs')
t = p.read_text(encoding='utf-8')

# 找到文件末尾的 save_connection_at 结束位置，在其后追加删除方法
tail_anchor = '''    let dir = default_global_dir();
    save_connection_at(&dir, name, db_type, url, username, password)
}'''
assert tail_anchor in t

add = '''
/// 删除指定目录中的全局连接（目录可注入，便于测试）。
///
/// 成功返回 `Ok(())`，失败返回 `Err(中文提示)`。
pub fn delete_connection_at(dir: &Path, conn_id: &str) -> Result<(), String> {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => return Err(format!("无法启动异步运行时: {e}")),
    };

    let sqlite = dir.join("global.db");
    let duckdb = dir.join("global.duckdb");

    runtime.block_on(async {
        let manager = GlobalDatabaseManager::new(sqlite, duckdb, 2).await?;
        manager
            .delete_global_connection(conn_id)
            .await
            .map_err(|e| format!("删除连接失败: {e}"))
    })
}

/// 删除默认全局目录中的连接。
pub fn delete_connection(conn_id: &str) -> Result<(), String> {
    let dir = default_global_dir();
    delete_connection_at(&dir, conn_id)
}
'''
t = t.replace(tail_anchor, tail_anchor + add)
p.write_text(t, encoding='utf-8')
print('loader delete added')
