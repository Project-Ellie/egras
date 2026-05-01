use uuid::Uuid;

use crate::app_state::AppState;
use crate::pagination as cursor_codec;
use crate::tenants::model::MembershipCursor;
use crate::tenants::persistence::RepoError;

#[derive(Debug, Clone)]
pub struct ListMembersInput {
    pub organisation_id: Uuid,
    pub after: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct ListMembersOutput {
    pub items: Vec<MemberSummaryDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MemberSummaryDto {
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
    pub role_codes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ListMembersError {
    #[error("not found")]
    NotFound,
    #[error("invalid cursor")]
    InvalidCursor,
    #[error(transparent)]
    Repo(#[from] RepoError),
}

pub async fn list_organisation_members(
    state: &AppState,
    caller: Uuid,
    is_operator: bool,
    input: ListMembersInput,
) -> Result<ListMembersOutput, ListMembersError> {
    let limit = input.limit.clamp(1, 100);

    let cursor = match input.after.as_deref() {
        Some(raw) => Some(
            cursor_codec::decode::<MembershipCursor>(raw)
                .map_err(|_| ListMembersError::InvalidCursor)?,
        ),
        None => None,
    };

    // Cross-org rule (§3.5): non-operators must be a member of the organisation.
    if !is_operator {
        let member = state
            .organisations
            .is_member(caller, input.organisation_id)
            .await?;
        if !member {
            return Err(ListMembersError::NotFound);
        }
    }

    // Fetch limit+1 to detect a next page.
    let over_fetch = limit.saturating_add(1);
    let mut rows = state
        .organisations
        .list_members(input.organisation_id, cursor, over_fetch)
        .await?;

    let next_cursor = if rows.len() as u32 > limit {
        rows.truncate(limit as usize);
        let last = rows.last().expect("rows is non-empty by construction");
        Some(cursor_codec::encode(&MembershipCursor {
            created_at: last.joined_at,
            user_id: last.user_id,
        }))
    } else {
        None
    };

    Ok(ListMembersOutput {
        items: rows
            .into_iter()
            .map(|m| MemberSummaryDto {
                user_id: m.user_id,
                username: m.username,
                email: m.email,
                role_codes: m.role_codes,
            })
            .collect(),
        next_cursor,
    })
}
