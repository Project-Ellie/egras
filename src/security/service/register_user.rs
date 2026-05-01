use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::security::persistence::user_repository::CreateAndAddError;

#[derive(Debug, Clone)]
pub struct RegisterUserInput {
    pub username: String,
    pub email: String,
    pub password: String,
    pub target_org_id: Uuid,
    pub role_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterUserOutput {
    pub user_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum RegisterUserError {
    #[error("username already taken")]
    DuplicateUsername,
    #[error("email already registered")]
    DuplicateEmail,
    #[error("invalid username: must be 1-64 chars")]
    InvalidUsername,
    #[error("invalid email")]
    InvalidEmail,
    #[error("password too short (min 8 chars)")]
    PasswordTooShort,
    #[error("organisation not found")]
    OrgNotFound,
    #[error("unknown role code")]
    UnknownRoleCode,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn register_user(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org_id: Uuid,
    input: RegisterUserInput,
) -> Result<RegisterUserOutput, RegisterUserError> {
    let username = input.username.trim().to_string();
    let email = input.email.trim().to_lowercase();
    if username.is_empty() || username.len() > 64 {
        return Err(RegisterUserError::InvalidUsername);
    }
    if !is_valid_email(&email) {
        return Err(RegisterUserError::InvalidEmail);
    }
    if input.password.len() < 8 {
        return Err(RegisterUserError::PasswordTooShort);
    }

    let hash = super::password_hash::hash_password(&input.password)
        .map_err(RegisterUserError::Internal)?;

    let user = state
        .users
        .create_and_add_to_org(
            &username,
            &email,
            &hash,
            input.target_org_id,
            &input.role_code,
        )
        .await
        .map_err(|e| match e {
            CreateAndAddError::DuplicateUsername(_) => RegisterUserError::DuplicateUsername,
            CreateAndAddError::DuplicateEmail(_) => RegisterUserError::DuplicateEmail,
            CreateAndAddError::OrgNotFound => RegisterUserError::OrgNotFound,
            CreateAndAddError::UnknownRoleCode(_) => RegisterUserError::UnknownRoleCode,
            CreateAndAddError::Db(e) => RegisterUserError::Internal(e.into()),
        })?;

    let event = AuditEvent::user_registered_success(
        actor_user_id,
        actor_org_id,
        user.id,
        input.target_org_id,
        input.role_code,
    );
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %user.id, "audit record failed for user.registered");
    }

    Ok(RegisterUserOutput { user_id: user.id })
}

fn is_valid_email(email: &str) -> bool {
    if email.len() > 254 {
        return false;
    }
    match email.splitn(2, '@').collect::<Vec<_>>().as_slice() {
        [local, domain] => {
            !local.is_empty()
                && !domain.is_empty()
                && domain.contains('.')
                && !domain.ends_with('.')
                && !domain.starts_with('.')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_valid_email;

    #[test]
    fn valid_emails_pass() {
        assert!(is_valid_email("user@example.com"));
        assert!(is_valid_email("a@b.io"));
        assert!(is_valid_email("foo.bar+baz@sub.domain.org"));
    }

    #[test]
    fn invalid_emails_fail() {
        assert!(!is_valid_email(""));
        assert!(!is_valid_email("@"));
        assert!(!is_valid_email("a@"));
        assert!(!is_valid_email("@b"));
        assert!(!is_valid_email("nodomain"));
        assert!(!is_valid_email("user@.com"));
        assert!(!is_valid_email("user@com."));
        assert!(!is_valid_email("user@nodot"));
        assert!(!is_valid_email(&"a".repeat(255)));
    }
}
