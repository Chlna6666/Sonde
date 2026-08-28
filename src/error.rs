use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("authentication required")]
    Unauthorized,
    #[error("permission denied")]
    Forbidden,
    #[error("resource not found")]
    NotFound,
    #[error("Sonde has not been initialized")]
    NotInitialized,
    #[error("Sonde is already initialized")]
    AlreadyInitialized,
    #[error("invalid request: {0}")]
    Validation(String),
    #[error("request payload is too large")]
    PayloadTooLarge,
    #[error("password must be 15..128 characters")]
    PasswordLength,
    #[error("password appears in the blocked password list")]
    PasswordBlocked,
    #[error("rate limit exceeded")]
    TooManyRequests,
    #[error("database operation failed")]
    Database(#[from] sea_orm::DbErr),
    #[error("internal operation failed")]
    Internal,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: String,
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::NotInitialized => StatusCode::SERVICE_UNAVAILABLE,
            Self::AlreadyInitialized => StatusCode::CONFLICT,
            Self::Validation(_) | Self::PasswordLength | Self::PasswordBlocked => {
                StatusCode::BAD_REQUEST
            }
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            Self::Database(_) | Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let code = match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::NotInitialized => "not_initialized",
            Self::AlreadyInitialized => "already_initialized",
            Self::Validation(_) => "validation_error",
            Self::PayloadTooLarge => "payload_too_large",
            Self::PasswordLength => "password_length",
            Self::PasswordBlocked => "password_blocked",
            Self::TooManyRequests => "rate_limited",
            Self::Database(_) | Self::Internal => "internal_error",
        };
        HttpResponse::build(self.status_code()).json(ErrorBody {
            code,
            message: self.to_string(),
        })
    }
}
