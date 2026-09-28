use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::usage::{Agent, UsageCoverage};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrowthJournalCycle {
    pub cycle_id: String,
    pub started_at_utc: Option<String>,
    pub ended_at_utc: Option<String>,
    pub wallet_credit: Option<u64>,
    pub wallet_credit_at_utc: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrowthJournalEntry {
    pub device_id: String,
    pub cycle_id: String,
    pub bucket_date: String,
    pub agent: Agent,
    pub revision: u64,
    pub generation: u64,
    pub present: bool,
    pub confirmed_tokens: Option<u64>,
    pub coverage: UsageCoverage,
    pub payload_hash: String,
}

impl GrowthJournalEntry {
    pub fn compute_hash(&self) -> String {
        let fields = BTreeMap::from([
            ("agent", serde_json::json!(self.agent)),
            ("bucket_date", serde_json::json!(self.bucket_date)),
            ("confirmed_tokens", serde_json::json!(self.confirmed_tokens)),
            ("coverage", serde_json::json!(self.coverage)),
            ("cycle_id", serde_json::json!(self.cycle_id)),
            ("device_id", serde_json::json!(self.device_id)),
            ("generation", serde_json::json!(self.generation)),
            ("present", serde_json::json!(self.present)),
            ("revision", serde_json::json!(self.revision)),
        ]);
        let bytes = serde_json::to_vec(&fields).expect("growth journal fields serialize");
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn seal(mut self) -> Self {
        self.payload_hash = self.compute_hash();
        self
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrowthJournal {
    pub generation: u64,
    pub deleted_at_utc: Option<String>,
    pub timezone: Option<String>,
    pub cycles: Vec<GrowthJournalCycle>,
    pub entries: Vec<GrowthJournalEntry>,
}
