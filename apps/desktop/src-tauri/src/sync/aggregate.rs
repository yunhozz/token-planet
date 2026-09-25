use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::usage::{Agent, UsageCoverage};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DailyUsageSnapshot {
    pub device_id: String,
    pub bucket_date: String,
    pub bucket_policy_version: u16,
    pub agent: Agent,
    pub schema_version: u16,
    pub revision: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub coverage: UsageCoverage,
    pub payload_hash: String,
}

impl DailyUsageSnapshot {
    pub fn compute_hash(&self) -> String {
        let fields = BTreeMap::from([
            ("agent", serde_json::json!(self.agent)),
            ("bucket_date", serde_json::json!(self.bucket_date)),
            (
                "bucket_policy_version",
                serde_json::json!(self.bucket_policy_version),
            ),
            (
                "cache_read_tokens",
                serde_json::json!(self.cache_read_tokens),
            ),
            (
                "cache_write_tokens",
                serde_json::json!(self.cache_write_tokens),
            ),
            ("coverage", serde_json::json!(self.coverage)),
            ("device_id", serde_json::json!(self.device_id)),
            ("input_tokens", serde_json::json!(self.input_tokens)),
            ("output_tokens", serde_json::json!(self.output_tokens)),
            ("revision", serde_json::json!(self.revision)),
            ("schema_version", serde_json::json!(self.schema_version)),
            ("total_tokens", serde_json::json!(self.total_tokens)),
        ]);
        let bytes = serde_json::to_vec(&fields).expect("aggregate fields serialize");
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

#[cfg(test)]
mod tests {
    use super::DailyUsageSnapshot;
    use crate::domain::usage::{Agent, UsageCoverage};

    fn sample() -> DailyUsageSnapshot {
        DailyUsageSnapshot {
            device_id: "device-1".into(),
            bucket_date: "2026-09-25".into(),
            bucket_policy_version: 1,
            agent: Agent::Codex,
            schema_version: 1,
            revision: 1,
            input_tokens: Some(30),
            output_tokens: Some(12),
            cache_read_tokens: Some(8),
            cache_write_tokens: Some(0),
            total_tokens: Some(42),
            coverage: UsageCoverage::Complete,
            payload_hash: String::new(),
        }
    }

    #[test]
    fn snapshot_serialization_excludes_source_identity() {
        let value = serde_json::to_value(sample()).unwrap();
        let keys: Vec<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "agent",
                "bucket_date",
                "bucket_policy_version",
                "cache_read_tokens",
                "cache_write_tokens",
                "coverage",
                "device_id",
                "input_tokens",
                "output_tokens",
                "payload_hash",
                "revision",
                "schema_version",
                "total_tokens",
            ]
        );
        for forbidden in [
            "session_id",
            "source_path",
            "prompt",
            "transcript",
            "message",
        ] {
            assert!(value.get(forbidden).is_none());
        }
    }

    #[test]
    fn unavailable_and_partial_counts_never_become_zero() {
        let mut unavailable = sample();
        unavailable.total_tokens = None;
        unavailable.input_tokens = None;
        unavailable.output_tokens = None;
        unavailable.cache_read_tokens = None;
        unavailable.cache_write_tokens = None;
        unavailable.coverage = UsageCoverage::Unavailable;
        let value = serde_json::to_value(&unavailable).unwrap();
        assert!(value["total_tokens"].is_null());
        assert_eq!(value["coverage"], "unavailable");
        let mut partial = unavailable;
        partial.total_tokens = Some(42);
        partial.coverage = UsageCoverage::Partial;
        let value = serde_json::to_value(partial).unwrap();
        assert_eq!(value["total_tokens"], 42);
        assert_eq!(value["coverage"], "partial");
    }

    #[test]
    fn aggregate_hash_is_stable_and_changes_when_count_changes() {
        let original = sample();
        assert_eq!(original.compute_hash(), original.compute_hash());
        let mut changed = original.clone();
        changed.total_tokens = Some(43);
        assert_ne!(original.compute_hash(), changed.compute_hash());
        changed = original.clone();
        changed.revision += 1;
        assert_ne!(original.compute_hash(), changed.compute_hash());
    }
}
