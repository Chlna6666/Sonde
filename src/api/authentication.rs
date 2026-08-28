use std::sync::Arc;

use actix_web::{
    HttpRequest, HttpResponse,
    cookie::{Cookie, SameSite, time::Duration},
    http::{StatusCode, header},
    web,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth,
    error::AppError,
    services::authentication::{self, LoginFailure, LoginInput},
    state::AppState,
};

const MAX_AUTH_JSON_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginRequest {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    account: Option<String>,
    #[serde(default)]
    identifier: Option<String>,
    password: String,
    challenge_id: Option<String>,
    challenge_response: Option<String>,
    website: Option<String>,
}

impl LoginRequest {
    fn identifier(&self) -> Result<&str, AppError> {
        let identifier = self
            .username
            .as_deref()
            .or(self.email.as_deref())
            .or(self.account.as_deref())
            .or(self.identifier.as_deref())
            .filter(|val| !val.trim().is_empty())
            .ok_or_else(|| AppError::Validation("username or email is required".into()))?;
        if identifier.len() > 254 {
            return Err(AppError::Validation(
                "username or email is too long".into(),
            ));
        }
        Ok(identifier)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UserResponse {
    id: String,
    email: String,
    username: String,
    locale: String,
    roles: Vec<String>,
    csrf_token: String,
    totp_enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TwoFactorRequiredResponse {
    requires2fa: bool,
    temp_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TwoFactorVerifyRequest {
    temp_token: String,
    code: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TwoFactorEnableRequest {
    secret: String,
    code: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TwoFactorDisableRequest {
    code: String,
    password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginErrorResponse {
    code: &'static str,
    message: &'static str,
    challenge_id: Option<String>,
    challenge_prompt: Option<String>,
    retry_after_seconds: Option<u64>,
}

pub fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1/auth")
            .app_data(web::JsonConfig::default().limit(MAX_AUTH_JSON_BYTES))
            .route("/login", web::post().to(login))
            .route("/2fa/verify", web::post().to(verify_2fa))
            .route("/2fa/setup", web::post().to(setup_2fa))
            .route("/2fa/enable", web::post().to(enable_2fa))
            .route("/2fa/disable", web::post().to(disable_2fa))
            .route("/me", web::get().to(me))
            .route("/logout", web::post().to(logout)),
    );
}

async fn login(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<LoginRequest>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let source = login_source(&request);
    let identifier = body.identifier()?;
    let result = authentication::login(
        &installed,
        LoginInput {
            email: identifier,
            password: &body.password,
            source: &source,
            challenge_id: body.challenge_id.as_deref(),
            challenge_response: body.challenge_response.as_deref(),
            website: body.website.as_deref(),
        },
        state.runtime.password_pepper.as_bytes(),
    )
    .await;
    let result = match result {
        Ok(res) => res,
        Err(LoginFailure::ChallengeRequired {
            challenge_id,
            prompt,
        }) => return Ok(challenge_response(challenge_id, prompt)),
        Err(LoginFailure::Delayed {
            retry_after_seconds,
        }) => return Ok(delayed_response(retry_after_seconds)),
        Err(LoginFailure::Application(AppError::Unauthorized)) => {
            return Ok(HttpResponse::build(StatusCode::UNAUTHORIZED)
                .insert_header((header::CACHE_CONTROL, "no-store"))
                .json(LoginErrorResponse {
                    code: "invalid_credentials",
                    message: "Invalid username or password.",
                    challenge_id: None,
                    challenge_prompt: None,
                    retry_after_seconds: None,
                }));
        }
        Err(LoginFailure::Application(error)) => return Err(error),
    };

    match result {
        authentication::LoginResult::RequiresTwoFactor { temp_token } => {
            Ok(HttpResponse::Ok()
                .insert_header((header::CACHE_CONTROL, "no-store"))
                .json(TwoFactorRequiredResponse {
                    requires2fa: true,
                    temp_token,
                }))
        }
        authentication::LoginResult::Success(outcome) => {
            let cookie = session_cookie(&installed.config, outcome.session_token, Duration::hours(8));
            Ok(HttpResponse::Ok()
                .insert_header((header::CACHE_CONTROL, "no-store"))
                .cookie(cookie)
                .json(user_response(outcome.user, outcome.csrf_token)))
        }
    }
}

async fn verify_2fa(
    state: web::Data<Arc<AppState>>,
    body: web::Json<TwoFactorVerifyRequest>,
) -> Result<HttpResponse, AppError> {
    if body.temp_token.len() > 256 || body.code.len() > 16 {
        return Err(AppError::Validation("invalid 2FA verification request".into()));
    }
    let installed = state.installed().await?;
    let outcome = authentication::verify_2fa_login(&installed, &body.temp_token, &body.code).await?;
    let cookie = session_cookie(&installed.config, outcome.session_token, Duration::hours(8));
    Ok(HttpResponse::Ok()
        .insert_header((header::CACHE_CONTROL, "no-store"))
        .cookie(cookie)
        .json(user_response(outcome.user, outcome.csrf_token)))
}

async fn setup_2fa(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let setup = authentication::setup_2fa(&installed, &user).await?;
    Ok(HttpResponse::Ok().json(setup))
}

async fn enable_2fa(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<TwoFactorEnableRequest>,
) -> Result<HttpResponse, AppError> {
    if body.secret.len() > 512 || body.code.len() > 16 || body.password.len() > 128 {
        return Err(AppError::Validation("invalid 2FA enable request".into()));
    }
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    authentication::enable_2fa(
        &installed,
        &user,
        &body.secret,
        &body.code,
        &body.password,
        state.runtime.password_pepper.as_bytes(),
    )
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn disable_2fa(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<TwoFactorDisableRequest>,
) -> Result<HttpResponse, AppError> {
    if body.code.len() > 16 || body.password.len() > 128 {
        return Err(AppError::Validation("invalid 2FA disable request".into()));
    }
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    authentication::disable_2fa(
        &installed,
        &user,
        &body.code,
        &body.password,
        state.runtime.password_pepper.as_bytes(),
    )
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn me(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let (user, csrf_token) = authentication::current_user(&installed, &request).await?;
    Ok(HttpResponse::Ok()
        .insert_header((header::CACHE_CONTROL, "no-store"))
        .json(user_response(user, csrf_token)))
}

async fn logout(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    authentication::authenticate_mutation(&installed, &request).await?;
    authentication::logout(&installed, &request).await?;
    let mut response = HttpResponse::NoContent();
    for name in [auth::SESSION_COOKIE, auth::DEVELOPMENT_SESSION_COOKIE] {
        response.cookie(expired_cookie(name, installed.config.secure_cookie));
    }
    Ok(response.finish())
}

fn user_response(user: authentication::AuthenticatedUser, csrf_token: String) -> UserResponse {
    UserResponse {
        id: user.id,
        email: user.email,
        username: user.username,
        locale: user.locale,
        roles: user.roles,
        csrf_token,
        totp_enabled: user.totp_enabled,
    }
}

fn session_cookie(
    config: &crate::config::InstallationConfig,
    token: String,
    max_age: Duration,
) -> Cookie<'static> {
    let name = if config.secure_cookie {
        auth::SESSION_COOKIE
    } else {
        auth::DEVELOPMENT_SESSION_COOKIE
    };
    Cookie::build(name, token)
        .http_only(true)
        .secure(config.secure_cookie)
        .same_site(SameSite::Strict)
        .path("/")
        .max_age(max_age)
        .finish()
}

fn expired_cookie(name: &'static str, secure: bool) -> Cookie<'static> {
    Cookie::build(name, "")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Strict)
        .path("/")
        .max_age(Duration::ZERO)
        .finish()
}

fn challenge_response(challenge_id: String, prompt: String) -> HttpResponse {
    HttpResponse::build(StatusCode::UNAUTHORIZED)
        .insert_header((header::CACHE_CONTROL, "no-store"))
        .json(LoginErrorResponse {
            code: "challenge_required",
            message: "Invalid username or password. Complete verification to continue.",
            challenge_id: Some(challenge_id),
            challenge_prompt: Some(prompt),
            retry_after_seconds: None,
        })
}

fn delayed_response(retry_after_seconds: u64) -> HttpResponse {
    HttpResponse::build(StatusCode::TOO_MANY_REQUESTS)
        .insert_header((header::RETRY_AFTER, retry_after_seconds.to_string()))
        .insert_header((header::CACHE_CONTROL, "no-store"))
        .json(LoginErrorResponse {
            code: "rate_limited",
            message: "Sign-in is temporarily delayed. Try again shortly.",
            challenge_id: None,
            challenge_prompt: None,
            retry_after_seconds: Some(retry_after_seconds),
        })
}

fn login_source(request: &HttpRequest) -> String {
    let peer = request
        .peer_addr()
        .map_or_else(|| "unknown".to_owned(), |address| address.ip().to_string());
    let user_agent = request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown");
    format!("{peer}|{user_agent}")
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::LoginRequest;

    #[test]
    fn login_request_supports_username_email_and_both() {
        let req1: LoginRequest =
            serde_json::from_str(r#"{"username":"admin","password":"secret"}"#).expect("json");
        assert_eq!(req1.identifier().expect("id"), "admin");

        let req2: LoginRequest =
            serde_json::from_str(r#"{"email":"admin@example.com","password":"secret"}"#)
                .expect("json");
        assert_eq!(req2.identifier().expect("id"), "admin@example.com");

        let req3: LoginRequest = serde_json::from_str(
            r#"{"username":"admin","email":"admin@example.com","password":"secret"}"#,
        )
        .expect("json");
        assert_eq!(req3.identifier().expect("id"), "admin");
    }
}
