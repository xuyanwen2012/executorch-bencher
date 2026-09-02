use serde::Serialize;
use std::fmt;
use utoipa::ToSchema;

#[derive(Debug)]
pub struct InvalidValue(String);

impl fmt::Display for InvalidValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for InvalidValue {}

/// A run's process-level outcome. See `specs/benchmark-schema` - "A run
/// captures process-level outcome data".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExitStatus {
    Succeeded,
    Crashed,
    TimedOut,
    Cancelled,
    InfrastructureError,
}

impl ExitStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ExitStatus::Succeeded => "succeeded",
            ExitStatus::Crashed => "crashed",
            ExitStatus::TimedOut => "timed_out",
            ExitStatus::Cancelled => "cancelled",
            ExitStatus::InfrastructureError => "infrastructure_error",
        }
    }
}

impl TryFrom<&str> for ExitStatus {
    type Error = InvalidValue;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "succeeded" => Ok(ExitStatus::Succeeded),
            "crashed" => Ok(ExitStatus::Crashed),
            "timed_out" => Ok(ExitStatus::TimedOut),
            "cancelled" => Ok(ExitStatus::Cancelled),
            "infrastructure_error" => Ok(ExitStatus::InfrastructureError),
            other => Err(InvalidValue(format!("invalid exit status: {other:?}"))),
        }
    }
}

/// A run's correctness-validation outcome, independent of `ExitStatus`. See
/// `specs/benchmark-schema` - "Correctness validation is tracked
/// independently of process exit status".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CorrectnessResult {
    Passed,
    Failed,
    NotChecked,
    ValidatorError,
}

impl CorrectnessResult {
    pub fn as_str(self) -> &'static str {
        match self {
            CorrectnessResult::Passed => "passed",
            CorrectnessResult::Failed => "failed",
            CorrectnessResult::NotChecked => "not_checked",
            CorrectnessResult::ValidatorError => "validator_error",
        }
    }
}

impl TryFrom<&str> for CorrectnessResult {
    type Error = InvalidValue;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "passed" => Ok(CorrectnessResult::Passed),
            "failed" => Ok(CorrectnessResult::Failed),
            "not_checked" => Ok(CorrectnessResult::NotChecked),
            "validator_error" => Ok(CorrectnessResult::ValidatorError),
            other => Err(InvalidValue(format!(
                "invalid correctness result: {other:?}"
            ))),
        }
    }
}

/// A validated 64-character lowercase hexadecimal SHA-256 digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sha256Hex(String);

impl Sha256Hex {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Sha256Hex {
    type Error = InvalidValue;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let is_valid = value.len() == 64
            && value
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
        if is_valid {
            Ok(Sha256Hex(value))
        } else {
            Err(InvalidValue(format!(
                "not a 64-character lowercase hexadecimal SHA-256 digest: {value:?}"
            )))
        }
    }
}

impl fmt::Display for Sha256Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Parses `raw` as JSON and re-serializes it in canonical form (sorted
/// object keys, no incidental whitespace).
fn canonicalize(raw: &str) -> Result<(serde_json::Value, String), InvalidValue> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|err| InvalidValue(format!("invalid JSON: {err}")))?;
    let text = serde_json::to_string(&value)
        .map_err(|err| InvalidValue(format!("failed to canonicalize JSON: {err}")))?;
    Ok((value, text))
}

/// Validates a command-line argument array: well-formed JSON that
/// deserializes as an array of strings.
pub fn validate_command_args(raw: &str) -> Result<String, InvalidValue> {
    let args: Vec<String> = serde_json::from_str(raw).map_err(|err| {
        InvalidValue(format!(
            "command args must be a JSON array of strings: {err}"
        ))
    })?;
    serde_json::to_string(&args)
        .map_err(|err| InvalidValue(format!("failed to canonicalize command args: {err}")))
}

/// Validates arbitrary caller-defined JSON (used for input parameters):
/// well-formedness only.
pub fn validate_json(raw: &str) -> Result<String, InvalidValue> {
    canonicalize(raw).map(|(_, text)| text)
}

/// Validates an environment-variable allowlist capture: well-formed JSON
/// that is a top-level object (mapping variable name to its captured
/// value), preserving `null` (unset) vs. `""` (empty) as distinct values.
pub fn validate_env_vars(raw: &str) -> Result<String, InvalidValue> {
    let (value, text) = canonicalize(raw)?;
    if value.is_object() {
        Ok(text)
    } else {
        Err(InvalidValue(
            "environment-variable allowlist capture must be a JSON object".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_status_rejects_unknown_value() {
        assert!(ExitStatus::try_from("bogus").is_err());
    }

    #[test]
    fn exit_status_round_trips_known_values() {
        for s in [
            "succeeded",
            "crashed",
            "timed_out",
            "cancelled",
            "infrastructure_error",
        ] {
            let status = ExitStatus::try_from(s).expect("known value should parse");
            assert_eq!(status.as_str(), s);
        }
    }

    #[test]
    fn correctness_result_rejects_unknown_value() {
        assert!(CorrectnessResult::try_from("bogus").is_err());
    }

    #[test]
    fn sha256_accepts_valid_digest() {
        let digest = "a".repeat(64);
        assert!(Sha256Hex::try_from(digest).is_ok());
    }

    #[test]
    fn sha256_rejects_too_short() {
        let digest = "a".repeat(63);
        assert!(Sha256Hex::try_from(digest).is_err());
    }

    #[test]
    fn sha256_rejects_uppercase() {
        let digest = "A".repeat(64);
        assert!(Sha256Hex::try_from(digest).is_err());
    }

    #[test]
    fn sha256_rejects_non_hex_characters() {
        let digest = "g".repeat(64);
        assert!(Sha256Hex::try_from(digest).is_err());
    }

    #[test]
    fn validate_command_args_accepts_string_array() {
        assert!(validate_command_args(r#"["--flag","value"]"#).is_ok());
    }

    #[test]
    fn validate_command_args_rejects_non_string_array() {
        assert!(validate_command_args(r#"[1,2,3]"#).is_err());
    }

    #[test]
    fn validate_command_args_rejects_malformed_json() {
        assert!(validate_command_args("not json").is_err());
    }

    #[test]
    fn validate_json_accepts_any_well_formed_value() {
        assert!(validate_json(r#"{"a":1}"#).is_ok());
        assert!(validate_json(r#"[1,2,3]"#).is_ok());
    }

    #[test]
    fn validate_json_rejects_malformed_json() {
        assert!(validate_json("{not json}").is_err());
    }

    #[test]
    fn validate_env_vars_preserves_unset_vs_empty() {
        let text = validate_env_vars(r#"{"FOO":null,"BAR":""}"#).expect("should validate");
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(value["FOO"].is_null());
        assert_eq!(value["BAR"], serde_json::Value::String(String::new()));
    }

    #[test]
    fn validate_env_vars_rejects_non_object() {
        assert!(validate_env_vars(r#"["FOO"]"#).is_err());
    }
}

/// The kind of host a run executed on. Selects which host-state snapshot a
/// run carries. See `specs/benchmark-schema` - "Host state is captured as a
/// platform-specific immutable snapshot".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// An Android phone identified by its device serial.
    Android,
    /// A Linux workstation or server identified by its hostname.
    Linux,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Android => "android",
            Platform::Linux => "linux",
        }
    }
}

impl TryFrom<&str> for Platform {
    type Error = InvalidValue;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "android" => Ok(Platform::Android),
            "linux" => Ok(Platform::Linux),
            other => Err(InvalidValue(format!("invalid platform: {other:?}"))),
        }
    }
}

/// Whether a host is a lab device under full control or an external one.
/// Internal devices (rooted development phones with SUMD/BSP/clock control)
/// must carry the rigorous Android snapshot; external hosts (retail phones,
/// any Linux box) record whatever they can report. See
/// `specs/benchmark-schema` - "Host state is captured as a
/// platform-specific immutable snapshot".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    Internal,
    External,
}

impl DeviceClass {
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceClass::Internal => "internal",
            DeviceClass::External => "external",
        }
    }
}

impl TryFrom<&str> for DeviceClass {
    type Error = InvalidValue;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "internal" => Ok(DeviceClass::Internal),
            "external" => Ok(DeviceClass::External),
            other => Err(InvalidValue(format!("invalid device class: {other:?}"))),
        }
    }
}
