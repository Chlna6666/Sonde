use actix_web::{
    HttpResponse, ResponseError,
    error::{JsonPayloadError, PathError, QueryPayloadError},
    http::StatusCode,
    web,
};
use serde::Serialize;

use crate::error::AppError;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody<'a> {
    code: &'a str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_id: Option<String>,
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Unauthorized | Self::IngestTokenRequired => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::NotInitialized | Self::ServiceUnavailable { .. } => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            Self::AlreadyInitialized | Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Validation(_) | Self::PasswordLength | Self::PasswordBlocked => {
                StatusCode::BAD_REQUEST
            }
            Self::UnsupportedMediaType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            Self::Upstream { .. } => StatusCode::BAD_GATEWAY,
            Self::Database(_) | Self::InternalContext { .. } => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    fn error_response(&self) -> HttpResponse {
        let error_id = if self.is_server_failure() {
            let error_id = uuid::Uuid::now_v7().to_string();
            tracing::error!(
                error_id = %error_id,
                code = self.code(),
                error = ?self,
                "request failed"
            );
            Some(error_id)
        } else {
            None
        };

        HttpResponse::build(self.status_code())
            .insert_header(("cache-control", "no-store"))
            .json(ErrorBody {
                code: self.code(),
                message: self.public_message(),
                error_id,
            })
    }
}

pub(crate) fn json_config(limit: usize) -> web::JsonConfig {
    web::JsonConfig::default()
        .limit(limit)
        .error_handler(|error, _request| json_payload_error(error).into())
}

pub(crate) fn query_config() -> web::QueryConfig {
    web::QueryConfig::default().error_handler(|error, _request| {
        let error = match error {
            QueryPayloadError::Deserialize(error) => {
                AppError::Validation(format!("invalid query parameters: {error}"))
            }
            other => AppError::Validation(format!("invalid query parameters: {other}")),
        };
        error.into()
    })
}

pub(crate) fn path_config() -> web::PathConfig {
    web::PathConfig::default().error_handler(|error, _request| {
        let error = match error {
            PathError::Deserialize(error) => {
                AppError::Validation(format!("invalid route parameters: {error}"))
            }
            other => AppError::Validation(format!("invalid route parameters: {other}")),
        };
        error.into()
    })
}

fn json_payload_error(error: JsonPayloadError) -> AppError {
    match error {
        JsonPayloadError::OverflowKnownLength { .. } | JsonPayloadError::Overflow { .. } => {
            AppError::PayloadTooLarge
        }
        JsonPayloadError::ContentType => {
            AppError::UnsupportedMediaType("content-type must be application/json".into())
        }
        JsonPayloadError::Deserialize(_) => {
            AppError::Validation("invalid JSON request body".into())
        }
        JsonPayloadError::Payload(error) => {
            AppError::Validation(format!("request body could not be read: {error}"))
        }
        JsonPayloadError::Serialize(error) => {
            AppError::internal("serialize extracted JSON request", error)
        }
        other => AppError::internal("extract JSON request body", other),
    }
}

#[cfg(test)]
mod tests {
    use actix_web::{ResponseError, http::StatusCode};

    use crate::error::AppError;

    #[test]
    fn infrastructure_errors_use_gateway_or_service_statuses() {
        assert_eq!(
            AppError::upstream("send webhook", "connection refused").status_code(),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            AppError::unavailable("ingest writer", "closed").status_code(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            AppError::internal("serialize token", "unexpected state").status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn direct_api_key_ingest_requires_device_token() {
        assert_eq!(
            AppError::IngestTokenRequired.status_code(),
            StatusCode::UNAUTHORIZED
        );
    }
}
