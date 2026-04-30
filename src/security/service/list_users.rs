use std::collections::HashMap;

use uuid::Uuid;

use crate::app_state::AppState;
use crate::security::model::{UserCursor, UserMembership};
use crate::security::persistence::user_repository::UserRepoError;
use crate::tenants::service::cursor_codec;

#[derive(Debug, Clone)]
pub struct ListUsersInput {
    pub org_id: Option<Uuid>,
    pub q: Option<String>,
    pub after: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct UserWithMemberships {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub memberships: Vec<UserMembership>,
}

#[derive(Debug, Clone)]
pub struct ListUsersOutput {
    pub items: Vec<UserWithMemberships>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ListUsersError {
    #[error("invalid cursor")]
    InvalidCursor,
    #[error(transparent)]
    Repo(#[from] UserRepoError),
}

pub async fn list_users(
    state: &AppState,
    _caller: Uuid,
    is_operator: bool,
    caller_org_id: Option<Uuid>,
    input: ListUsersInput,
) -> Result<ListUsersOutput, ListUsersError> {
    let limit = input.limit.clamp(1, 100);

    let cursor = match input.after.as_deref() {
        Some(raw) => Some(
            cursor_codec::decode::<UserCursor>(raw)
                .map_err(|_| ListUsersError::InvalidCursor)?,
        ),
        None => None,
    };

    // Non-operators are scoped to their own org regardless of org_id param.
    let effective_org_id = if is_operator {
        input.org_id
    } else {
        caller_org_id
    };

    // Over-fetch by 1 to detect next page.
    let over_fetch = limit.saturating_add(1);
    let mut users = state
        .users
        .list_users(effective_org_id, input.q.as_deref(), cursor, over_fetch)
        .await?;

    let next_cursor = if users.len() as u32 > limit {
        users.truncate(limit as usize);
        let last = users.last().expect("non-empty after truncation");
        Some(cursor_codec::encode(&UserCursor {
            created_at: last.created_at,
            user_id: last.id,
        }))
    } else {
        None
    };

    // Batch-fetch memberships.
    let user_ids: Vec<Uuid> = users.iter().map(|u| u.id).collect();
    let raw_memberships = state
        .users
        .list_memberships_for_users(&user_ids)
        .await?;

    // Group memberships by user_id.
    let mut by_user: HashMap<Uuid, Vec<UserMembership>> = HashMap::new();
    for (uid, membership) in raw_memberships {
        by_user.entry(uid).or_default().push(membership);
    }

    // For non-operators, filter memberships to caller's org only.
    let items = users
        .into_iter()
        .map(|u| {
            let mut memberships = by_user.remove(&u.id).unwrap_or_default();
            if !is_operator {
                if let Some(org) = caller_org_id {
                    memberships.retain(|m| m.org_id == org);
                }
            }
            UserWithMemberships {
                id: u.id,
                username: u.username,
                email: u.email,
                created_at: u.created_at,
                memberships,
            }
        })
        .collect();

    Ok(ListUsersOutput { items, next_cursor })
}
