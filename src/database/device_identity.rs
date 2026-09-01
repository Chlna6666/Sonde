use sha2::{Digest, Sha256};

use super::telemetry::TelemetryScope;

/// Derive the persisted pseudonymous device identity for one application environment.
///
/// The source value is already expected to be pseudonymous. Scope hashing prevents the same source
/// identifier from becoming a cross-application correlation key or colliding in the global device
/// profile primary key.
pub fn scoped_hash(scope: &TelemetryScope, value: &str) -> String {
    scoped_hash_parts(&scope.application_id, &scope.environment_id, value)
}

pub fn scoped_hash_parts(application_id: &str, environment_id: &str, value: &str) -> String {
    let salt = format!("{application_id}:{environment_id}");
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher.update(salt.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::scoped_hash_parts;

    #[test]
    fn same_source_identity_isolated_between_scopes() {
        let one = scoped_hash_parts("app-a", "prod", "source-device");
        let two = scoped_hash_parts("app-b", "prod", "source-device");
        assert_eq!(one.len(), 64);
        assert_ne!(one, two);
    }
}
