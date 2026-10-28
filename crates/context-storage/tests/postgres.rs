//! Actual PostgreSQL acceptance tests; run explicitly with the test database URL.

use context_core::{
    access::{Authentication, Permission, Resource},
    ids::{AssetId, CollectionId, ContentId, PrincipalId, RevisionId, TenantId, WorkspaceId},
};
use context_storage::Error;
use context_storage::postgres::{
    Database, claim_job, existing_receipt, insert_resource, publish_revision, receipt,
};
use serde_json::json;
use sqlx::Row;
use std::collections::BTreeSet;
use uuid::Uuid;

async fn database() -> Database {
    Database::connect(
        &std::env::var("CONTEXT_TEST_DATABASE_URL").expect("explicit test database URL"),
        5,
    )
    .await
    .unwrap_or_else(|error| {
        panic!(
            "test database setup failed: {:?}",
            std::error::Error::source(&error)
        )
    })
}

struct Hierarchy {
    owner: TenantId,
    workspace: WorkspaceId,
    collection: CollectionId,
    asset: AssetId,
    principal: PrincipalId,
}

async fn hierarchy(db: &Database) -> Hierarchy {
    let owner = TenantId::new();
    let workspace = WorkspaceId::new();
    let collection = CollectionId::new();
    let asset = AssetId::new();
    let principal = db
        .identity("test", &Uuid::now_v7().to_string(), "service")
        .await
        .unwrap();
    let mut tx = db.transaction().await.unwrap();
    insert_resource(&mut tx, Resource::Tenant(owner), owner, None, true)
        .await
        .unwrap();
    insert_resource(
        &mut tx,
        Resource::Workspace(workspace),
        owner,
        Some(Resource::Tenant(owner)),
        true,
    )
    .await
    .unwrap();
    insert_resource(
        &mut tx,
        Resource::Collection(collection),
        owner,
        Some(Resource::Workspace(workspace)),
        true,
    )
    .await
    .unwrap();
    insert_resource(
        &mut tx,
        Resource::Asset(asset),
        owner,
        Some(Resource::Collection(collection)),
        true,
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO context.assets(id) VALUES($1)")
        .bind(Uuid::from(asset))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    Hierarchy {
        owner,
        workspace,
        collection,
        asset,
        principal,
    }
}

async fn revision(db: &Database, h: &Hierarchy) -> RevisionId {
    let revision = RevisionId::new();
    let content = ContentId::new();
    let mut tx = db.transaction().await.unwrap();
    sqlx::query("INSERT INTO context.content(id,workspace,asset,revision,kind,origin,body) VALUES($1,$2,$3,$4,'source','original_evidence',$5)")
        .bind(Uuid::from(content)).bind(Uuid::from(h.workspace)).bind(Uuid::from(h.asset)).bind(Uuid::from(revision))
        .bind(json!({"verified_fixture":"source"})).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO context.revisions(id,asset,content,author,media_type,size_bytes,sha256) VALUES($1,$2,$3,$4,'text/plain',6,$5)")
        .bind(Uuid::from(revision)).bind(Uuid::from(h.asset)).bind(Uuid::from(content)).bind(Uuid::from(h.principal))
        .bind([0_u8; 32].as_slice()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    revision
}

#[tokio::test]
#[ignore = "requires CONTEXT_TEST_DATABASE_URL pointing at isolated PostgreSQL"]
async fn inherited_cross_tenant_acl_and_token_scope_are_both_enforced() {
    let db = database().await;
    let h = hierarchy(&db).await;
    let guest = TenantId::new();
    let mut tx = db.transaction().await.unwrap();
    insert_resource(&mut tx, Resource::Tenant(guest), guest, None, true)
        .await
        .unwrap();
    sqlx::query("INSERT INTO context.memberships VALUES($1,$2)")
        .bind(Uuid::from(h.principal))
        .bind(Uuid::from(guest))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO context.grants VALUES($1,'tenant',$2,$3)")
        .bind(Uuid::from(h.collection))
        .bind(Uuid::from(guest))
        .bind(vec!["read", "purge"])
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let context = db
        .access_context(h.principal, BTreeSet::from([Permission::Read]))
        .await
        .unwrap();
    let target = Resource::Asset(h.asset);
    let chain = db.access_chain(target).await.unwrap();
    assert_eq!(chain[0].owner, h.owner);
    assert!(context.permits(target, Permission::Read, &chain));
    assert!(!context.permits(target, Permission::Purge, &chain));
    sqlx::query("UPDATE context.resources SET inherit=false WHERE id=$1")
        .bind(Uuid::from(h.asset))
        .execute(db.pool())
        .await
        .unwrap();
    assert!(!context.permits(
        target,
        Permission::Read,
        &db.access_chain(target).await.unwrap()
    ));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires CONTEXT_TEST_DATABASE_URL pointing at isolated PostgreSQL"]
async fn immutable_owner_and_ancestor_contracts_fail_at_the_database() {
    let db = database().await;
    let h = hierarchy(&db).await;
    assert!(matches!(
        db.access_chain(Resource::Asset(Uuid::from(h.workspace).into()))
            .await,
        Err(Error::Unavailable)
    ));
    let mut tx = db.transaction().await.unwrap();
    let error = insert_resource(
        &mut tx,
        Resource::Workspace(WorkspaceId::new()),
        TenantId::new(),
        Some(Resource::Tenant(h.owner)),
        true,
    )
    .await
    .unwrap_err();
    assert!(context_storage::is_constraint_violation(&error));
    assert!(!format!("{error:?}").contains(&h.owner.to_string()));
    tx.rollback().await.unwrap();
    assert!(
        sqlx::query("UPDATE context.resources SET parent=$2 WHERE id=$1")
            .bind(Uuid::from(h.collection))
            .bind(Uuid::from(h.owner))
            .execute(db.pool())
            .await
            .is_err()
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires CONTEXT_TEST_DATABASE_URL pointing at isolated PostgreSQL"]
async fn stale_or_purging_work_never_replaces_the_successful_current_revision() {
    let db = database().await;
    let h = hierarchy(&db).await;
    let first = revision(&db, &h).await;
    let second = revision(&db, &h).await;
    let mut tx = db.transaction().await.unwrap();
    publish_revision(&mut tx, h.asset, first, 1).await.unwrap();
    tx.commit().await.unwrap();
    sqlx::query("UPDATE context.assets SET generation=2 WHERE id=$1")
        .bind(Uuid::from(h.asset))
        .execute(db.pool())
        .await
        .unwrap();
    let mut tx = db.transaction().await.unwrap();
    assert!(matches!(
        publish_revision(&mut tx, h.asset, second, 1).await,
        Err(Error::Conflict)
    ));
    tx.rollback().await.unwrap();
    sqlx::query("UPDATE context.resources SET purging=true WHERE id=$1")
        .bind(Uuid::from(h.asset))
        .execute(db.pool())
        .await
        .unwrap();
    let mut tx = db.transaction().await.unwrap();
    assert!(matches!(
        publish_revision(&mut tx, h.asset, second, 2).await,
        Err(Error::Conflict)
    ));
    tx.rollback().await.unwrap();
    let current: Uuid =
        sqlx::query_scalar("SELECT current_revision FROM context.assets WHERE id=$1")
            .bind(Uuid::from(h.asset))
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(current, Uuid::from(first));
    let unpublished: bool =
        sqlx::query_scalar("SELECT NOT published FROM context.content WHERE revision=$1")
            .bind(Uuid::from(second))
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert!(unpublished);
    assert!(matches!(
        db.access_chain(Resource::Asset(h.asset)).await,
        Err(Error::Unavailable)
    ));
    sqlx::query("UPDATE context.resources SET purging=false WHERE id=$1")
        .bind(Uuid::from(h.asset))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE context.resources SET purging=true WHERE id=$1")
        .bind(Uuid::from(h.workspace))
        .execute(db.pool())
        .await
        .unwrap();
    let mut tx = db.transaction().await.unwrap();
    assert!(matches!(
        publish_revision(&mut tx, h.asset, second, 2).await,
        Err(Error::Conflict)
    ));
    tx.rollback().await.unwrap();
    db.close().await;
}

#[tokio::test]
#[ignore = "requires CONTEXT_TEST_DATABASE_URL pointing at isolated PostgreSQL"]
async fn concurrent_first_admission_has_one_receipt_and_changed_requests_conflict() {
    let db = database().await;
    let h = hierarchy(&db).await;
    let mut tasks = Vec::new();
    for candidate in [1, 2] {
        let db = db.clone();
        let principal = h.principal;
        tasks.push(tokio::spawn(async move {
            let mut tx = db.transaction().await.unwrap();
            let value = match existing_receipt(&mut tx, principal, "ingest", "concurrent", &[3; 32])
                .await
                .unwrap()
            {
                Some(value) => value,
                None => receipt(
                    &mut tx,
                    principal,
                    "ingest",
                    "concurrent",
                    &[3; 32],
                    &json!({"job":candidate}),
                )
                .await
                .unwrap(),
            };
            tx.commit().await.unwrap();
            value
        }));
    }
    assert_eq!(
        tasks.remove(0).await.unwrap(),
        tasks.remove(0).await.unwrap()
    );
    let mut tx = db.transaction().await.unwrap();
    assert!(matches!(
        existing_receipt(&mut tx, h.principal, "ingest", "concurrent", &[4; 32]).await,
        Err(Error::Conflict)
    ));
    tx.rollback().await.unwrap();
    let count = sqlx::query("SELECT count(*) AS count FROM context.receipts WHERE principal=$1")
        .bind(Uuid::from(h.principal))
        .fetch_one(db.pool())
        .await
        .unwrap()
        .get::<i64, _>("count");
    assert_eq!(count, 1);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires CONTEXT_TEST_DATABASE_URL pointing at isolated PostgreSQL"]
async fn concurrent_workers_claim_each_pending_job_once() {
    let db = database().await;
    let h = hierarchy(&db).await;
    let expected: BTreeSet<_> = [Uuid::now_v7(), Uuid::now_v7()].into();
    for id in &expected {
        sqlx::query("INSERT INTO context.jobs(id,principal,generation,kind,state,workflow) VALUES($1,$2,0,'synthesis','accepted','operator-fixture')")
            .bind(id).bind(Uuid::from(h.principal)).execute(db.pool()).await.unwrap();
    }
    let mut workers = Vec::new();
    for _ in 0..2 {
        let db = db.clone();
        workers.push(tokio::spawn(async move {
            let mut tx = db.transaction().await.unwrap();
            let job = claim_job(&mut tx).await.unwrap().unwrap();
            tx.commit().await.unwrap();
            job
        }));
    }
    let actual: BTreeSet<_> = [
        workers.remove(0).await.unwrap(),
        workers.remove(0).await.unwrap(),
    ]
    .into();
    assert_eq!(actual, expected);
    let mut tx = db.transaction().await.unwrap();
    assert!(claim_job(&mut tx).await.unwrap().is_none());
    tx.commit().await.unwrap();
    db.close().await;
}

#[tokio::test]
#[ignore = "requires CONTEXT_TEST_DATABASE_URL pointing at isolated PostgreSQL"]
async fn membership_and_grant_reads_never_mix_different_time_snapshots() {
    let db = database().await;
    let a = hierarchy(&db).await;
    let b = hierarchy(&db).await;
    let actor = Authentication {
        principal: a.principal,
        scopes: BTreeSet::from([Permission::Read]),
        credential: None,
    };
    sqlx::query("INSERT INTO context.memberships VALUES($1,$2)")
        .bind(Uuid::from(a.principal))
        .bind(Uuid::from(b.owner))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO context.grants VALUES($1,'tenant',$2,ARRAY['read'])")
        .bind(Uuid::from(a.asset))
        .bind(Uuid::from(b.owner))
        .execute(db.pool())
        .await
        .unwrap();
    let mut snapshot = db
        .authorized_read(&actor, Resource::Asset(a.asset))
        .await
        .unwrap();
    let mut change = db.transaction().await.unwrap();
    sqlx::query("DELETE FROM context.memberships WHERE principal=$1 AND resource=$2")
        .bind(Uuid::from(a.principal))
        .bind(Uuid::from(b.owner))
        .execute(&mut *change)
        .await
        .unwrap();
    sqlx::query("INSERT INTO context.grants VALUES($1,'tenant',$2,ARRAY['read'])")
        .bind(Uuid::from(b.asset))
        .bind(Uuid::from(b.owner))
        .execute(&mut *change)
        .await
        .unwrap();
    change.commit().await.unwrap();
    // The old snapshot sees membership but not the new grant. A fresh snapshot
    // sees the new grant but not membership. Neither ever permits the second asset.
    assert!(matches!(
        snapshot
            .authorize(Resource::Asset(b.asset), Permission::Read)
            .await,
        Err(Error::Unavailable)
    ));
    snapshot.commit().await.unwrap();
    assert!(matches!(
        db.authorized_read(&actor, Resource::Asset(b.asset)).await,
        Err(Error::Unavailable)
    ));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires CONTEXT_TEST_DATABASE_URL pointing at isolated PostgreSQL"]
async fn service_credential_scope_revocation_and_expiry_are_rechecked_in_the_data_transaction() {
    use sha2::{Digest, Sha256};
    let db = database().await;
    let h = hierarchy(&db).await;
    let credential = Uuid::now_v7();
    let digest = Sha256::digest(credential.as_bytes()).to_vec();
    sqlx::query("INSERT INTO context.grants VALUES($1,'principal',$2,ARRAY['read','purge'])")
        .bind(Uuid::from(h.asset))
        .bind(Uuid::from(h.principal))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO context.credentials(id,principal,digest,scopes,expires_at) VALUES($1,$2,$3,ARRAY['read'],now()+interval '1 hour')")
        .bind(credential).bind(Uuid::from(h.principal)).bind(digest).execute(db.pool()).await.unwrap();
    let actor = Authentication {
        principal: h.principal,
        scopes: BTreeSet::from([Permission::Read, Permission::Purge]),
        credential: Some(credential.into()),
    };
    db.authorized_read(&actor, Resource::Asset(h.asset))
        .await
        .unwrap()
        .commit()
        .await
        .unwrap();
    assert!(matches!(
        db.authorized_write(&actor, Resource::Asset(h.asset), Permission::Purge)
            .await,
        Err(Error::Unavailable)
    ));
    sqlx::query("UPDATE context.credentials SET revoked=true WHERE id=$1")
        .bind(credential)
        .execute(db.pool())
        .await
        .unwrap();
    assert!(matches!(
        db.authorized_read(&actor, Resource::Asset(h.asset)).await,
        Err(Error::Unavailable)
    ));
    sqlx::query("UPDATE context.credentials SET revoked=false,expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(credential).execute(db.pool()).await.unwrap();
    assert!(matches!(
        db.authorized_read(&actor, Resource::Asset(h.asset)).await,
        Err(Error::Unavailable)
    ));
    db.close().await;
}
