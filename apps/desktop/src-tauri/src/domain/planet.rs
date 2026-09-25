use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanetAvatar {
    Masculine,
    Feminine,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlanetProfile {
    pub nickname: String,
    pub avatar: PlanetAvatar,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlanetObject {
    pub stage: u8,
    pub ordinal: u32,
    pub kind: String,
    pub x: u8,
    pub y: u8,
    pub seed: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlanetWalletCredit {
    pub previous_cycle_id: String,
    pub amount: u64,
    pub created_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlanetState {
    pub version: u8,
    pub profile: Option<PlanetProfile>,
    pub timezone: String,
    pub current_cycle_id: String,
    pub cycle_started_at_utc: String,
    pub last_reset_at_utc: Option<String>,
    pub wallet_balance: u64,
    pub wallet_credits: Vec<PlanetWalletCredit>,
    pub current_planet_tokens: u64,
    pub lifetime_tokens: u64,
    pub growth_credit: f64,
    pub stage: u8,
    pub progress_to_next: f64,
    pub incomplete: bool,
    pub can_reset: bool,
    pub reset_available_at_utc: Option<String>,
    pub objects: Vec<PlanetObject>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlanetDeviceContribution {
    pub device_id: String,
    pub current_cycle_id: String,
    pub lifetime_tokens: u64,
    pub current_planet_tokens: u64,
    pub daily_tokens: BTreeMap<String, u64>,
    pub incomplete: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorldPlanet {
    pub nickname: String,
    pub avatar: PlanetAvatar,
    pub stage: u8,
    pub current_planet_tokens: u64,
    pub lifetime_tokens: u64,
    pub growth_credit: f64,
    pub progress_to_next: f64,
    pub incomplete: bool,
    pub objects: Vec<PlanetObject>,
    pub token_rank: u8,
    pub civilization_rank: u8,
}
