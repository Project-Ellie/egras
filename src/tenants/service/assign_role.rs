use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::persistence::RepoError;

#[derive(Debug, Clone)]
pub struct AssignRoleInput {
    pub organisation_id: Uuid,
    pub target_user_id: Uuid,
    pub role_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignRoleOutput {
    pub was_new: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AssignRoleError {
    #[error("not found")]
    NotFound,
    #[error("unknown role code")]
    UnknownRoleCode,
    #[error("unknown user")]
    UnknownUser,
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn assign_role(
    state: &AppState,
    actor: Uuid,
    actor_org: Uuid,
    is_operator: bool,
    input: AssignRoleInput,
) -> Result<AssignRoleOutput, AssignRoleError> {
    // Step 1: Cross-org rule — non-operators must be a member of the organisation.
    if !is_operator {
        let member = state
            .organisations
            .is_member(actor, input.organisation_id)
            .await?;
        if !member {
            return Err(AssignRoleError::NotFound);
        }
    }

    // Step 2: Resolve role by code.
    let role = state
        .roles
        .find_by_code(&input.role_code)
        .await?
        .ok_or(AssignRoleError::UnknownRoleCode)?;

    // Step 3: Verify target is already a member of the org.
    let target_is_member = state
        .organisations
        .is_member(input.target_user_id, input.organisation_id)
        .await?;
    if !target_is_member {
        return Err(AssignRoleError::UnknownUser);
    }

    // Step 4: Check pre-existence for idempotency.
    let already = state
        .roles
        .has_role(input.target_user_id, input.organisation_id, role.id)
        .await?;
    let was_new = !already;

    // Step 5: Assign (idempotent via ON CONFLICT DO NOTHING).
    state
        .roles
        .assign(input.target_user_id, input.organisation_id, role.id)
        .await?;

    // Step 6: Emit audit event only when the row is newly created.
    // Known limitation: the has_role/assign pair is not atomic. Two concurrent
    // equal requests can both observe has_role=false, both call assign (second
    // hits ON CONFLICT DO NOTHING), and both emit an audit event. Mitigation
    // for a follow-up: change RoleRepository::assign to return bool (rows_affected > 0)
    // and derive was_new from the insert result. Left as-is for Plan 2a — the race
    // is narrow under expected load and the duplicate audit is accurate, just redundant.
    if was_new {
        let event = AuditEvent::organisation_role_assigned(
            actor,
            actor_org,
            input.organisation_id,
            input.target_user_id,
            &input.role_code,
        );
        if let Err(e) = state.audit_recorder.record(event).await {
            warn!(
                error = %e,
                org_id = %input.organisation_id,
                target_user_id = %input.target_user_id,
                "audit record failed for organisation.role_assigned"
            );
        }
    }

    // Step 7: Return output.
    Ok(AssignRoleOutput { was_new })
}
