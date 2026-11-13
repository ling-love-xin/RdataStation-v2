//! 检出、版本恢复与版本检出。

use super::*;
use crate::model::*;

impl ArchiveService {
    /// 取回（检出）：把本体复制成可写工作副本，本体不动。
    pub async fn checkout(&self, req: CheckoutRequest) -> Result<CheckoutOutcome, CoreError> {
        // 目标不得落在 resources/ 内：那等于绕开只读守卫写本体。
        if req.dest_path.starts_with(self.payload.resources_dir()) {
            return Err(service_err(
                "checkout",
                "取回目标不能位于 resources/ 内（那是归档本体目录）",
            ));
        }

        let resource = self.store.get_resource_by_id(&req.resource_id).await?;
        let rel_path = resource
            .file_rel_path
            .clone()
            .ok_or_else(|| service_err("checkout", "该存档没有本体（非文件型或旧行）"))?;

        match PayloadMode::from_db_str(&resource.payload_mode) {
            // copy：从 `resources/` 副本复制（本体不动、仍只读）。
            PayloadMode::Copy => self.payload.copy_out(&rel_path, &req.dest_path).await?,
            // reference：从源绝对路径复制（本体在源位置，M6 不动它）。
            PayloadMode::Reference => {
                let source = self.payload_path_for(&resource)?;
                self.payload
                    .copy_out_source(&source, &req.dest_path)
                    .await?;
            }
        }
        self.emit(ChangeReason::CheckedOut, Some(resource.id.clone()));
        self.record_timeline(
            &resource.id,
            Some(resource.version),
            crate::model::TimelineEventKind::CheckedOut,
            Some(&format!("取回为工作副本：{}", req.dest_path.display())),
            None,
        )
        .await;

        Ok(CheckoutOutcome {
            resource_id: resource.id,
            version: resource.version,
            dest_path: req.dest_path,
        })
    }

    /// 还原到某个历史版本：**用旧内容生成新版本**，不覆盖历史（原型 §4.3）。
    ///
    /// 与 `archive_into_existing` 同构（写前快照 → 覆盖本体 → 索引 +1），只是内容来源从
    /// “工作区文件”换成“历史副本”；两条守卫：
    ///
    /// - 该版本没有内容副本（被保留策略裁掉 / 从未生成）→ 拒绝，并说清“副本没了”；
    /// - 副本内容与当前内容指纹相同 → 幂等返回，不产生无意义的新版本（架构 §5.1）。
    pub async fn restore_version(
        &self,
        resource_id: &str,
        version: i32,
    ) -> Result<ArchiveOutcome, CoreError> {
        let current = self.store.get_resource_by_id(resource_id).await?;
        if current.deleted_at.is_some() {
            return Err(service_err("restore", "存档已删除，无法还原"));
        }
        // reference 模式的本体在源位置，不归 M6 管辖——"还原为旧版本"意味着覆盖源文件，
        // 那可能正是用户工作区里在用的活文件，风险不可控；M6 定位"留存 + 版本"不做覆盖外部源。
        // 出口：取回该版本为草稿（`checkout_version`），需要旧内容时从历史副本拿。
        if PayloadMode::from_db_str(&current.payload_mode) == PayloadMode::Reference {
            return Err(service_err(
                "restore",
                "引用型存档的本体在源位置，不支持覆盖还原；请用「取回该版本为草稿」",
            ));
        }
        let rel_path = current
            .file_rel_path
            .clone()
            .ok_or_else(|| service_err("restore", "该存档没有本体路径（非文件型或旧行）"))?;

        let copy = self
            .payload
            .version_copy_file(resource_id, version)
            .await?
            .ok_or_else(|| {
                service_err(
                    "restore",
                    &format!("v{version} 没有内容副本（可能已被保留策略裁剪），无法还原"),
                )
            })?;
        let copy_hash = self.payload.content_hash(&copy).await?;
        // 还原后的体积就是副本的体积（写回后本体与副本同内容）。
        let file_size = self.payload.file_size(&copy).await?;
        if copy_hash == current.content_hash.clone().unwrap_or_default() {
            // 幂等：要还原的内容就是当前内容，什么都不做（与再归档的指纹守卫同一口径）。
            return Ok(ArchiveOutcome {
                resource_id: current.id,
                version: current.version,
                content_hash: copy_hash,
                file_rel_path: rel_path,
                created_new_version: false,
                notes: Vec::new(),
            });
        }

        // 与再归档同一顺序（P0-1）：旧内容留副本 → 历史副本进暂存 → 索引事务 → 替换本体。
        if self
            .payload
            .store_version_copy(resource_id, current.version, &rel_path)
            .await?
            .is_none()
        {
            return Err(service_err(
                "restore",
                "当前内容未能留成历史副本，已中止还原以免丢失当前版本",
            ));
        }
        let snapshot = serde_json::to_string(&current).map_err(|e| {
            CoreError::storage(StorageError::Serialization {
                format: "JSON".to_string(),
                reason: e.to_string(),
            })
        })?;
        // `stage_payload` 会重新检查副本是否还在：等价于原来「副本在查到与写回之间消失」的守卫。
        let staged = self.payload.stage_payload(&copy, &rel_path).await?;

        let committed = self
            .store
            // 版本还原：只换内容与体积；分析表的定义 / 规模保持原位不动
            // （还原旧内容不等于把「这份东西是什么」改回去——那要走再归档）。
            .commit_new_version(
                resource_id,
                current.version,
                &snapshot,
                &copy_hash,
                file_size,
                None,
                crate::model::TimelineEventKind::Restored,
                Some(&format!("从 v{version} 的副本生成新版本")),
            )
            .await;
        let updated = match committed {
            Ok(value) => value,
            Err(error) => {
                self.payload.discard_staged(&staged).await;
                return Err(error);
            }
        };

        if let Err(error) = self.payload.commit_staged(&staged, &rel_path).await {
            tracing::error!(
                error = %error,
                staged = %staged.display(),
                resource_id,
                "历史还原：索引已提交但本体替换失败，暂存文件保留待恢复"
            );
            self.record_timeline(
                resource_id,
                Some(updated.version),
                crate::model::TimelineEventKind::Validated,
                Some(&format!("还原内容未落地，暂存文件保留：{}", staged.display())),
                Some(&copy_hash),
            )
            .await;
            return Err(error);
        }
        self.payload.discard_staged(&staged).await;

        self.prune_copies(resource_id, None).await;
        self.emit(ChangeReason::Restored, Some(resource_id.to_string()));

        Ok(ArchiveOutcome {
            resource_id: updated.id,
            version: updated.version,
            content_hash: copy_hash,
            file_rel_path: rel_path,
            created_new_version: true,
            // 还原不带标签 / 分组：那两项是归档入口的事。
            notes: Vec::new(),
        })
    }

    /// 取回某个**历史版本**为工作副本（版本历史对话框的“取回该版本为草稿”）。
    ///
    /// 与 `checkout` 同一语义，只是源换成历史副本（本体不动）。
    pub async fn checkout_version(
        &self,
        resource_id: &str,
        version: i32,
        dest_path: &std::path::Path,
    ) -> Result<CheckoutOutcome, CoreError> {
        if dest_path.starts_with(self.payload.resources_dir()) {
            return Err(service_err(
                "checkout_version",
                "取回目标不能位于 resources/ 内（那是归档本体目录）",
            ));
        }
        let resource = self.store.get_resource_by_id(resource_id).await?;
        // 本体异常（缺失 / 内容已变）不影响历史副本：能从历史取回就让它取。
        if !self
            .payload
            .copy_version_out(resource_id, version, dest_path)
            .await?
        {
            return Err(service_err(
                "checkout_version",
                &format!("v{version} 没有内容副本（可能已被保留策略裁剪），无法取回"),
            ));
        }
        self.emit(ChangeReason::CheckedOut, Some(resource.id.clone()));
        self.record_timeline(
            &resource.id,
            Some(version),
            crate::model::TimelineEventKind::CheckedOut,
            Some(&format!("取回 v{version} 的内容副本为草稿")),
            None,
        )
        .await;

        Ok(CheckoutOutcome {
            resource_id: resource.id,
            version,
            dest_path: dest_path.to_path_buf(),
        })
    }

}
