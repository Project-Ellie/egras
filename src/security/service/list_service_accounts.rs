use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::app_state::AppState;
use crate::security::model::ServiceAccount;

#[derive(Debug, Clone)]
pub struct ListServiceAccountsInput {
    pub organisation_id: Uuid,
    pub limit: u32,
    pub after: Option<(DateTime<Utc>, Uuid)>,
}

#[derive(Debug, Clone)]
pub struct ListServiceAccountsOutput {
    pub items: Vec<ServiceAccount>,
    pub next_cursor: Option<(DateTime<Utc>, Uuid)>,
}

pub async fn list_service_accounts(
    state: &AppState,
    input: ListServiceAccountsInput,
) -> anyhow::Result<ListServiceAccountsOutput> {
    let limit = input.limit.clamp(1, 200);
    let mut items = state
        .service_accounts
        .list(input.organisation_id, limit + 1, input.after)
        .await?;

    let next_cursor = if items.len() as u32 > limit {
        let extra = items.pop().unwrap();
        Some((extra.created_at, extra.user_id))
    } else {
        None
    };

    Ok(ListServiceAccountsOutput { items, next_cursor })
}
