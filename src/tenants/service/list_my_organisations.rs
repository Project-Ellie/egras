use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app_state::AppState;
use crate::pagination as cursor_codec;
use crate::tenants::model::OrganisationCursor;
use crate::tenants::persistence::RepoError;

#[derive(Debug, Clone)]
pub struct ListMyOrganisationsInput {
    pub after: Option<String>, // base64url-encoded OrganisationCursor (JSON)
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct ListMyOrganisationsOutput {
    pub items: Vec<OrganisationSummaryDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganisationSummaryDto {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub role_codes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ListError {
    #[error("invalid cursor")]
    InvalidCursor,
    #[error(transparent)]
    Repo(#[from] RepoError),
}

pub async fn list_my_organisations(
    state: &AppState,
    caller: Uuid,
    input: ListMyOrganisationsInput,
) -> Result<ListMyOrganisationsOutput, ListError> {
    let limit = input.limit.clamp(1, 100);

    let cursor = match input.after.as_deref() {
        Some(raw) => Some(
            cursor_codec::decode::<OrganisationCursor>(raw)
                .map_err(|_| ListError::InvalidCursor)?,
        ),
        None => None,
    };

    // Fetch limit+1 to detect a next page.
    let over_fetch = limit.saturating_add(1);
    let mut rows = state
        .organisations
        .list_for_user(caller, cursor, over_fetch)
        .await?;

    let next_cursor = if rows.len() as u32 > limit {
        rows.truncate(limit as usize);
        let last = rows.last().expect("rows is non-empty by construction");
        Some(cursor_codec::encode(&OrganisationCursor {
            created_at: last.created_at,
            id: last.id,
        }))
    } else {
        None
    };

    Ok(ListMyOrganisationsOutput {
        items: rows
            .into_iter()
            .map(|o| OrganisationSummaryDto {
                id: o.id,
                name: o.name,
                business: o.business,
                role_codes: o.role_codes,
            })
            .collect(),
        next_cursor,
    })
}
