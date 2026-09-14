use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFactsInput {
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    pub system_language: Option<String>,
    pub architecture: Option<String>,
}

impl DeviceFactsInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_optional(
            &self.app_version,
            128,
            "appVersion must be at most 128 bytes",
        )?;
        validate_optional(
            &self.launcher_version,
            128,
            "launcherVersion must be at most 128 bytes",
        )?;
        validate_optional(&self.os, 256, "os must be at most 256 bytes")?;
        validate_optional(
            &self.system_language,
            64,
            "systemLanguage must be at most 64 bytes",
        )?;
        validate_optional(
            &self.architecture,
            64,
            "architecture must be at most 64 bytes",
        )?;
        if self.app_version.is_none()
            && self.launcher_version.is_none()
            && self.os.is_none()
            && self.system_language.is_none()
            && self.architecture.is_none()
        {
            return Err("at least one device fact is required");
        }
        Ok(())
    }
}

fn validate_optional(
    value: &Option<String>,
    max_bytes: usize,
    message: &'static str,
) -> Result<(), &'static str> {
    if let Some(value) = value
        && (value.is_empty()
            || value.len() > max_bytes
            || value.chars().any(|character| character.is_control()))
    {
        return Err(message);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::DeviceFactsInput;

    #[test]
    fn device_facts_require_at_least_one_current_value() {
        assert!(DeviceFactsInput::default().validate().is_err());
        assert!(
            DeviceFactsInput {
                system_language: Some("zh-CN".into()),
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
    }
}
