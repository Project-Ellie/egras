use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use egras::audit::model::AuditEvent;
use egras::audit::persistence::{AuditRepository, AuditQueryFilter, AuditQueryPage};
use egras::audit::worker::AuditWorker;
use tokio::sync::mpsc;
use uuid::Uuid;

struct AlwaysOkRepo { calls: Arc<AtomicU32> }

#[async_trait]
impl AuditRepository for AlwaysOkRepo {
    async fn insert(&self, _e: &AuditEvent) -> anyhow::Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn list_events(&self, _f: &AuditQueryFilter) -> anyhow::Result<AuditQueryPage> {
        unimplemented!()
    }
}

struct FailsNTimes {
    remaining_failures: Arc<AtomicU32>,
    successes: Arc<AtomicU32>,
}

#[async_trait]
impl AuditRepository for FailsNTimes {
    async fn insert(&self, _e: &AuditEvent) -> anyhow::Result<()> {
        if self.remaining_failures.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
            if n == 0 { None } else { Some(n - 1) }
        }).is_ok() {
            anyhow::bail!("transient failure");
        }
        self.successes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn list_events(&self, _f: &AuditQueryFilter) -> anyhow::Result<AuditQueryPage> {
        unimplemented!()
    }
}

#[tokio::test]
async fn drains_all_events_on_shutdown() {
    let (tx, rx) = mpsc::channel(16);
    let calls = Arc::new(AtomicU32::new(0));
    let repo = Arc::new(AlwaysOkRepo { calls: calls.clone() });
    let handle = AuditWorker::new(rx, repo, 3, 5).spawn();

    for _ in 0..5 {
        tx.send(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7())).await.unwrap();
    }
    drop(tx);
    handle.shutdown().await;

    assert_eq!(calls.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn retries_then_succeeds() {
    let (tx, rx) = mpsc::channel(4);
    let successes = Arc::new(AtomicU32::new(0));
    let repo = Arc::new(FailsNTimes {
        remaining_failures: Arc::new(AtomicU32::new(2)),
        successes: successes.clone(),
    });
    let handle = AuditWorker::new(rx, repo, 5, 1).spawn();

    tx.send(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7())).await.unwrap();
    drop(tx);
    handle.shutdown().await;

    assert_eq!(successes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn gives_up_after_max_retries() {
    let (tx, rx) = mpsc::channel(4);
    let successes = Arc::new(AtomicU32::new(0));
    let repo = Arc::new(FailsNTimes {
        remaining_failures: Arc::new(AtomicU32::new(100)), // always fail
        successes: successes.clone(),
    });
    let handle = AuditWorker::new(rx, repo, 2, 1).spawn();

    tx.send(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7())).await.unwrap();
    drop(tx);
    handle.shutdown().await;

    assert_eq!(successes.load(Ordering::SeqCst), 0); // never succeeded
    // Worker still terminated (did not block forever)
}
