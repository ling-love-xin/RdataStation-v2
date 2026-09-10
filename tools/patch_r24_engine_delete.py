# -*- coding: utf-8 -*-
"""Round 24：engine 加 delete_global_connection（脚本文件版）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\persistence\global_db.rs')
t = p.read_text(encoding='utf-8')

anchor = '    /// 保存导航器状态\n    ///'
add = (
    '    /// 删除全局连接（元数据物理删除）。\n'
    '    ///\n'
    '    /// 注意：`get_global_connections` 仅返回 `is_active = 1` 的连接；\n'
    '    /// 删除后该连接从工作台列表消失。若需"隐藏而非删除"，可将 `is_active` 置 0。\n'
    '    pub async fn delete_global_connection(&self, conn_id: &str) -> Result<(), CoreError> {\n'
    '        let conn = self.sqlite_pool.acquire().await?;\n'
    '        conn.inner()?\n'
    '            .execute("DELETE FROM global_connections WHERE id = ?1", [conn_id])\n'
    '            .map_err(|e| {\n'
    '                CoreError::storage(StorageError::Persistence {\n'
    '                    store: "sqlite".to_string(),\n'
    '                    operation: "delete_global_connection".to_string(),\n'
    '                    reason: e.to_string(),\n'
    '                })\n'
    '            })?;\n'
    '        tracing::info!(conn_id, "全局连接已删除");\n'
    '        Ok(())\n'
    '    }\n'
    '\n'
)
assert t.count(anchor) == 1, 'anchor count %d' % t.count(anchor)
t = t.replace(anchor, add + anchor)
p.write_text(t, encoding='utf-8')
print('engine delete added, lines now', len(t.splitlines()))
