#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSummary {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub retention_days: i32,
    pub owner_user_id: Option<String>,
    pub is_public: bool,
    pub description: Option<String>,
    pub github_url: Option<String>,
    pub website_url: Option<String>,
    pub custom_header: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicApplicationInfo {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub is_public: bool,
    pub description: Option<String>,
    pub github_url: Option<String>,
    pub website_url: Option<String>,
    pub custom_header: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMemberSummary {
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub role: String,
    pub granted_at: i64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSummary {
    pub id: String,
    pub application_id: String,
    pub name: String,
    pub slug: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyDetail {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub environment_name: String,
    pub name: String,
    pub key_prefix: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub created_at: i64,
    pub is_active: bool,
}

pub struct UpdateApplicationParams<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub retention_days: i32,
    pub is_public: Option<bool>,
    pub description: Option<Option<String>>,
    pub github_url: Option<Option<String>>,
    pub website_url: Option<Option<String>>,
    pub custom_header: Option<Option<String>>,
}
