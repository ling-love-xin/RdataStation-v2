//! 检出、版本恢复与版本检出。

use super::*;
use crate::model::*;

impl ArchiveService {
    /// 草稿落点：相对草稿箱记短路径；落到草稿箱外就记绝对路径，别伪装成草稿箱条目。
    fn checkout_dest_rel(&self, dest: &std::path::Path) -> String {
        match dest.strip_prefix(self.payload.scratchpad_dir()) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => dest.to_string_lossy().into_owned(),
        }
    }

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

        // 文件资产优先按 manifest 物化：内容和 FileMeta 一起从 CAS 恢复；
        // 旧资产没有 manifest 时退回原来的 copy_out，元数据为 None。
        let file_meta = if let Some(version) = self
            .store
            .file_asset_version(&resource.id, resource.version)
            .await?
        {
            let manifest = self
                .payload
                .cas()
                .read_manifest(&version.manifest_id)
                .await?;
            self.payload
                .cas()
                .materialize_content(&manifest, &req.dest_path)
                .await?
        } else {
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
            None
        };

        // 取回是带 base_version 的租约，不是 OS 锁；同一资产同一 scope 只允许一条 active。
        // `dest_rel` 记草稿落点（相对草稿箱），不是资产本体路径：详情 / 再归档都靠它找草稿。
        let draft_rel = self.checkout_dest_rel(&req.dest_path);
        if let Err(error) = self
            .store
            .create_lease(&resource.id, "project", resource.version, &draft_rel)
            .await
        {
            // 工作副本没有租约就再归档无法校验 base：宁可删掉刚落的副本。
            if let Err(cleanup) = tokio::fs::remove_file(&req.dest_path).await {
                tracing::warn!(
                    error = %cleanup,
                    path = %req.dest_path.display(),
                    "取回失败后清理工作副本失败"
                );
            }
            return Err(error);
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
            file_meta,
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
        // 表型资产走另一条还原：本体是项目分析库里的一张表，**没有副本文件**，
        // 回退 = 把表名指回旧版本那张表（两张表都还在，不搬数据、不删新表）。
        if crate::ArchiveKind::resolve(&current.kind, current.analytics_table.as_deref())
            == crate::ArchiveKind::AnalyticsTable
        {
            return self.restore_table_version(&current, version).await;
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

        let target = self.store.file_asset_version(resource_id, version).await?.ok_or_else(|| {
            service_err(
                "restore",
                &format!("v{version} 没有 manifest：旧文件资产不再支持版本还原"),
            )
        })?;
        if target.content_pruned {
            return Err(service_err(
                "restore",
                &format!("v{version} 的内容已被标记裁剪，无法还原"),
            ));
        }
        let manifest = self.payload.cas().read_manifest(&target.manifest_id).await?;
        let entry = manifest.content_entry().ok_or_else(|| {
            service_err("restore", &format!("v{version} 的 manifest 缺少 Content"))
        })?;
        let copy_hash = manifest.content_hash.clone();
        let file_size = Some(entry.size as i64);
        // 幂等按**整版 manifest** 比，而不是只比字节：内容相同、元数据不同也是另一版。
        let current_manifest = self
            .store
            .file_asset_version(&current.id, current.version)
            .await?;
        let unchanged = match current_manifest.as_ref() {
            Some(current) => current.manifest_id == target.manifest_id,
            None => copy_hash == current.content_hash.clone().unwrap_or_default(),
        };
        if unchanged {
            // 幂等：要还原的就是当前这一版完整快照，什么都不做。
            return Ok(ArchiveOutcome {
                resource_id: current.id,
                version: current.version,
                content_hash: copy_hash,
                file_rel_path: rel_path,
                created_new_version: false,
                notes: Vec::new(),
            });
        }

        let snapshot = serde_json::to_string(&current).map_err(|e| {
            CoreError::storage(StorageError::Serialization {
                format: "JSON".to_string(),
                reason: e.to_string(),
            })
        })?;
        let temp = self.payload.project_root().join(".RSmeta/cas/staging").join(format!(
            "restore-{}.tmp",
            uuid::Uuid::new_v4().simple()
        ));
        self.payload
            .cas()
            .materialize_content(&manifest, &temp)
            .await?;
        let staged = self.payload.stage_payload(&temp, &rel_path).await;
        if let Err(error) = tokio::fs::remove_file(&temp).await {
            tracing::warn!(error = %error, path = %temp.display(), "清理还原临时物化文件失败");
        }
        let staged = staged?;

        let committed = self
            .store
            // 版本还原：只换内容与体积；分析表的定义 / 规模保持原位不动
            // （还原旧内容不等于把「这份东西是什么」改回去——那要走再归档）。
            .commit_new_file_version(
                resource_id,
                current.version,
                &snapshot,
                crate::store::file_asset::NewFileAssetVersion {
                    manifest_id: target.manifest_id.clone(),
                    content_hash: target.content_hash.clone(),
                    meta_hash: target.meta_hash.clone(),
                    parent_version: Some(current.version),
                },
                file_size,
                None,
                crate::model::TimelineEventKind::Restored,
                Some(&format!("从 v{version} 的 manifest 生成新版本")),
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

    /// 表型资产的还原：把本体表名指回快照里那张旧表，生成新版本（历史不动）。
    ///
    /// 与文件型还原的区别只有一点：**不碰任何数据**。旧表还在项目库里，回退只改
    /// 登记行的指向；当前版本那张表也原样留着（所以"回退"之后还能再回退回去）。
    async fn restore_table_version(
        &self,
        current: &AnalyticsResource,
        version: i32,
    ) -> Result<ArchiveOutcome, CoreError> {
        let versions = self.store.get_resource_versions(&current.id).await?;
        let snapshot = versions
            .iter()
            .find(|row| row.version == version)
            .ok_or_else(|| service_err("restore", &format!("找不到 v{version} 的版本快照")))?;
        let target = crate::table_asset::snapshot_table(&snapshot.snapshot).ok_or_else(|| {
            service_err(
                "restore",
                &format!("v{version} 的快照里没有本体表名，无法回退"),
            )
        })?;
        let hash = crate::table_asset::table_fingerprint(&target);
        // 幂等：要回退到的就是当前指的那张表，什么都不做（与文件型的指纹守卫同一口径）。
        if current.analytics_table.as_deref() == Some(target.as_str()) {
            return Ok(ArchiveOutcome {
                resource_id: current.id.clone(),
                version: current.version,
                content_hash: current.content_hash.clone().unwrap_or_default(),
                file_rel_path: String::new(),
                created_new_version: false,
                notes: Vec::new(),
            });
        }
        let snapshot_of_current = serde_json::to_string(current).map_err(|e| {
            CoreError::storage(StorageError::Serialization {
                format: "JSON".to_string(),
                reason: e.to_string(),
            })
        })?;
        let detail = format!("回退到 v{version} 的表 {target}");
        let updated = self
            .store
            .append_table_version(
                &current.id,
                current.version,
                &target,
                &snapshot_of_current,
                &hash,
                crate::model::TimelineEventKind::Restored,
                Some(&detail),
            )
            .await?;
        // 清单 / 详情面板靠这条广播刷新（与文件型还原同一个理由）。
        self.emit(ChangeReason::Restored, Some(updated.id.clone()));
        Ok(ArchiveOutcome {
            resource_id: updated.id,
            version: updated.version,
            content_hash: hash,
            file_rel_path: String::new(),
            created_new_version: true,
            notes: Vec::new(),
        })
    }

    /// 把某个历史版本的 manifest 物化到项目内部只读预览路径。
    ///
    /// 只给「用 vN 分析」这类只读查看用：返回 `None` = 该版本没有 manifest。
    /// 落点是确定的（同一版本重复查看覆盖同一文件），不占用用户可见目录。
    pub async fn version_file(
        &self,
        resource_id: &str,
        version: i32,
    ) -> Result<Option<std::path::PathBuf>, CoreError> {
        let Some(target) = self.store.file_asset_version(resource_id, version).await? else {
            return Ok(None);
        };
        if target.content_pruned {
            return Err(service_err(
                "version_file",
                &format!("v{version} 的内容已被标记裁剪，无法查看"),
            ));
        }
        let manifest = self.payload.cas().read_manifest(&target.manifest_id).await?;
        let entry = manifest.content_entry().ok_or_else(|| {
            service_err("version_file", "manifest 缺少 Content")
        })?;
        let name = std::path::Path::new(&entry.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("version.dat");
        let dest = self
            .payload
            .project_root()
            .join(".RSmeta/cas/materialized/versions")
            .join(resource_id)
            .join(format!("v{version}"))
            .join(name);
        if dest.exists() {
            tokio::fs::remove_file(&dest).await.map_err(|error| {
                service_err(
                    "version_file",
                    &format!("清理旧预览失败（{}）：{error}", dest.display()),
                )
            })?;
        }
        self.payload
            .cas()
            .materialize_content(&manifest, &dest)
            .await?;
        Ok(Some(dest))
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
        let target = self.store.file_asset_version(resource_id, version).await?.ok_or_else(|| {
            service_err(
                "checkout_version",
                &format!("v{version} 没有 manifest：旧文件资产不再支持历史取回"),
            )
        })?;
        if target.content_pruned {
            return Err(service_err(
                "checkout_version",
                &format!("v{version} 的内容已被标记裁剪，无法取回"),
            ));
        }
        let manifest = self.payload.cas().read_manifest(&target.manifest_id).await?;
        let file_meta = self
            .payload
            .cas()
            .materialize_content(&manifest, dest_path)
            .await?;
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
            file_meta,
        })
    }

}
