use actix_web::HttpRequest;

use crate::{
    auth,
    database::{auth_repo, auth_state_repo},
    domain::permission::PermissionGrant,
    error::AppError,
    security::LoginGate,
    state::InstalledState,
};

pub struct LoginInput<'a> {
    pub email: &'a str,
    pub password: &'a str,
    pub source: &'a str,
    pub challenge_id: Option<&'a str>,
    pub challenge_response: Option<&'a str>,
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
    ChallengeRequired {
        challenge_id: String,
        prompt: String,
    },
    Delayed {
        retry_after_seconds: u64,
    },
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
}

pub async fn login(
    installed: &InstalledState,
    input: LoginInput<'_>,
    pepper: &[u8],
) -> Result<LoginResult, LoginFailure> {
    let account_key = format!("account:{}", auth::token_hash(&input.email.to_lowercase()));
    let source_key = format!("source:{}", auth::token_hash(input.source));
    require_login_gate(installed, &input, &account_key, &source_key).await?;

    let mut credential = auth_repo::user_by_identifier(&installed.database, input.email)
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
    .map_err(|_| LoginFailure::Application(AppError::Internal))?;

    if needs_upgrade && let Some(user) = credential.as_mut() {
        let password_str = input.password.to_owned();
        let pepper_bytes = pepper.to_vec();
        let upgraded_hash = tokio::task::spawn_blocking(move || {
            auth::hash_password_unchecked(&password_str, &pepper_bytes)
        })
        .await
        .map_err(|_| LoginFailure::Application(AppError::Internal))?
        .map_err(LoginFailure::Application)?;

        auth_repo::update_password_hash(&installed.database, &user.id, &upgraded_hash)
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

    let credential = credential.ok_or(LoginFailure::Application(AppError::Internal))?;
    installed
        .auth_security
        .record_success(&account_key, &source_key)
        .await;

    if credential.totp_enabled {
        let temp_token = auth_state_repo::issue_2fa_temp_token(&installed.database, &credential.id)
            .await
            .map_err(AppError::from)
            .map_err(LoginFailure::Application)?;
        return Ok(LoginResult::RequiresTwoFactor { temp_token });
    }

    let (session_token, csrf_token) = auth_state_repo::create_session(&installed.database, &credential.id)
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
    let user_id = auth_state_repo::consume_2fa_temp_token(&installed.database, temp_token)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let (enabled, secret_opt) = auth_repo::get_totp_info(&installed.database, &user_id).await?;
    let Some(secret) = secret_opt else {
        return Err(AppError::Unauthorized);
    };
    if !enabled || !verify_totp_once(installed, &user_id, &secret, code).await? {
        return Err(AppError::Validation("Invalid 2FA verification code".into()));
    }

    let credential = auth_repo::user_by_id(&installed.database, &user_id)
        .await?
        .filter(|u| u.active)
        .ok_or(AppError::Unauthorized)?;

    let (session_token, csrf_token) =
        auth_state_repo::create_session(&installed.database, &credential.id).await?;
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
    let credential = auth_repo::user_by_id(&installed.database, &user.id)
        .await?
        .ok_or(AppError::NotFound)?;
    if credential.totp_enabled {
        return Err(AppError::Validation("2FA is already enabled".into()));
    }
    let secret = crate::totp::generate_totp_secret();
    let otpauth_uri = crate::totp::build_otpauth_uri(&credential.username, &secret);
    Ok(TwoFactorSetup { secret, otpauth_uri })
}

pub async fn enable_2fa(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    secret: &str,
    code: &str,
    password: &str,
    pepper: &[u8],
) -> Result<(), AppError> {
    let credential = auth_repo::user_by_id(&installed.database, &user.id)
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

    auth_repo::enable_totp(&installed.database, &user.id, secret).await?;
    crate::database::app_repo::audit(
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
    let credential = auth_repo::user_by_id(&installed.database, &user.id)
        .await?
        .ok_or(AppError::NotFound)?;

    if !verify_account_password(password, &credential.password_hash, pepper).await? {
        return Err(AppError::Validation("Invalid account password".into()));
    }

    let (enabled, secret_opt) = auth_repo::get_totp_info(&installed.database, &user.id).await?;
    let Some(secret) = secret_opt else {
        return Err(AppError::Validation("2FA is not enabled".into()));
    };
    if !enabled {
        return Err(AppError::Validation("2FA is not enabled".into()));
    }

    if !verify_totp_once(installed, &user.id, &secret, code).await? {
        return Err(AppError::Validation("Invalid 2FA verification code".into()));
    }

    auth_repo::disable_totp(&installed.database, &user.id).await?;
    auth_state_repo::clear_totp_replay(&installed.database, &user.id).await?;
    crate::database::app_repo::audit(
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
    request: &HttpRequest,
) -> Result<AuthenticatedUser, AppError> {
    authenticate_session(installed, request)
        .await
        .map(|(user, _)| user)
}

pub async fn current_user(
    installed: &InstalledState,
    request: &HttpRequest,
) -> Result<(AuthenticatedUser, String), AppError> {
    authenticate_session(installed, request).await
}

pub async fn authenticate_mutation(
    installed: &InstalledState,
    request: &HttpRequest,
) -> Result<AuthenticatedUser, AppError> {
    let (user, expected_csrf) = authenticate_session(installed, request).await?;
    let supplied_csrf = request
        .headers()
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Forbidden)?;
    if supplied_csrf != expected_csrf {
        return Err(AppError::Forbidden);
    }
    Ok(user)
}

pub async fn logout(installed: &InstalledState, request: &HttpRequest) -> Result<(), AppError> {
    if let Some(token) = auth::session_token(request) {
        auth_state_repo::revoke_session(&installed.database, &auth::token_hash(&token)).await?;
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
        .login_gate(
            account_key,
            source_key,
            input.challenge_id,
            input.challenge_response,
        )
        .await
    {
        LoginGate::Allowed => Ok(()),
        LoginGate::ChallengeRequired {
            challenge_id,
            prompt,
        } => Err(LoginFailure::ChallengeRequired {
            challenge_id,
            prompt,
        }),
        LoginGate::Delayed {
            retry_after_seconds,
        } => Err(LoginFailure::Delayed {
            retry_after_seconds,
        }),
    }
}

async fn authenticate_session(
    installed: &InstalledState,
    request: &HttpRequest,
) -> Result<(AuthenticatedUser, String), AppError> {
    let token = auth::session_token(request).ok_or(AppError::Unauthorized)?;
    let session = auth_state_repo::session(&installed.database, &auth::token_hash(&token))
        .await?
        .ok_or(AppError::Unauthorized)?;
    let credential = auth_repo::user_by_id(&installed.database, &session.user_id)
        .await?
        .filter(|user| user.active)
        .ok_or(AppError::Unauthorized)?;
    Ok((
        load_user(installed, credential).await?,
        session.csrf_token,
    ))
}

async fn load_user(
    installed: &InstalledState,
    credential: auth_repo::UserCredential,
) -> Result<AuthenticatedUser, AppError> {
    let grants = auth_repo::grants_for_user(&installed.database, &credential.id).await?;
    let roles = auth_repo::role_names_for_user(&installed.database, &credential.id).await?;
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
        LoginGate::ChallengeRequired {
            challenge_id,
            prompt,
        } => LoginFailure::ChallengeRequired {
            challenge_id,
            prompt,
        },
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
        .map_err(|_| AppError::Internal)
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
    Ok(auth_state_repo::consume_totp_step(&installed.database, user_id, step).await?)
}
