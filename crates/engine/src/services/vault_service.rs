//! 密钥来源（三档）的**壳层入口**：读状态、切档。
//!
//! 落到两处已有实现上，不另造一套：
//! - 档位与密钥推导：`shared::vault_source`（machine / os / master）；
//! - 切档重加密编排：`store::persistence::vault_switch::switch_source`（项目库先、
//!   全局库后、档位文件最后）。
//!
//! 本模块只补「UI 需要一次问全的事实」：当前档位、master 是否已建立 / 已解锁、平台是否
//! 支持系统凭据档。**不在这里做解锁交互**——启动时解锁由壳层决定（见原型 ⑧）。

use shared::error::{CommonError, CoreError};
use shared::vault_source::{self, VaultSource};

pub use crate::persistence::vault_switch::SwitchReport;

/// 密钥页需要的全部状态（一次拿全，渲染路径不再查盘）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultStatus {
    pub source: VaultSource,
    /// master 档的密钥库是否已建立（没建立 = 切过去就是「新建主密码」）
    pub master_initialized: bool,
    /// master 钥是否已在本进程解锁（未解锁时加密落库会被拒，见 `active_key`）
    pub master_unlocked: bool,
    /// 当前平台能否用「系统凭据」档（非 Windows 返回 false，切过去会明确报 NotSupported）
    pub os_supported: bool,
}

/// 读状态（纯本地文件读取，可在 UI 线程直接调用）。
pub fn status() -> VaultStatus {
    VaultStatus {
        source: vault_source::current_source(),
        master_initialized: vault_source::master_initialized(),
        master_unlocked: vault_source::master_unlocked(),
        os_supported: cfg!(windows),
    }
}

/// 切档：把三张表（认证 / 网络 / 网络资源）的密文从旧钥换成目标档的钥。
///
/// 失败时档位文件不变（旧钥仍能解开全部密文），错误原样上抛给 UI 显示。
pub async fn switch_source(
    target: VaultSource,
    master_password: Option<&str>,
) -> Result<SwitchReport, CoreError> {
    let Some(gdb) = crate::migration::get_global_db_manager() else {
        return Err(CoreError::common(CommonError::Internal(
            "全局库未初始化：切档需要在启动完成后进行".to_string(),
        )));
    };
    crate::persistence::vault_switch::switch_source(gdb, target, master_password).await
}
