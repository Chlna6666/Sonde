use actix_web::HttpRequest;

use crate::{
    auth,
    services::authentication::AuthRequest,
};

impl AuthRequest for HttpRequest {
    fn session_token(&self) -> Option<String> {
        self.cookie(auth::SESSION_COOKIE)
            .or_else(|| self.cookie(auth::DEVELOPMENT_SESSION_COOKIE))
            .map(|cookie| cookie.value().to_owned())
    }

    fn csrf_token(&self) -> Option<&str> {
        self.headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
    }
}

pub(crate) fn bearer_token(request: &HttpRequest) -> Option<&str> {
    request
        .headers()
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}
