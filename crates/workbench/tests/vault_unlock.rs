//! 启动解锁门禁（`workbench::unlock`）的口径测试。
//!
//! 只钉「什么时候**不**该拦人」：默认机器绑定档下，任何启动都不该弹解锁窗。
//! 「该拦」的分支要造 master 档文件（`data/vault-master-key`），属于 `rds-shared`
//! 的 `vault_source` 单测范围，不在窗口层重复造钥匙。

use rds_workbench::unlock;

#[test]
fn machine_source_needs_no_unlock() {
    // 测试数据根是隔离目录（`paths` 的 test-support），默认无 `vault-source.json` → 机器绑定档。
    assert!(!unlock::needs_unlock(), "默认档位下不应弹出启动解锁窗");
}
