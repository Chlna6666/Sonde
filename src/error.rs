use sea_orm::{DbErr, SqlErr};
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
    #[error("resource conflict: {0}")]
    Conflict(String),
    #[error("unsupported media type: {0}")]
    UnsupportedMediaType(String),
    #[error("request payload is too large")]
    PayloadTooLarge,
    #[error("password must be 15..128 characters")]
    PasswordLength,
    #[error("password appears in the blocked password list")]
    PasswordBlocked,
    #[error("rate limit exceeded")]
    TooManyRequests,
    #[error("database operation failed")]
    Database(DbErr),
    #[error("service temporarily unavailable: {context}: {detail}")]
    ServiceUnavailable {
        context: &'static str,
        detail: String,
    },
    #[error("upstream service failed: {context}: {detail}")]
    Upstream {
        context: &'static str,
        detail: String,
    },
    #[error("internal operation failed")]
    Internal,
    #[error("internal operation failed: {context}: {detail}")]
    InternalContext {
        context: &'static str,
        detail: String,
    },
}

impl AppError {
    pub fn internal(context: &'static str, error: impl std::fmt::Display) -> Self {
        Self::InternalContext {
            context,
            detail: error.to_string(),
        }
    }

    pub fn unavailable(context: &'static str, error: impl std::fmt::Display) -> Self {
        Self::ServiceUnavailable {
            context,
            detail: error.to_string(),
        }
    }

    pub fn upstream(context: &'static str, error: impl std::fmt::Display) -> Self {
        Self::Upstream {
            context,
            detail: error.to_string(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::NotInitialized => "not_initialized",
            Self::AlreadyInitialized => "already_initialized",
            Self::Validation(_) => "validation_error",
            Self::Conflict(_) => "conflict",
            Self::UnsupportedMediaType(_) => "unsupported_media_type",
            Self::PayloadTooLarge => "payload_too_large",
            Self::PasswordLength => "password_length",
            Self::PasswordBlocked => "password_blocked",
            Self::TooManyRequests => "rate_limited",
            Self::Database(_) | Self::Internal | Self::InternalContext { .. } => "internal_error",
            Self::ServiceUnavailable { .. } => "service_unavailable",
            Self::Upstream { .. } => "upstream_error",
        }
    }

    pub fn public_message(&self) -> String {
        match self {
            Self::Database(_) => "database operation failed".into(),
            Self::Internal | Self::InternalContext { .. } => "internal operation failed".into(),
            Self::ServiceUnavailable { .. } => "service temporarily unavailable".into(),
            Self::Upstream { .. } => "upstream service request failed".into(),
            _ => self.to_string(),
        }
    }

    pub fn is_server_failure(&self) -> bool {
        matches!(
            self,
            Self::Database(_)
                | Self::ServiceUnavailable { .. }
                | Self::Upstream { .. }
                | Self::Internal
                | Self::InternalContext { .. }
        )
    }
}

impl From<DbErr> for AppError {
    fn from(error: DbErr) -> Self {
        match error.sql_err() {
            Some(SqlErr::UniqueConstraintViolation(_)) => {
                Self::Conflict("resource already exists".into())
            }
            Some(SqlErr::ForeignKeyConstraintViolation(_)) => {
                Self::Conflict("resource conflicts with related data".into())
            }
            _ => Self::Database(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn internal_context_is_redacted_from_public_message() {
        let error = AppError::internal("read secret config", "sensitive/path/value");
        assert_eq!(error.code(), "internal_error");
        assert_eq!(error.public_message(), "internal operation failed");
        assert!(error.is_server_failure());
        assert!(error.to_string().contains("sensitive/path/value"));
    }

    #[test]
    fn validation_message_remains_actionable() {
        let error = AppError::Validation("field is required".into());
        assert_eq!(error.code(), "validation_error");
        assert_eq!(error.public_message(), "invalid request: field is required");
        assert!(!error.is_server_failure());
    }
}
