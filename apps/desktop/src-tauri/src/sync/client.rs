use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{sync::OnceLock, time::Duration};

use crate::domain::cosmetic_shop::{
    CosmeticEquipResult, CosmeticPurchaseResult, CosmeticShopState, GuestCosmeticImport,
    GuestCosmeticImportResult,
};
use crate::domain::growth_journal::{GrowthJournal, GrowthJournalCycle, GrowthJournalEntry};
use crate::domain::planet::{PlanetDeviceContribution, PlanetState, WorldPlanet};
use crate::sync::aggregate::DailyUsageSnapshot;

pub(crate) fn shared_http_client() -> Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(20))
                .build()
                .expect("shared HTTP client configuration is valid")
        })
        .clone()
}

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

fn purchase_cosmetic_rpc_body(purchase_id: &str, sku: &str, catalog_revision: u32) -> serde_json::Value {
    serde_json::json!({
        "p_purchase_id": purchase_id,
        "p_sku": sku,
        "p_catalog_revision": catalog_revision,
    })
}

fn equip_cosmetic_rpc_body(
    cycle_id: &str,
    slot_id: &str,
    sku: Option<&str>,
    expected_version: u64,
) -> serde_json::Value {
    serde_json::json!({
        "p_cycle_id": cycle_id,
        "p_slot_id": slot_id,
        "p_sku": sku,
        "p_expected_version": expected_version,
    })
}

fn guest_import_rpc_body(import: &GuestCosmeticImport) -> serde_json::Value {
    serde_json::json!({
        "p_import_id": import.import_id,
        "p_wallet_credits": import.wallet_credits,
        "p_purchases": import.purchases,
    })
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldShell {
    pub id: String,
    pub name: String,
    pub timezone: String,
    pub owner_id: String,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldMember {
    pub user_id: String,
    pub role: String,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
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

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MySyncPolicy {
    pub deleted_through: Option<String>,
    pub paused: bool,
}

pub struct SupabaseSyncClient {
    http: Client,
    base_url: String,
    publishable_key: String,
}

impl SupabaseSyncClient {
    pub fn new(base_url: &str, publishable_key: &str) -> Self {
        Self {
            http: shared_http_client(),
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

    pub async fn my_planet_state(
        &self,
        access_token: &str,
    ) -> Result<Option<PlanetState>, SyncError> {
        self.post_rpc(access_token, "get_my_planet_state", &serde_json::json!({}))
            .await
    }

    pub async fn cosmetic_shop_state(
        &self,
        access_token: &str,
    ) -> Result<CosmeticShopState, SyncError> {
        self.post_rpc(access_token, "get_my_cosmetic_state", &serde_json::json!({}))
            .await
    }

    pub async fn purchase_cosmetic(
        &self,
        access_token: &str,
        purchase_id: &str,
        sku: &str,
        catalog_revision: u32,
    ) -> Result<CosmeticPurchaseResult, SyncError> {
        self.post_rpc(
            access_token,
            "purchase_my_cosmetic",
            &purchase_cosmetic_rpc_body(purchase_id, sku, catalog_revision),
        )
        .await
    }

    pub async fn equip_cosmetic(
        &self,
        access_token: &str,
        cycle_id: &str,
        slot_id: &str,
        sku: Option<&str>,
        expected_version: u64,
    ) -> Result<CosmeticEquipResult, SyncError> {
        self.post_rpc(
            access_token,
            "equip_my_cosmetic",
            &equip_cosmetic_rpc_body(cycle_id, slot_id, sku, expected_version),
        )
        .await
    }

    pub async fn import_guest_cosmetics(
        &self,
        access_token: &str,
        import: &GuestCosmeticImport,
    ) -> Result<GuestCosmeticImportResult, SyncError> {
        self.post_rpc(
            access_token,
            "import_my_guest_cosmetics",
            &guest_import_rpc_body(import),
        )
        .await
    }

    pub async fn upload_planet_state(
        &self,
        access_token: &str,
        state: &PlanetState,
        contribution: &PlanetDeviceContribution,
    ) -> Result<PlanetState, SyncError> {
        self.post_rpc(
            access_token,
            "upsert_my_planet_state",
            &serde_json::json!({ "p_state": state, "p_device_contribution": contribution }),
        )
        .await
    }

    pub async fn growth_journal(&self, access_token: &str) -> Result<GrowthJournal, SyncError> {
        self.post_rpc(
            access_token,
            "get_my_growth_journal",
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn upsert_growth_journal(
        &self,
        access_token: &str,
        generation: u64,
        timezone: &str,
        cycles: &[GrowthJournalCycle],
        entries: &[GrowthJournalEntry],
    ) -> Result<GrowthJournal, SyncError> {
        if entries.iter().any(|entry| {
            entry.generation != generation || entry.payload_hash != entry.compute_hash()
        }) {
            return Err(SyncError::InvalidSnapshot);
        }
        self.post_rpc(
            access_token,
            "upsert_my_growth_journal",
            &serde_json::json!({
                "p_generation": generation,
                "p_timezone": timezone,
                "p_cycles": cycles,
                "p_entries": entries,
            }),
        )
        .await
    }

    pub async fn delete_growth_journal(
        &self,
        access_token: &str,
    ) -> Result<GrowthJournal, SyncError> {
        self.post_rpc(
            access_token,
            "delete_my_growth_journal",
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn world_planets(
        &self,
        access_token: &str,
        world_id: &str,
    ) -> Result<Vec<WorldPlanet>, SyncError> {
        self.post_rpc(
            access_token,
            "get_world_planets",
            &serde_json::json!({ "p_world_id": world_id }),
        )
        .await
    }

    pub async fn join_world_by_member_code(
        &self,
        access_token: &str,
        code: &str,
    ) -> Result<WorldSummary, SyncError> {
        let rows: Vec<WorldSummary> = self
            .post_rpc(
                access_token,
                "join_world_by_member_code",
                &serde_json::json!({ "p_code": code }),
            )
            .await?;
        one_row(rows)
    }

    pub async fn my_member_code(&self, access_token: &str) -> Result<String, SyncError> {
        self.post_rpc(
            access_token,
            "get_my_member_code",
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn rotate_my_member_code(&self, access_token: &str) -> Result<String, SyncError> {
        self.post_rpc(
            access_token,
            "rotate_my_member_code",
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn current_world(&self, access_token: &str) -> Result<Option<WorldShell>, SyncError> {
        let response = self
            .http
            .get(format!(
                "{}/rest/v1/worlds?select=id,name,timezone,owner_id",
                self.base_url
            ))
            .header("apikey", &self.publishable_key)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| SyncError::Transport)?;
        if !response.status().is_success() {
            return Err(SyncError::Rejected(response.status().as_u16()));
        }
        let mut rows: Vec<WorldShell> = response
            .json()
            .await
            .map_err(|_| SyncError::InvalidResponse)?;
        if rows.len() > 1 {
            return Err(SyncError::InvalidResponse);
        }
        Ok(rows.pop())
    }

    pub async fn create_world(
        &self,
        access_token: &str,
        owner_id: &str,
        name: &str,
        timezone: &str,
    ) -> Result<WorldShell, SyncError> {
        let response = self
            .http
            .post(format!("{}/rest/v1/worlds", self.base_url))
            .header("apikey", &self.publishable_key)
            .header("Prefer", "return=minimal")
            .bearer_auth(access_token)
            .json(&serde_json::json!({
                "owner_id": owner_id, "name": name, "timezone": timezone
            }))
            .send()
            .await
            .map_err(|_| SyncError::Transport)?;
        if !response.status().is_success() {
            return Err(SyncError::Rejected(response.status().as_u16()));
        }
        self.current_world(access_token)
            .await?
            .ok_or(SyncError::InvalidResponse)
    }

    pub async fn list_members(
        &self,
        access_token: &str,
        world_id: &str,
    ) -> Result<Vec<WorldMember>, SyncError> {
        self.post_rpc(
            access_token,
            "list_world_members",
            &serde_json::json!({ "p_world_id": world_id }),
        )
        .await
    }

    pub async fn transfer_owner(
        &self,
        access_token: &str,
        world_id: &str,
        new_owner_id: &str,
    ) -> Result<bool, SyncError> {
        self.post_rpc(
            access_token,
            "transfer_world_owner",
            &serde_json::json!({
                "p_world_id": world_id, "p_new_owner_id": new_owner_id
            }),
        )
        .await
    }

    pub async fn leave_world(&self, access_token: &str, world_id: &str) -> Result<bool, SyncError> {
        self.post_rpc(
            access_token,
            "leave_world",
            &serde_json::json!({ "p_world_id": world_id }),
        )
        .await
    }

    pub async fn delete_synced_usage(
        &self,
        access_token: &str,
        world_id: &str,
    ) -> Result<u64, SyncError> {
        self.post_rpc(
            access_token,
            "delete_synced_usage",
            &serde_json::json!({ "p_world_id": world_id }),
        )
        .await
    }

    pub async fn my_sync_policy(
        &self,
        access_token: &str,
        world_id: &str,
    ) -> Result<MySyncPolicy, SyncError> {
        let rows: Vec<MySyncPolicy> = self
            .post_rpc(
                access_token,
                "get_my_sync_policy",
                &serde_json::json!({ "p_world_id": world_id }),
            )
            .await?;
        one_row(rows)
    }

    pub async fn resume_my_sync(
        &self,
        access_token: &str,
        world_id: &str,
    ) -> Result<bool, SyncError> {
        self.post_rpc(
            access_token,
            "resume_my_sync",
            &serde_json::json!({ "p_world_id": world_id }),
        )
        .await
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
    use super::{guest_import_rpc_body, one_row, MySyncPolicy, SyncError, UploadBody, WorldSummary};
    use crate::domain::cosmetic_shop::{GuestCosmeticImport, GuestCosmeticPurchase};
    use crate::domain::planet::PlanetWalletCredit;
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

    #[test]
    fn world_planet_accepts_equipped_keys_and_rejects_private_wallet_fields() {
        let value = serde_json::json!({
            "nickname": "Nova", "avatar": "feminine", "stage": 0,
            "current_planet_tokens": 0, "lifetime_tokens": 100000,
            "growth_credit": 1.0, "progress_to_next": 0.2, "incomplete": false,
            "objects": [], "equipped_cosmetics": [{"slot_id":"sky", "sku":"star_cluster"}],
            "token_rank": 1, "civilization_rank": 1
        });
        let planet: crate::domain::planet::WorldPlanet = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(planet.equipped_cosmetics[0].slot_id, "sky");
        assert_eq!(planet.equipped_cosmetics[0].sku, "star_cluster");
        assert!(value.get("wallet_balance").is_none());

        let mut contaminated = value;
        contaminated["wallet_balance"] = serde_json::json!(100000);
        assert!(serde_json::from_value::<crate::domain::planet::WorldPlanet>(contaminated).is_err());
    }

    #[test]
    fn deletion_policy_response_contains_only_own_cutoff_and_pause() {
        let value = serde_json::json!({"deleted_through":"2026-09-25","paused":true});
        let policy: MySyncPolicy = serde_json::from_value(value).unwrap();
        assert_eq!(policy.deleted_through.as_deref(), Some("2026-09-25"));
        assert!(policy.paused);
        assert!(serde_json::from_value::<MySyncPolicy>(serde_json::json!({
            "deleted_through": null, "paused": false, "member_totals": [42]
        }))
        .is_err());
    }

    #[test]
    fn guest_import_rpc_body_keeps_the_same_import_id_and_only_guest_rows() {
        let import = GuestCosmeticImport {
            import_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
            wallet_credits: vec![PlanetWalletCredit {
                previous_cycle_id: "guest-cycle".into(),
                amount: 100000,
                created_at_utc: "2026-09-28T00:00:00Z".into(),
            }],
            purchases: vec![GuestCosmeticPurchase {
                purchase_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
                sku: "star_cluster".into(),
                price: 100000,
                purchased_at_utc: "2026-09-28T00:00:00Z".into(),
            }],
        };
        let body = guest_import_rpc_body(&import);
        assert_eq!(body.as_object().unwrap().len(), 3);
        assert_eq!(body["p_import_id"], import.import_id);
        assert_eq!(body["p_wallet_credits"][0]["previous_cycle_id"], "guest-cycle");
        assert_eq!(body["p_purchases"][0]["purchase_id"], import.purchases[0].purchase_id);
        assert!(body.get("user_id").is_none());
    }
}
