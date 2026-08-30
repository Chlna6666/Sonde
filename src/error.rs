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
