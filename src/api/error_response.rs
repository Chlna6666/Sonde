use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use serde::Serialize;

use crate::error::AppError;

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
