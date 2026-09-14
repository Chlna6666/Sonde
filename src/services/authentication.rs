use crate::{
    auth,
    database::{applications, auth as auth_store, auth_state},
    domain::permission::PermissionGrant,
    error::AppError,
    security::LoginGate,
    state::InstalledState,
};

pub trait AuthRequest {
    fn session_token(&self) -> Option<String>;
    fn csrf_token(&self) -> Option<&str>;
}

pub struct LoginInput<'a> {
    pub email: &'a str,
    pub password: &'a str,
    pub source: &'a str,
    pub challenge_id: Option<&'a str>,
    pub website: Option<&'a str>,
}

pub struct LoginOutcome {
    pub session_token: String,
    pub csrf_token: String,
    pub user: AuthenticatedUser,
}

pub enum LoginResult {
    Success(LoginOutcome),
    RequiresTwoFactor { temp_token: String },
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TwoFactorSetup {
    pub secret: String,
    pub otpauth_uri: String,
}

pub enum LoginFailure {
    ChallengeRequired { challenge_id: String },
    Delayed { retry_after_seconds: u64 },
    Application(AppError),
}

#[derive(Clone, Debug)]
pub struct AuthenticatedUser {
    pub id: String,
    pub email: String,
    pub username: String,
    pub locale: String,
    pub roles: Vec<String>,
    pub grants: Vec<PermissionGrant>,
    pub totp_enabled: bool,
}

impl AuthenticatedUser {
    pub fn require(&self, permission: &str, application_id: Option<&str>) -> Result<(), AppError> {
        self.grants
            .iter()
            .any(|grant| grant.allows(permission, application_id))
            .then_some(())
            .ok_or(AppError::Forbidden)
    }

    #[must_use]
    pub fn has_unscoped_permission(&self, permission: &str) -> bool {
        self.grants
            .iter()
            .any(|grant| grant.application_id.is_none() && grant.allows(permission, None))
    }

    #[must_use]
    pub fn is_unscoped_admin(&self) -> bool {
        self.has_unscoped_permission("*")
            || (self.has_unscoped_permission("members.manage")
                && self.has_unscoped_permission("apps.manage")
                && self.has_unscoped_permission("settings.manage"))
    }

    #[must_use]
    pub fn is_unscoped_owner(&self) -> bool {
        self.has_unscoped_permission("*")
    }
}

pub async fn login(
    installed: &InstalledState,
    input: LoginInput<'_>,
    pepper: &[u8],
) -> Result<LoginResult, LoginFailure> {
    let account_key = format!("account:{}", auth::token_hash(&input.email.to_lowercase()));
    let source_key = format!("source:{}", auth::token_hash(input.source));
    require_login_gate(installed, &input, &account_key, &source_key).await?;

    let mut credential = auth_store::user_by_identifier(&installed.database, input.email)
        .await
        .map_err(AppError::from)
        .map_err(LoginFailure::Application)?;
    let password_hash = credential
        .as_ref()
        .map_or(installed.auth_security.dummy_password_hash(), |user| {
            user.password_hash.as_str()
        });
    let password = input.password.to_owned();
    let pepper_vec = pepper.to_vec();
    let password_hash_owned = password_hash.to_owned();
    let legacy_hash = credential.as_ref().map(|user| user.password_hash.clone());

    let (mut password_valid, needs_upgrade) = tokio::task::spawn_blocking(move || {
        if auth::verify_password(&password, &password_hash_owned, &pepper_vec) {
            (true, false)
        } else if let Some(legacy) = legacy_hash {
            if auth::verify_legacy_password(&password, &legacy) {
                (true, true)
            } else {
                (false, false)
            }
        } else {
            (false, false)
        }
    })
    .await
    .map_err(|error| {
        LoginFailure::Application(AppError::internal("verify login password", error))
    })?;

    if needs_upgrade && let Some(user) = credential.as_mut() {
        let password_str = input.password.to_owned();
        let pepper_bytes = pepper.to_vec();
        let upgraded_hash = tokio::task::spawn_blocking(move || {
            auth::hash_password_unchecked(&password_str, &pepper_bytes)
        })
        .await
        .map_err(|error| {
            LoginFailure::Application(AppError::internal("upgrade password hash", error))
        })?
        .map_err(LoginFailure::Application)?;

        auth_store::update_password_hash(&installed.database, &user.id, &upgraded_hash)
            .await
            .map_err(AppError::from)
            .map_err(LoginFailure::Application)?;
        user.password_hash = upgraded_hash;
        password_valid = true;
    }
    let credential_valid = credential
        .as_ref()
        .is_some_and(|user| user.active && password_valid);
    if !credential_valid || input.website.is_some_and(|value| !value.is_empty()) {
        return Err(login_challenge(installed, &account_key, &source_key).await);
    }

    let credential = credential.ok_or_else(|| {
        LoginFailure::Application(AppError::internal(
            "load authenticated credential",
            "credential disappeared after successful validation",
        ))
    })?;
    installed
        .auth_security
        .record_success(&account_key, &source_key)
        .await;

    if credential.totp_enabled {
        let temp_token = auth_state::issue_2fa_temp_token(&installed.database, &credential.id)
            .await
            .map_err(AppError::from)
            .map_err(LoginFailure::Application)?;
        return Ok(LoginResult::RequiresTwoFactor { temp_token });
    }

    let (session_token, csrf_token) =
        auth_state::create_session(&installed.database, &credential.id)
            .await
            .map_err(AppError::from)
            .map_err(LoginFailure::Application)?;
    let user = load_user(installed, credential)
        .await
        .map_err(LoginFailure::Application)?;
    Ok(LoginResult::Success(LoginOutcome {
        session_token,
        csrf_token,
        user,
    }))
}

pub async fn verify_2fa_login(
    installed: &InstalledState,
    temp_token: &str,
    code: &str,
) -> Result<LoginOutcome, AppError> {
    let user_id = auth_state::consume_2fa_temp_token(&installed.database, temp_token)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let (enabled, secret_opt) = auth_store::get_totp_info(&installed.database, &user_id).await?;
    let Some(secret) = secret_opt else {
        return Err(AppError::Unauthorized);
    };
    if !enabled || !verify_totp_once(installed, &user_id, &secret, code).await? {
        return Err(AppError::Validation("Invalid 2FA verification code".into()));
    }

    let credential = auth_store::user_by_id(&installed.database, &user_id)
        .await?
        .filter(|u| u.active)
        .ok_or(AppError::Unauthorized)?;

    let (session_token, csrf_token) =
        auth_state::create_session(&installed.database, &credential.id).await?;
    let user = load_user(installed, credential).await?;
    Ok(LoginOutcome {
        session_token,
        csrf_token,
        user,
    })
}

pub async fn setup_2fa(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<TwoFactorSetup, AppError> {
    let credential = auth_store::user_by_id(&installed.database, &user.id)
        .await?
        .ok_or(AppError::NotFound)?;
    if credential.totp_enabled {
        return Err(AppError::Validation("2FA is already enabled".into()));
    }
    let secret = crate::totp::generate_totp_secret();
    let otpauth_uri = crate::totp::build_otpauth_uri(&credential.username, &secret);
    Ok(TwoFactorSetup {
        secret,
        otpauth_uri,
    })
}

pub async fn enable_2fa(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    secret: &str,
    code: &str,
    password: &str,
    pepper: &[u8],
) -> Result<(), AppError> {
    let credential = auth_store::user_by_id(&installed.database, &user.id)
        .await?
        .ok_or(AppError::NotFound)?;
    if credential.totp_enabled {
        return Err(AppError::Validation("2FA is already enabled".into()));
    }

    if !verify_account_password(password, &credential.password_hash, pepper).await? {
        return Err(AppError::Validation("Invalid account password".into()));
    }

    if !verify_totp_once(installed, &user.id, secret, code).await? {
        return Err(AppError::Validation("Invalid 2FA verification code".into()));
    }

    auth_store::enable_totp(&installed.database, &user.id, secret).await?;
    applications::audit(
        &installed.database,
        Some(&user.id),
        "user.2fa_enabled",
        "user",
        Some(&user.id),
    )
    .await?;

    Ok(())
}

pub async fn disable_2fa(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    code: &str,
    password: &str,
    pepper: &[u8],
) -> Result<(), AppError> {
    let credential = auth_store::user_by_id(&installed.database, &user.id)
        .await?
        .ok_or(AppError::NotFound)?;

    if !verify_account_password(password, &credential.password_hash, pepper).await? {
        return Err(AppError::Validation("Invalid account password".into()));
    }

    let (enabled, secret_opt) = auth_store::get_totp_info(&installed.database, &user.id).await?;
    let Some(secret) = secret_opt else {
        return Err(AppError::Validation("2FA is not enabled".into()));
    };
    if !enabled {
        return Err(AppError::Validation("2FA is not enabled".into()));
    }

    if !verify_totp_once(installed, &user.id, &secret, code).await? {
        return Err(AppError::Validation("Invalid 2FA verification code".into()));
    }

    auth_store::disable_totp(&installed.database, &user.id).await?;
    auth_state::clear_totp_replay(&installed.database, &user.id).await?;
    auth_state::revoke_sessions_for_user(&installed.database, &user.id).await?;
    applications::audit(
        &installed.database,
        Some(&user.id),
        "user.2fa_disabled",
        "user",
        Some(&user.id),
    )
    .await?;

    Ok(())
}

pub async fn authenticate(
    installed: &InstalledState,
    request: &impl AuthRequest,
) -> Result<AuthenticatedUser, AppError> {
    authenticate_session(installed, request)
        .await
        .map(|(user, _)| user)
}

pub async fn current_user(
    installed: &InstalledState,
    request: &impl AuthRequest,
) -> Result<(AuthenticatedUser, String), AppError> {
    authenticate_session(installed, request).await
}

pub async fn authenticate_mutation(
    installed: &InstalledState,
    request: &impl AuthRequest,
) -> Result<AuthenticatedUser, AppError> {
    let (user, expected_csrf) = authenticate_session(installed, request).await?;
    let supplied_csrf = request.csrf_token().ok_or(AppError::Forbidden)?;
    if !constant_time_eq(supplied_csrf.as_bytes(), expected_csrf.as_bytes()) {
        return Err(AppError::Forbidden);
    }
    Ok(user)
}

pub async fn logout(
    installed: &InstalledState,
    request: &impl AuthRequest,
) -> Result<(), AppError> {
    if let Some(token) = request.session_token() {
        auth_state::revoke_session(&installed.database, &auth::token_hash(&token)).await?;
    }
    Ok(())
}

async fn require_login_gate(
    installed: &InstalledState,
    input: &LoginInput<'_>,
    account_key: &str,
    source_key: &str,
) -> Result<(), LoginFailure> {
    match installed
        .auth_security
        .login_gate(account_key, source_key, input.challenge_id)
        .await
    {
        LoginGate::Allowed => Ok(()),
        LoginGate::ChallengeRequired { challenge_id } => {
            Err(LoginFailure::ChallengeRequired { challenge_id })
        }
        LoginGate::Delayed {
            retry_after_seconds,
        } => Err(LoginFailure::Delayed {
            retry_after_seconds,
        }),
    }
}

async fn authenticate_session(
    installed: &InstalledState,
    request: &impl AuthRequest,
) -> Result<(AuthenticatedUser, String), AppError> {
    let token = request.session_token().ok_or(AppError::Unauthorized)?;
    let session = auth_state::session(&installed.database, &auth::token_hash(&token))
        .await?
        .ok_or(AppError::Unauthorized)?;
    let credential = auth_store::user_by_id(&installed.database, &session.user_id)
        .await?
        .filter(|user| user.active)
        .ok_or(AppError::Unauthorized)?;
    Ok((load_user(installed, credential).await?, session.csrf_token))
}

async fn load_user(
    installed: &InstalledState,
    credential: auth_store::UserCredential,
) -> Result<AuthenticatedUser, AppError> {
    let grants = auth_store::grants_for_user(&installed.database, &credential.id).await?;
    let roles = auth_store::role_names_for_user(&installed.database, &credential.id).await?;
    Ok(AuthenticatedUser {
        id: credential.id,
        email: credential.email,
        username: credential.username,
        locale: credential.locale,
        roles,
        grants,
        totp_enabled: credential.totp_enabled,
    })
}

async fn login_challenge(
    installed: &InstalledState,
    account_key: &str,
    source_key: &str,
) -> LoginFailure {
    match installed
        .auth_security
        .record_failure(account_key, source_key)
        .await
    {
        LoginGate::ChallengeRequired { challenge_id } => {
            LoginFailure::ChallengeRequired { challenge_id }
        }
        LoginGate::Delayed {
            retry_after_seconds,
        } => LoginFailure::Delayed {
            retry_after_seconds,
        },
        LoginGate::Allowed => LoginFailure::Application(AppError::Unauthorized),
    }
}

async fn verify_account_password(
    password: &str,
    password_hash: &str,
    pepper: &[u8],
) -> Result<bool, AppError> {
    let password = password.to_owned();
    let password_hash = password_hash.to_owned();
    let pepper = pepper.to_vec();
    tokio::task::spawn_blocking(move || auth::verify_password(&password, &password_hash, &pepper))
        .await
        .map_err(|error| AppError::internal("verify account password", error))
}

async fn verify_totp_once(
    installed: &InstalledState,
    user_id: &str,
    secret: &str,
    code: &str,
) -> Result<bool, AppError> {
    let Some(step) = crate::totp::verify_totp_step(secret, code) else {
        return Ok(false);
    };
    Ok(auth_state::consume_totp_step(&installed.database, user_id, step).await?)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::{AuthenticatedUser, constant_time_eq};
    use crate::domain::permission::PermissionGrant;

    fn user_with(grants: Vec<PermissionGrant>) -> AuthenticatedUser {
        AuthenticatedUser {
            id: "user".into(),
            email: "user@example.com".into(),
            username: "user".into(),
            locale: "en".into(),
            roles: Vec::new(),
            grants,
            totp_enabled: false,
        }
    }

    #[test]
    fn application_scoped_wildcard_is_not_unscoped_owner() {
        let user = user_with(vec![PermissionGrant {
            permissions: vec!["*".into()],
            application_id: Some("app-1".into()),
        }]);
        assert!(!user.is_unscoped_owner());
        assert!(!user.is_unscoped_admin());
        assert!(user.require("apps.manage", Some("app-1")).is_ok());
        assert!(user.require("apps.manage", None).is_err());
    }

    #[test]
    fn global_admin_permissions_are_unscoped_admin_not_owner() {
        let user = user_with(vec![PermissionGrant {
            permissions: vec![
                "members.manage".into(),
                "apps.manage".into(),
                "settings.manage".into(),
            ],
            application_id: None,
        }]);
        assert!(user.is_unscoped_admin());
        assert!(!user.is_unscoped_owner());
    }

    #[test]
    fn csrf_compare_rejects_length_mismatch() {
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"tokem"));
    }
}
