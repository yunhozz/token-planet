use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::sync::aggregate::DailyUsageSnapshot;

#[derive(Debug, Eq, PartialEq)]
pub enum SyncError {
    InvalidSnapshot,
    Transport,
    Rejected(u16),
    InvalidResponse,
}

#[derive(Serialize)]
struct UploadBody<'a> {
    p_world_id: &'a str,
    p_snapshot: &'a DailyUsageSnapshot,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InviteLink {
    pub invite_id: String,
    pub code: String,
    pub expires_at: String,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldSummary {
    pub world_id: String,
    pub member_count: u8,
    pub known_tokens: Option<u64>,
    pub growth_credit: f64,
    pub stage: u8,
    pub progress_to_next: f64,
    pub incomplete: bool,
    pub last_update: Option<String>,
}

pub struct SupabaseSyncClient {
    http: Client,
    base_url: String,
    publishable_key: String,
}

impl SupabaseSyncClient {
    pub fn new(base_url: &str, publishable_key: &str) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.trim_end_matches('/').to_owned(),
            publishable_key: publishable_key.to_owned(),
        }
    }

    pub async fn upload_snapshot(
        &self,
        access_token: &str,
        world_id: &str,
        snapshot: &DailyUsageSnapshot,
    ) -> Result<u64, SyncError> {
        if snapshot.payload_hash != snapshot.compute_hash() {
            return Err(SyncError::InvalidSnapshot);
        }
        self.post_rpc(
            access_token,
            "upload_daily_snapshot",
            &UploadBody {
                p_world_id: world_id,
                p_snapshot: snapshot,
            },
        )
        .await
    }

    pub async fn create_invite(
        &self,
        access_token: &str,
        world_id: &str,
    ) -> Result<InviteLink, SyncError> {
        let rows: Vec<InviteLink> = self
            .post_rpc(
                access_token,
                "create_world_invite",
                &serde_json::json!({ "p_world_id": world_id }),
            )
            .await?;
        one_row(rows)
    }

    pub async fn accept_invite(
        &self,
        access_token: &str,
        code: &str,
    ) -> Result<WorldSummary, SyncError> {
        let rows: Vec<WorldSummary> = self
            .post_rpc(
                access_token,
                "accept_world_invite",
                &serde_json::json!({ "p_code": code }),
            )
            .await?;
        one_row(rows)
    }

    pub async fn revoke_invite(
        &self,
        access_token: &str,
        invite_id: &str,
    ) -> Result<bool, SyncError> {
        self.post_rpc(
            access_token,
            "revoke_world_invite",
            &serde_json::json!({ "p_invite_id": invite_id }),
        )
        .await
    }

    pub async fn world_summary(
        &self,
        access_token: &str,
        world_id: &str,
    ) -> Result<WorldSummary, SyncError> {
        let rows: Vec<WorldSummary> = self
            .post_rpc(
                access_token,
                "get_world_summary",
                &serde_json::json!({ "p_world_id": world_id }),
            )
            .await?;
        one_row(rows)
    }

    async fn post_rpc<P: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        access_token: &str,
        function: &str,
        body: &P,
    ) -> Result<R, SyncError> {
        let response = self
            .http
            .post(format!("{}/rest/v1/rpc/{function}", self.base_url))
            .header("apikey", &self.publishable_key)
            .bearer_auth(access_token)
            .json(body)
            .send()
            .await
            .map_err(|_| SyncError::Transport)?;
        if !response.status().is_success() {
            return Err(SyncError::Rejected(response.status().as_u16()));
        }
        response
            .json::<R>()
            .await
            .map_err(|_| SyncError::InvalidResponse)
    }
}

fn one_row<T>(mut rows: Vec<T>) -> Result<T, SyncError> {
    if rows.len() != 1 {
        return Err(SyncError::InvalidResponse);
    }
    Ok(rows.remove(0))
}

#[cfg(test)]
mod tests {
    use super::{one_row, SyncError, UploadBody, WorldSummary};
    use crate::domain::usage::{Agent, UsageCoverage};
    use crate::sync::aggregate::DailyUsageSnapshot;

    #[test]
    fn rpc_body_contains_only_world_id_and_daily_aggregate() {
        let snapshot = DailyUsageSnapshot {
            device_id: "60000000-0000-0000-0000-000000000001".into(),
            bucket_date: "2026-09-25".into(),
            bucket_policy_version: 1,
            agent: Agent::Codex,
            schema_version: 1,
            revision: 1,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: Some(42),
            coverage: UsageCoverage::Complete,
            payload_hash: String::new(),
        }
        .seal();
        let body = serde_json::to_value(UploadBody {
            p_world_id: "50000000-0000-0000-0000-000000000001",
            p_snapshot: &snapshot,
        })
        .unwrap();
        assert_eq!(body.as_object().unwrap().len(), 2);
        assert_eq!(body["p_snapshot"]["total_tokens"], 42);
        for secret in [
            "source_path",
            "session_id",
            "prompt",
            "transcript",
            "message",
            "user_id",
        ] {
            assert!(body["p_snapshot"].get(secret).is_none());
        }
    }

    #[test]
    fn world_response_is_group_only_and_requires_one_row() {
        let value = serde_json::json!({
            "world_id": "50000000-0000-0000-0000-000000000001",
            "member_count": 2,
            "known_tokens": 100000,
            "growth_credit": 1.0,
            "stage": 0,
            "progress_to_next": 0.2,
            "incomplete": false,
            "last_update": null
        });
        let summary: WorldSummary = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(summary.member_count, 2);
        for forbidden in ["user_id", "device_id", "member_totals", "source_path"] {
            assert!(value.get(forbidden).is_none());
        }
        let mut contaminated = value;
        contaminated["member_totals"] = serde_json::json!([42]);
        assert!(serde_json::from_value::<WorldSummary>(contaminated).is_err());
        assert!(matches!(
            one_row::<WorldSummary>(vec![]),
            Err(SyncError::InvalidResponse)
        ));
        assert!(one_row(vec![summary]).is_ok());
    }
}
