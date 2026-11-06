//! 服务层用例分组。

use super::*;

#[tokio::test]
async fn t104_checkout_copies_payload_out_writable() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let mut events = service.subscribe();
    let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
    let outcome = service
        .archive(archive_req(&draft, "dau.sql", None))
        .await
        .expect("archive");
    let _ = events.recv().await;

    let dest = dir.join("scratchpad").join("dau（工作副本）.sql");
    let checkout = service
        .checkout(CheckoutRequest {
            resource_id: outcome.resource_id.clone(),
            dest_path: dest.clone(),
        })
        .await
        .expect("checkout");

    assert_eq!(checkout.version, 1);
    assert!(dest.is_file());
    assert!(!service.payload().is_readonly(&dest), "工作副本必须可写");
    assert!(
        service.payload().resources_dir().join("dau.sql").is_file(),
        "取回不得移动本体"
    );
    let event = events.recv().await.expect("event");
    assert_eq!(event.reason, ChangeReason::CheckedOut);

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t107_checkout_rejects_target_inside_resources() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
    let outcome = service
        .archive(archive_req(&draft, "dau.sql", None))
        .await
        .expect("archive");

    let inside = service.payload().resources_dir().join("escape.sql");
    assert!(
        service
            .checkout(CheckoutRequest {
                resource_id: outcome.resource_id,
                dest_path: inside,
            })
            .await
            .is_err(),
        "取回目标落在 resources/ 内必须被拒（那等于绕开只读守卫）"
    );

    let _ = fs::remove_dir_all(&dir).await;
}
