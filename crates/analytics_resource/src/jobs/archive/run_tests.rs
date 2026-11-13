//! 取回凭据的端到端回归：真正跑「取回 → 改 → 再归档」，验证回到原存档升版且可回溯。
//!
//! 直接调用 `run_*_with_service`（注入同一份测试库），因此覆盖的是作业层的真实接线，
//! 而不是只测服务层的一个辅助函数。

use std::path::{Path, PathBuf};

use tokio::fs;

use super::*;
use crate::model::{ArchiveBinding, ArchiveKind, ArchiveRequest, CheckoutRequest, PayloadMode};
use crate::store::checkout_receipt::normalize_dest;
use crate::KeepVersions;

async fn env() -> (ArchiveService, PathBuf) {
    let (store, dir) = crate::tests::create_test_store().await;
    let service = ArchiveService::new(&dir, store).with_keep_versions(KeepVersions::All);
    (service, dir)
}

fn archive_job(root: &Path, source: PathBuf, rel: &str) -> ArchiveJob {
    ArchiveJob {
        project_root: root.to_path_buf(),
        read_only: false,
        request: ArchiveRequest {
            source_path: source,
            rel_path: rel.to_string(),
            name: "dau".to_string(),
            alias: None,
            kind: ArchiveKind::File,
            payload_mode: PayloadMode::Copy,
            analysis: None,
            binding: ArchiveBinding::default(),
            tags: Vec::new(),
            group_id: None,
            keep_versions: None,
            // 关键：UI 这一侧仍然是 None——是作业层反查凭据后才决定升版。
            existing_resource_id: None,
        },
        keep_versions: KeepVersions::All,
        analysis_probe: false,
        assertion: None,
    }
}

fn checkout_job(root: &Path, resource_id: &str, dest: PathBuf) -> CheckoutJob {
    CheckoutJob {
        project_root: root.to_path_buf(),
        read_only: false,
        request: CheckoutRequest {
            resource_id: resource_id.to_string(),
            dest_path: dest,
        },
        open_after: false,
    }
}

fn archived(outcome: OpOutcome) -> (String, i32, Option<crate::ArchiveUndo>, Vec<String>) {
    match outcome {
        OpOutcome::Archived {
            resource_id,
            version,
            undo,
            notes,
            ..
        } => (resource_id, version, undo, notes),
        _ => panic!("期望归档成功"),
    }
}

/// 全链路：首次归档 v1 → 取回（写凭据）→ 改工作副本 → 再归档回原档 v2。
#[tokio::test]
async fn t164_checkout_edit_rearchive_returns_to_original_version() {
    let (service, dir) = env().await;
    let draft = dir.join("draft/dau.sql");
    fs::create_dir_all(draft.parent().unwrap())
        .await
        .expect("draft dir");
    fs::write(&draft, b"select 1;\n").await.expect("draft 1");

    let first_job = archive_job(&dir, draft.clone(), "reports/dau.sql");
    let (id, v1, undo, _) = archived(run_archive_with_service(&first_job, &service).await);
    assert_eq!(v1, 1);
    assert!(undo.is_some(), "首次归档要给撤销凭据");

    let dest = dir.join("scratchpad/dau.sql");
    fs::create_dir_all(dest.parent().unwrap())
        .await
        .expect("scratchpad dir");
    let checkout = run_checkout_with_service(&checkout_job(&dir, &id, dest.clone()), &service).await;
    let checked_out_dest = match checkout {
        OpOutcome::CheckedOut { dest, version, .. } => {
            assert_eq!(version, 1);
            dest
        }
        _ => panic!("期望取回成功"),
    };

    let receipt = service
        .store()
        .find_checkout(&checked_out_dest)
        .await
        .expect("find receipt")
        .expect("取回必须留下凭据");
    assert_eq!(receipt.resource_id, id);
    assert_eq!(receipt.from_version, 1);
    assert!(receipt.consumed_at.is_none(), "还没再归档");

    fs::write(&checked_out_dest, b"select 2;\n")
        .await
        .expect("edit working copy");
    // 故意换一个 rel_path：模拟 UI 走「新建归档」，服务层应因命中凭据而忽略它。
    let second_job = archive_job(&dir, checked_out_dest.clone(), "reports/dau-renamed.sql");
    let (id2, v2, undo2, notes) = archived(run_archive_with_service(&second_job, &service).await);

    assert_eq!(id2, id, "必须回到原存档，不能新建一条");
    assert_eq!(v2, 2, "内容变了应升到 v2");
    assert!(undo2.is_none(), "再归档不能给撤销凭据（否则会把原档删掉）");
    assert!(
        notes.iter().any(|note| note.contains("回到原存档升版")),
        "回执要说明这是升版：{notes:?}"
    );

    // 旧内容留在版本快照、索引指纹与本体一致。
    let versions = service
        .store()
        .get_resource_versions(&id)
        .await
        .expect("versions");
    assert_eq!(versions.len(), 1, "v1 的写前快照应留下");
    let current = service.store().get_resource_by_id(&id).await.expect("row");
    let actual = service
        .payload()
        .content_hash(&service.payload().resolve(current.file_rel_path.as_deref().unwrap()).unwrap())
        .await
        .expect("hash body");
    assert_eq!(current.content_hash.as_deref(), Some(actual.as_str()));
    assert_eq!(std::fs::read_to_string(actual_path(&service, &current.file_rel_path)).unwrap(), "select 2;\n");

    // 凭据结案 + 回溯链可查。
    let consumed = service
        .store()
        .find_checkout(&checked_out_dest)
        .await
        .expect("find")
        .expect("receipt");
    assert_eq!(consumed.consumed_version, Some(2));
    assert_eq!(consumed.from_version, 1, "回溯起点不能被改写");
    assert_eq!(
        service
            .store()
            .checkouts_for_resource(&id)
            .await
            .expect("list")
            .len(),
        1
    );
    let events = service
        .store()
        .get_resource_events(&id)
        .await
        .expect("events");
    assert!(
        events
            .iter()
            .any(|e| e.kind == crate::model::TimelineEventKind::Updated),
        "升版要写 Updated 履历"
    );

    let _ = fs::remove_dir_all(&dir).await;
}

fn actual_path(service: &ArchiveService, rel: &Option<String>) -> PathBuf {
    service
        .payload()
        .resolve(rel.as_deref().unwrap())
        .expect("resolve")
}

/// 没有凭据的普通草稿仍走「新建」，这是不能破坏的既有语义。
#[tokio::test]
async fn t165_without_receipt_still_creates_a_new_archive() {
    let (service, dir) = env().await;
    fs::create_dir_all(dir.join("draft")).await.expect("draft dir");
    let a = dir.join("draft/a.sql");
    let b = dir.join("draft/b.sql");
    fs::write(&a, b"select 1;").await.expect("a");
    fs::write(&b, b"select 2;").await.expect("b");

    let (id_a, _, undo_a, _) =
        archived(run_archive_with_service(&archive_job(&dir, a, "a.sql"), &service).await);
    let (id_b, v_b, undo_b, notes) =
        archived(run_archive_with_service(&archive_job(&dir, b, "b.sql"), &service).await);
    assert_ne!(id_a, id_b, "无凭据时必须是两条独立存档");
    assert_eq!(v_b, 1);
    assert!(undo_a.is_some() && undo_b.is_some());
    assert!(notes.is_empty(), "新建不该带升版说明：{notes:?}");

    let _ = fs::remove_dir_all(&dir).await;
}

/// 源存档已进回收站：忽略凭据，别把一次正常的新建归档拖成失败。
#[tokio::test]
async fn t166_receipt_pointing_at_deleted_archive_is_ignored() {
    let (service, dir) = env().await;
    fs::create_dir_all(dir.join("draft")).await.expect("draft dir");
    let draft = dir.join("draft/a.sql");
    fs::write(&draft, b"select 1;").await.expect("draft");
    let (id, _, _, _) =
        archived(run_archive_with_service(&archive_job(&dir, draft, "a.sql"), &service).await);

    let dest = dir.join("scratchpad/a.sql");
    fs::create_dir_all(dest.parent().unwrap()).await.expect("dir");
    service
        .store()
        .record_checkout(&dest, &id, 1)
        .await
        .expect("receipt");
    service
        .store()
        .soft_delete_archive(&id)
        .await
        .expect("soft delete");

    fs::write(&dest, b"select 2;").await.expect("edit");
    let (new_id, version, undo, _) =
        archived(run_archive_with_service(&archive_job(&dir, dest, "b.sql"), &service).await);
    assert_ne!(new_id, id, "软删后应新建，而不是复活旧档");
    assert_eq!(version, 1);
    assert!(undo.is_some());

    let _ = fs::remove_dir_all(&dir).await;
}

/// 路径规范化与重复取回覆盖：Windows 反斜杠不能让凭据查不到。
#[tokio::test]
async fn t167_receipt_paths_are_normalized_and_recheckout_overwrites() {
    let (service, dir) = env().await;
    let dest = dir.join("scratchpad/dau.sql");
    let windows_style = PathBuf::from(dest.to_string_lossy().replace('/', "\\"));
    service
        .store()
        .record_checkout(&dest, "ar_1", 1)
        .await
        .expect("record");
    let found = service
        .store()
        .find_checkout(&windows_style)
        .await
        .expect("find")
        .expect("两种写法应命中同一条");
    assert_eq!(found.dest_path, normalize_dest(&dest));
    assert_eq!(found.from_version, 1);

    service
        .store()
        .record_checkout(&dest, "ar_1", 3)
        .await
        .expect("re-checkout");
    let again = service
        .store()
        .find_checkout(&dest)
        .await
        .expect("find")
        .expect("receipt");
    assert_eq!(again.from_version, 3, "重复取回应覆盖为最新版本");
    assert!(again.consumed_at.is_none(), "覆盖后应重置结案状态");

    let _ = fs::remove_dir_all(&dir).await;
}

/// reference 档的本体在外部源：不能凭「取回得到的草稿」给原档升版，
/// 否则索引指纹指向草稿、source_path 却仍指外部源，两者会错位。
#[tokio::test]
async fn t168_reference_archive_does_not_rearchive_from_draft() {
    let (service, dir) = env().await;
    let outside = dir.join("outside/big.csv");
    fs::create_dir_all(outside.parent().unwrap())
        .await
        .expect("outside dir");
    fs::write(&outside, b"a,b\n1,2\n").await.expect("outside");

    let mut first = archive_job(&dir, outside, "data/big.csv");
    first.request.payload_mode = PayloadMode::Reference;
    let (id, _, _, _) = archived(run_archive_with_service(&first, &service).await);

    let draft = dir.join("scratchpad/big.csv");
    fs::create_dir_all(draft.parent().unwrap())
        .await
        .expect("scratchpad dir");
    fs::write(&draft, b"a,b\n9,9\n").await.expect("draft");
    service
        .store()
        .record_checkout(&draft, &id, 1)
        .await
        .expect("receipt");

    let (new_id, version, undo, _) = archived(
        run_archive_with_service(&archive_job(&dir, draft, "data/big-2.csv"), &service).await,
    );
    assert_ne!(new_id, id, "reference 档应忽略草稿凭据");
    assert_eq!(version, 1);
    assert!(undo.is_some(), "这次是新建，应给撤销凭据");

    let _ = fs::remove_dir_all(&dir).await;
}
