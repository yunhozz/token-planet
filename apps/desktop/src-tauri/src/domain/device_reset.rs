use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceResetPhase {
    #[default]
    Idle,
    Pending,
    LocalCommitted,
    Completed,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeviceResetState {
    pub request_id: Option<String>,
    pub generation: u64,
    pub phase: DeviceResetPhase,
    pub cutoff_at_utc: Option<String>,
    pub new_cycle_id: Option<String>,
    pub new_device_id: Option<String>,
    pub new_lineage_id: Option<String>,
    pub service_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct LocalContext {
    pub generation: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExpectedPlanetContext {
    pub generation: u64,
    pub account_id: String,
    pub current_cycle_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocalEnvelope<T> {
    pub generation: u64,
    pub data: T,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocalCommandError {
    pub generation: u64,
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

impl LocalCommandError {
    pub fn new(generation: u64, code: &str, message: impl Into<String>) -> Self {
        Self {
            generation,
            code: code.to_owned(),
            message: message.into(),
            details: None,
        }
    }
    pub fn from_error(generation: u64, error: impl Serialize) -> Self {
        let details = serde_json::to_value(error).ok();
        let message = details
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .unwrap_or("작업 결과를 확인할 수 없습니다")
            .to_owned();
        Self {
            generation,
            code: "command_failed".into(),
            message,
            details,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DeviceResetView {
    pub state: DeviceResetState,
    pub actions_blocked: bool,
    pub storage_completed: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct DeviceResetResult {
    pub state: DeviceResetState,
    pub storage_completed: bool,
}
