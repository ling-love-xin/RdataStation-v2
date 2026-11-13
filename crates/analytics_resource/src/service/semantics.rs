//! 业务断言与效力的服务门面（薄委托：事务都在 store 层收口）。

use super::*;
use crate::model::{ArchiveAssertion, Lifecycle};

impl ArchiveService {
    /// 填写 / 修订业务断言。
    ///
    /// 空断言直接拒绝：留一条全空的记录只会让「有没有写口径」变得不可判断。
    /// 口径指纹变化时由 store 在同一事务里写 `SemanticChanged` 履历事件。
    pub async fn set_assertion(
        &self,
        resource_id: &str,
        assertion: ArchiveAssertion,
    ) -> Result<String, CoreError> {
        if assertion.is_empty() {
            return Err(service_err("assertion", "业务断言不能全空（至少填一项）"));
        }
        let resource = self.store.get_resource_by_id(resource_id).await?;
        let hash = self
            .store
            .set_assertion_with_event(resource_id, resource.version, &assertion)
            .await?;
        // 断言本身不涨内容版本：运行时广播只用于让详情面板刷新。
        self.emit(ChangeReason::Updated, Some(resource_id.to_string()));
        Ok(hash)
    }

    /// 读业务断言（`None` = 尚未填写）。
    pub async fn assertion(
        &self,
        resource_id: &str,
    ) -> Result<Option<ArchiveAssertion>, CoreError> {
        self.store.get_assertion(resource_id).await
    }

    /// 设置效力状态（认证 / 废弃 / 被取代 / 退回候选）。
    pub async fn set_lifecycle(
        &self,
        resource_id: &str,
        state: Lifecycle,
        superseded_by: Option<&str>,
        detail: Option<&str>,
    ) -> Result<(), CoreError> {
        let resource = self.store.get_resource_by_id(resource_id).await?;
        self.store
            .set_lifecycle_with_event(
                resource_id,
                resource.version,
                state,
                superseded_by,
                detail,
            )
            .await?;
        self.emit(ChangeReason::Updated, Some(resource_id.to_string()));
        Ok(())
    }

    /// 读效力状态（缺省 = 候选）。
    pub async fn lifecycle(&self, resource_id: &str) -> Result<Lifecycle, CoreError> {
        self.store.get_lifecycle(resource_id).await
    }
}
