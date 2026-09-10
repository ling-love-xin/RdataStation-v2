# -*- coding: utf-8 -*-
"""Round 24 修复：E0382（entity 克隆）+ E0277（loader 错误转换）。"""
import pathlib

# ---- 1. panels.rs：详情块开头克隆 entity（删除闭包 move 克隆，原值留给新建按钮闭包）----
p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')
old = '''            let shared = self.shared.clone();
            let selected_id = item.id.clone();
            let selected_name = item.name.clone();'''
new = '''            let shared = self.shared.clone();
            let entity = entity.clone();
            let selected_id = item.id.clone();
            let selected_name = item.name.clone();'''
assert t.count(old) == 1
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')

# ---- 2. workspace_loader.rs：delete_connection_at 内 match 代替 ? ----
w = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\workspace_loader.rs')
t2 = w.read_text(encoding='utf-8')
old2 = '''    runtime.block_on(async {
        let manager = GlobalDatabaseManager::new(sqlite, duckdb, 2).await?;
        manager
            .delete_global_connection(conn_id)
            .await
            .map_err(|e| format!("删除连接失败: {e}"))
    })
}'''
new2 = '''    runtime.block_on(async {
        match GlobalDatabaseManager::new(sqlite, duckdb, 2).await {
            Ok(manager) => manager
                .delete_global_connection(conn_id)
                .await
                .map_err(|e| format!("删除连接失败: {e}")),
            Err(e) => Err(format!("初始化全局库失败: {e}")),
        }
    })
}'''
assert t2.count(old2) == 1, 'loader anchor count %d' % t2.count(old2)
t2 = t2.replace(old2, new2)
w.write_text(t2, encoding='utf-8')
print('fixed E0382 + E0277')
