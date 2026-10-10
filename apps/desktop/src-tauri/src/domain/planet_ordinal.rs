use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanetOrdinalStatus {
    Verified,
    Unknown,
}
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct PlanetOrdinal {
    pub status: PlanetOrdinalStatus,
    pub current: Option<u64>,
}
impl PlanetOrdinal {
    pub fn unknown() -> Self {
        Self {
            status: PlanetOrdinalStatus::Unknown,
            current: None,
        }
    }
}
