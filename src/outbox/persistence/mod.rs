mod outbox_repository_pg;

use async_trait::async_trait;
use uuid::Uuid;

use crate::outbox::model::{AppendRequest, OutboxEvent};

pub use outbox_repository_pg::OutboxRepositoryPg;

#[async_trait]
pub trait OutboxRepository: Send + Sync + 'static {
    async fn append_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        req: AppendRequest,
    ) -> anyhow::Result<Uuid>;

    /// Claim a batch of unrelayed events with `FOR UPDATE SKIP LOCKED`.
    /// The caller must mark them relayed within the same `tx` to avoid loss.
    async fn claim_unrelayed_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        limit: u32,
    ) -> anyhow::Result<Vec<OutboxEvent>>;

    async fn mark_relayed_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        ids: &[Uuid],
    ) -> anyhow::Result<()>;

    async fn find(&self, id: Uuid) -> anyhow::Result<Option<OutboxEvent>>;
}
