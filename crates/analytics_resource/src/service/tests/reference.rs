//! 服务层用例分组。

use super::*;

/// reference 归档：本体不移动、源保留；登记 `file_rel_path`=源绝对路径 + `source_path`；
/// 指纹照算（源文件字节 sha256）。
#[tokio::test]
async fn t134_reference_archive_keeps_source_in_place_and_registers_metadata() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    // 源在项目外（reference 的典型场景：大文件 / 外部路径），但要绝对路径。
    let outside =
        std::env::temp_dir().join(format!("ref_src_{}.csv", uuid::Uuid::new_v4().simple()));
    fs::write(&outside, b"a,b\n1,2\n")
        .await
        .expect("write external source");

    let outcome = service
        .archive(reference_req(&outside, "data/big.csv"))
        .await
        .expect("reference archive");

    assert!(outcome.created_new_version);
    assert!(outside.is_file(), "reference：本体留原位，不搬走");
    assert!(
        !service
            .payload()
            .resources_dir()
            .join("data/big.csv")
            .exists(),
        "reference：不复制进 resources/"
    );

    let row = service
        .store()
        .get_resource_by_id(&outcome.resource_id)
        .await
        .expect("row");
    assert_eq!(row.payload_mode, "reference");
    assert_eq!(
        row.file_rel_path.as_deref(),
        Some(outside.to_string_lossy().replace('\\', "/").as_str()),
        "reference 的 file_rel_path = 源绝对路径（唯一索引防同一源重复归档）"
    );
    assert_eq!(
        row.source_path.as_deref(),
        Some(outside.to_string_lossy().replace('\\', "/").as_str()),
        "source_path 记录源绝对路径"
    );
    assert!(row.content_hash.is_some(), "指纹照算（源文件字节）");

    let _ = fs::remove_dir_all(&dir).await;
    let _ = fs::remove_file(&outside).await;
}

/// reference 取回：从**源**复制出可写工作副本（不是从 `resources/`）。
#[tokio::test]
async fn t135_reference_checkout_copies_from_source() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let outside =
        std::env::temp_dir().join(format!("ref_src_{}.csv", uuid::Uuid::new_v4().simple()));
    fs::write(&outside, b"a,b\n1,2\n")
        .await
        .expect("write external source");
    let archived = service
        .archive(reference_req(&outside, "data/big.csv"))
        .await
        .expect("reference archive");

    let dest = dir.join("scratchpad").join("big（工作副本）.csv");
    let checked = service
        .checkout(CheckoutRequest {
            resource_id: archived.resource_id.clone(),
            dest_path: dest.clone(),
        })
        .await
        .expect("reference checkout");

    assert_eq!(checked.dest_path, dest);
    assert_eq!(fs::read(&dest).await.expect("read"), b"a,b\n1,2\n");
    assert!(!service.payload().is_readonly(&dest), "工作副本必须可写");
    assert!(outside.is_file(), "源不受取回影响");

    let _ = fs::remove_dir_all(&dir).await;
    let _ = fs::remove_file(&outside).await;
}

/// reference 再归档：内容变了 → 版本 +1、指纹更新；源**不被覆盖**（本体在外部，M6 不动它）。
#[tokio::test]
async fn t136_reference_rearchive_updates_fingerprint_without_touching_source() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let outside =
        std::env::temp_dir().join(format!("ref_src_{}.csv", uuid::Uuid::new_v4().simple()));
    fs::write(&outside, b"a,b\n1,2\n")
        .await
        .expect("write source v1");
    let archived = service
        .archive(reference_req(&outside, "data/big.csv"))
        .await
        .expect("reference archive");
    assert_eq!(archived.version, 1);

    // 源内容被外部改掉（活文件）→ 再归档 = 新版本（指纹变了）。
    fs::write(&outside, b"a,b,c\n1,2,3\n")
        .await
        .expect("mutate source");
    let mut re = reference_req(&outside, "data/big.csv");
    re.existing_resource_id = Some(archived.resource_id.clone());
    let updated = service.archive(re).await.expect("reference re-archive");

    assert!(updated.created_new_version);
    assert_eq!(updated.version, 2);
    assert!(
        !service
            .payload()
            .resources_dir()
            .join("data/big.csv")
            .exists(),
        "reference 再归档不产生 resources/ 副本"
    );
    // 源内容保持（不是被“还原/覆盖”成什么别的）——M6 不动外部本体。
    assert_eq!(fs::read(&outside).await.expect("read"), b"a,b,c\n1,2,3\n");

    let _ = fs::remove_dir_all(&dir).await;
    let _ = fs::remove_file(&outside).await;
}

/// reference 撤销：只删登记行（源本来就在原位，没什么要搬回去）。
#[tokio::test]
async fn t137_reference_undo_drops_row_only() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let outside =
        std::env::temp_dir().join(format!("ref_src_{}.csv", uuid::Uuid::new_v4().simple()));
    fs::write(&outside, b"a,b\n1,2\n")
        .await
        .expect("write external source");
    let archived = service
        .archive(reference_req(&outside, "data/big.csv"))
        .await
        .expect("reference archive");

    service
        .undo_archive(&ArchiveUndo {
            resource_id: archived.resource_id.clone(),
            name: "big".to_string(),
            source_path: outside.clone(),
        })
        .await
        .expect("reference undo");

    assert!(outside.is_file(), "源不动");
    assert!(
        service
            .store()
            .get_resource_by_id(&archived.resource_id)
            .await
            .is_err(),
        "登记行已删"
    );

    let _ = fs::remove_dir_all(&dir).await;
    let _ = fs::remove_file(&outside).await;
}

/// reference 移入回收站（§12.1.1 ③）：本体留原位，只软删登记行；还原只复活登记行；
/// 永久删除只清登记行 + 版本元数据——全流程都不碰源文件（与 copy 同一进回收站判据）。
#[tokio::test]
async fn t138_reference_trash_cycles_without_touching_source() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let outside =
        std::env::temp_dir().join(format!("ref_src_{}.csv", uuid::Uuid::new_v4().simple()));
    fs::write(&outside, b"a,b\n1,2\n")
        .await
        .expect("write external source");
    let archived = service
        .archive(reference_req(&outside, "data/big.csv"))
        .await
        .expect("reference archive");
    let id = archived.resource_id.clone();

    // 1) 移入回收站：成功，本体不动，登记行进入软删态；条目带 ref: 前缀。
    let trashed = service
        .move_to_trash(&[id.clone()])
        .await
        .expect("reference 可进回收站（与 copy 同一判据）");
    let trash_id = trashed[0].trash_id.clone();
    assert!(
        trash_id.starts_with(crate::model::TRASH_ID_REF_PREFIX),
        "reference 回收站条目带 ref: 前缀"
    );
    assert!(outside.is_file(), "本体留原位");
    assert!(
        service
            .store()
            .get_resource_by_id(&id)
            .await
            .expect("row")
            .deleted_at
            .is_some(),
        "登记行进入软删态"
    );

    // 2) 还原：只复活登记行，本体仍在源位置。
    let restored = service
        .restore_archive_from_trash(&trash_id)
        .await
        .expect("reference 还原登记行");
    assert_eq!(restored.id, id);
    assert!(outside.is_file(), "本体仍在源位置");
    assert!(
        service
            .store()
            .get_resource_by_id(&id)
            .await
            .expect("row")
            .deleted_at
            .is_none(),
        "登记行复活"
    );

    // 3) 再次移入 → 永久删除：只清登记行与版本元数据，本体不动、登记行不再可查。
    let trashed_again = service
        .move_to_trash(&[id.clone()])
        .await
        .expect("reference 再次移入回收站");
    service
        .purge_archive(&trashed_again[0].trash_id)
        .await
        .expect("reference 永久删除只清登记行");
    assert!(outside.is_file(), "永久删除不碰本体");
    assert!(
        service.store().get_resource_by_id(&id).await.is_err(),
        "登记行已被清除"
    );

    let _ = fs::remove_dir_all(&dir).await;
    let _ = fs::remove_file(&outside).await;
}

/// 2026-11-12 A3 回归：同一外部源重复引用要在**预检阶段**给出可读提示，
/// 而不是漏到 020 的唯一索引、报一句裸 `insert` 错误。
/// 旧实现按 `req.rel_path`（相对）查重，库里却存源绝对路径，预检永远查不到。
#[tokio::test]
async fn t157_duplicate_reference_is_rejected_by_precheck() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let outside = std::env::temp_dir().join(format!(
        "ref_dup_{}.csv",
        uuid::Uuid::new_v4().simple()
    ));
    fs::write(&outside, b"a,b\n1,2\n")
        .await
        .expect("write external source");

    let first = service
        .archive(reference_req(&outside, "data/big.csv"))
        .await
        .expect("first reference archive");
    assert_eq!(
        first.file_rel_path,
        outside.to_string_lossy().replace('\\', "/"),
        "回执给的就是真实入库键"
    );

    // 换个相对路径也不该让用户以为能再登记一份引用。
    let reason = match service
        .archive(reference_req(&outside, "data/renamed.csv"))
        .await
    {
        Ok(_) => panic!("同一外部源不应被重复登记"),
        Err(error) => error.to_string(),
    };
    assert!(
        reason.contains("目标路径已被存档"),
        "预检要给出可读的冲突提示，实得：{reason}"
    );
    assert!(
        !reason.to_lowercase().contains("unique") && !reason.to_lowercase().contains("constraint"),
        "不该漏到数据库唯一索引：{reason}"
    );

    let _ = fs::remove_dir_all(&dir).await;
    let _ = fs::remove_file(&outside).await;
}
