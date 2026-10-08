use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{sync::OnceLock, time::Duration};

use crate::domain::cosmetic_shop::{
    CosmeticEquipResult, CosmeticPurchaseResult, CosmeticShopState, GuestCosmeticImport,
    GuestCosmeticImportResult, QuoteTarget, ResetShopResult, ShopActionResult, ShopActionStatus,
    ShopEffectTimeline, ShopQuote, ShopRequest, ShopState,
};
use crate::domain::growth_journal::{GrowthJournal, GrowthJournalCycle, GrowthJournalEntry};
use crate::domain::guest_shop_import::{
    GuestImportStatus, GuestShopImportV2Request, GuestShopImportV2Result,
};
use crate::domain::planet::{
    PlanetDeviceContribution, PlanetDeviceContributionSnapshot, PlanetState, WorldPlanet,
};
use crate::sync::aggregate::DailyUsageSnapshot;
use sha2::{Digest, Sha256};

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

#[cfg(test)]
pub(crate) fn task9_local_api_origin_port(base_url: &str) -> Option<u16> {
    let url = reqwest::Url::parse(base_url).ok()?;
    let port = url.port()?;
    (url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && (49152..=65535).contains(&port)
        && base_url == format!("http://127.0.0.1:{port}"))
    .then_some(port)
}

#[cfg(test)]
pub(crate) fn task9_local_api_url_matches_owned_port(base_url: &str, owned_port: u16) -> bool {
    task9_local_api_origin_port(base_url) == Some(owned_port)
}

#[cfg(test)]
fn task9_local_api_http_client_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
}

#[cfg(test)]
pub(crate) fn task9_local_api_http_client() -> Client {
    task9_local_api_http_client_builder()
        .build()
        .expect("Task 9 local API HTTP client configuration is valid")
}

#[derive(Debug, Eq, PartialEq)]
pub enum SyncError {
    InvalidSnapshot,
    Transport,
    Rejected(u16),
    InvalidResponse,
}

#[cfg(test)]
fn task9_postgrest_error_code(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let code = value.get("code")?.as_str()?;
    if code.is_empty()
        || code.len() > 12
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return None;
    }
    Some(code.to_owned())
}

#[derive(Serialize)]
struct UploadBody<'a> {
    p_world_id: &'a str,
    p_snapshot: &'a DailyUsageSnapshot,
}

fn purchase_cosmetic_rpc_body(
    purchase_id: &str,
    sku: &str,
    catalog_revision: u32,
) -> serde_json::Value {
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

fn reset_my_planet_rpc_body(request_id: uuid::Uuid, expected_cycle_id: &str) -> serde_json::Value {
    serde_json::json!({
        "p_request_id": request_id.to_string(),
        "p_cycle_id": expected_cycle_id,
    })
}

fn get_my_shop_effect_timeline_rpc_body() -> serde_json::Value {
    serde_json::json!({})
}

fn upload_planet_state_with_effects_rpc_body(
    state: &PlanetState,
    contribution: &PlanetDeviceContributionSnapshot,
) -> serde_json::Value {
    let mut state = state.clone();
    // The server owns wallet credits. Send the legacy fields required by the
    // existing RPC with empty values so this upload makes no wallet claim.
    state.wallet_balance = 0;
    state.wallet_credits.clear();
    serde_json::json!({
        "p_state": state,
        "p_device_contribution": contribution,
    })
}

fn validate_reset_shop_result(
    result: ResetShopResult,
    request_id: &str,
    expected_cycle_id: &str,
) -> Result<ResetShopResult, SyncError> {
    if result.action.request_id != request_id
        || result.action.state.current_cycle_id != result.planet_state.current_cycle_id
        || (result.action.status == ShopActionStatus::Reset
            && result.planet_state.current_cycle_id == expected_cycle_id)
    {
        return Err(SyncError::InvalidResponse);
    }
    Ok(result)
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
    #[cfg(test)]
    task9_diagnostic: bool,
}

// The issuance DTO deliberately has no Debug implementation: it owns a one-time secret.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreatedWorldInvite {
    pub invite_id: String,
    pub code: String,
    pub created_at: String,
    pub expires_at: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteStatus { Active, Used, Revoked, Expired }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldInvite {
    pub invite_id: String,
    pub created_at: String,
    pub expires_at: String,
    pub revoked_at: Option<String>,
    pub used_at: Option<String>,
    pub status: InviteStatus,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteRevokeStatus { Revoked, AlreadyRevoked, Used }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InviteRevokeResult { pub status: InviteRevokeStatus }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteAcceptStatus { Accepted, AlreadyAccepted, Unavailable, AlreadyMember, WorldFull, RateLimited }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InviteAcceptResult { pub status: InviteAcceptStatus, pub world_id: Option<String> }

impl SupabaseSyncClient {
    pub fn new(base_url: &str, publishable_key: &str) -> Self {
        Self {
            http: shared_http_client(),
            base_url: base_url.trim_end_matches('/').to_owned(),
            publishable_key: publishable_key.to_owned(),
            #[cfg(test)]
            task9_diagnostic: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_task9_local_api_e2e(
        base_url: &str,
        publishable_key: &str,
        owned_port: u16,
    ) -> Self {
        let normalized_url = base_url.strip_suffix('/').unwrap_or(base_url);
        assert!(
            task9_local_api_url_matches_owned_port(normalized_url, owned_port),
            "Task 9 E2E client requires the exact owned loopback origin"
        );
        Self {
            http: task9_local_api_http_client(),
            base_url: normalized_url.to_owned(),
            publishable_key: publishable_key.to_owned(),
            task9_diagnostic: true,
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

    pub async fn import_guest_shop(
        &self,
        access_token: &str,
        request: &GuestShopImportV2Request,
    ) -> Result<GuestShopImportV2Result, SyncError> {
        let response = self
            .http
            .post(format!("{}/rest/v1/rpc/import_guest_shop", self.base_url))
            .header("apikey", &self.publishable_key)
            .bearer_auth(access_token)
            .json(&serde_json::json!({
                "p_import_id": request.snapshot.import_id,
                "p_request": request,
            }))
            .send()
            .await
            .map_err(|_| SyncError::Transport)?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            #[cfg(test)]
            if self.task9_diagnostic {
                let body = response.text().await.unwrap_or_default();
                let code =
                    task9_postgrest_error_code(&body).unwrap_or_else(|| "UNKNOWN".to_owned());
                let diagnostic = format!("Task9 RPC diagnostic HTTP={status} code={code}");
                eprintln!("{diagnostic}");
                panic!("{diagnostic}");
            }
            return Err(SyncError::Rejected(status));
        }
        let response_body = response
            .text()
            .await
            .map_err(|_| SyncError::InvalidResponse)?;
        let result =
            crate::domain::guest_shop_import::parse_guest_shop_import_v2_result(&response_body)
                .map_err(|_| SyncError::InvalidResponse)?;
        validate_guest_shop_import_v2_result(request, &result)?;
        Ok(result)
    }

    pub async fn my_planet_state(
        &self,
        access_token: &str,
    ) -> Result<Option<PlanetState>, SyncError> {
        self.post_rpc(access_token, "get_my_planet_state", &serde_json::json!({}))
            .await
    }

    pub async fn get_my_shop_state(&self, access_token: &str) -> Result<ShopState, SyncError> {
        self.post_rpc(access_token, "get_my_shop_state", &serde_json::json!({}))
            .await
    }

    pub async fn quote_shop_action(
        &self,
        access_token: &str,
        target: &QuoteTarget,
    ) -> Result<ShopQuote, SyncError> {
        self.post_rpc(
            access_token,
            "quote_shop_action",
            &serde_json::json!({ "p_target": target }),
        )
        .await
    }

    pub async fn apply_shop_action(
        &self,
        access_token: &str,
        request: &ShopRequest,
    ) -> Result<ShopActionResult, SyncError> {
        self.post_rpc(
            access_token,
            "apply_shop_action",
            &serde_json::json!({ "p_request": request }),
        )
        .await
    }

    pub async fn get_my_shop_effect_timeline(
        &self,
        access_token: &str,
    ) -> Result<ShopEffectTimeline, SyncError> {
        self.post_rpc(
            access_token,
            "get_my_shop_effect_timeline",
            &get_my_shop_effect_timeline_rpc_body(),
        )
        .await
    }

    pub async fn reset_my_planet(
        &self,
        access_token: &str,
        request_id: uuid::Uuid,
        expected_cycle_id: &str,
    ) -> Result<ResetShopResult, SyncError> {
        let request_id_text = request_id.to_string();
        let result = self
            .post_rpc(
                access_token,
                "reset_my_planet",
                &reset_my_planet_rpc_body(request_id, expected_cycle_id),
            )
            .await?;
        validate_reset_shop_result(result, &request_id_text, expected_cycle_id)
    }

    pub async fn cosmetic_shop_state(
        &self,
        access_token: &str,
    ) -> Result<CosmeticShopState, SyncError> {
        self.post_rpc(
            access_token,
            "get_my_cosmetic_state",
            &serde_json::json!({}),
        )
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

    pub async fn upload_planet_state_with_effects(
        &self,
        access_token: &str,
        state: &PlanetState,
        contribution: &PlanetDeviceContributionSnapshot,
    ) -> Result<PlanetState, SyncError> {
        self.post_rpc(
            access_token,
            "upsert_my_planet_state",
            &upload_planet_state_with_effects_rpc_body(state, contribution),
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

    pub async fn accept_world_invite(&self, access_token: &str, code: &str) -> Result<InviteAcceptResult, SyncError> {
        let rows: Vec<InviteAcceptResult> = self.post_rpc(access_token,"accept_world_invite",&serde_json::json!({"p_code":code})).await?;
        let result = one_row(rows)?;
        let success = matches!(result.status, InviteAcceptStatus::Accepted | InviteAcceptStatus::AlreadyAccepted);
        if success != result.world_id.is_some() || result.world_id.as_ref().is_some_and(|id| uuid::Uuid::parse_str(id).is_err()) {
            return Err(SyncError::InvalidResponse);
        }
        Ok(result)
    }

    pub async fn create_world_invite(&self, access_token: &str, world_id: &str) -> Result<CreatedWorldInvite, SyncError> {
        let rows: Vec<CreatedWorldInvite> = self.post_rpc(access_token,"create_world_invite",&serde_json::json!({"p_world_id":world_id})).await?;
        let result = one_row(rows)?;
        let created = chrono::DateTime::parse_from_rfc3339(&result.created_at).map_err(|_| SyncError::InvalidResponse)?;
        let expires = chrono::DateTime::parse_from_rfc3339(&result.expires_at).map_err(|_| SyncError::InvalidResponse)?;
        if uuid::Uuid::parse_str(&result.invite_id).is_err() || result.code.len()!=64
            || !result.code.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || expires-created!=chrono::Duration::hours(168) { return Err(SyncError::InvalidResponse); }
        Ok(result)
    }

    pub async fn list_world_invites(&self, access_token: &str, world_id: &str) -> Result<Vec<WorldInvite>, SyncError> {
        self.post_rpc(access_token,"list_world_invites",&serde_json::json!({"p_world_id":world_id})).await
    }

    pub async fn revoke_world_invite(&self, access_token: &str, invite_id: &str) -> Result<InviteRevokeResult, SyncError> {
        let rows: Vec<InviteRevokeResult> = self.post_rpc(access_token,"revoke_world_invite",&serde_json::json!({"p_invite_id":invite_id})).await?;
        one_row(rows)
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

fn same_optional_utc_timestamp(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            let Ok(left) = chrono::DateTime::parse_from_rfc3339(left) else {
                return false;
            };
            let Ok(right) = chrono::DateTime::parse_from_rfc3339(right) else {
                return false;
            };
            left.with_timezone(&chrono::Utc) == right.with_timezone(&chrono::Utc)
        }
        _ => false,
    }
}

fn journal_metadata_matches_capture(
    snapshot: &crate::domain::guest_shop_import::GuestShopImportV2Snapshot,
    journal: &crate::domain::growth_journal::GrowthJournal,
) -> bool {
    let captured_state = snapshot.data.growth_journal_state.as_ref();
    let expected_generation = captured_state.map_or(0, |state| state.generation);
    let expected_deleted_at = captured_state.and_then(|state| state.deleted_at_utc.as_deref());
    let state_matches = journal.generation == expected_generation
        && same_optional_utc_timestamp(journal.deleted_at_utc.as_deref(), expected_deleted_at);

    let cycles_match = journal.cycles.len() == snapshot.data.growth_journal_cycles.len()
        && snapshot.data.growth_journal_cycles.iter().all(|captured| {
            journal
                .cycles
                .iter()
                .filter(|returned| {
                    returned.cycle_id == captured.cycle_id
                        && same_optional_utc_timestamp(
                            returned.started_at_utc.as_deref(),
                            captured.started_at_utc.as_deref(),
                        )
                        && same_optional_utc_timestamp(
                            returned.ended_at_utc.as_deref(),
                            captured.ended_at_utc.as_deref(),
                        )
                        && returned.wallet_credit == captured.wallet_credit
                        && same_optional_utc_timestamp(
                            returned.wallet_credit_at_utc.as_deref(),
                            captured.wallet_credit_at_utc.as_deref(),
                        )
                })
                .count()
                == 1
        });

    state_matches && cycles_match
}

pub(crate) fn validate_guest_shop_import_v2_result(
    request: &GuestShopImportV2Request,
    result: &GuestShopImportV2Result,
) -> Result<(), SyncError> {
    let snapshot = &request.snapshot;
    if result.schema_version != 2
        || result.import_id != snapshot.import_id
        || result.account_id != snapshot.target_account_id
        || result.source_fingerprint != snapshot.source_fingerprint
        || result.validate_wire_shape().is_err()
    {
        return Err(SyncError::InvalidResponse);
    }

    if result.status != GuestImportStatus::Imported {
        return Ok(());
    }

    let shop = result
        .shop_state
        .as_ref()
        .ok_or(SyncError::InvalidResponse)?;
    let planet = result
        .planet_state
        .as_ref()
        .ok_or(SyncError::InvalidResponse)?;
    let timeline = result
        .effect_timeline
        .as_ref()
        .ok_or(SyncError::InvalidResponse)?;
    let canonical = result
        .canonical_contribution
        .as_ref()
        .ok_or(SyncError::InvalidResponse)?;
    let journal = result
        .journal_confirmation
        .as_ref()
        .ok_or(SyncError::InvalidResponse)?;
    let ack = result.ack.as_ref().ok_or(SyncError::InvalidResponse)?;
    let expected_cycle = &snapshot.provenance.reset_receipt.result.new_cycle_id;

    let canonical_bytes = crate::domain::guest_shop_import::canonical_json_bytes(canonical)
        .map_err(|_| SyncError::InvalidResponse)?;
    let canonical_fingerprint = Sha256::digest(canonical_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let ack_matches_captured_journal = ack.journal_entries.len()
        == snapshot.data.growth_journal_entries.len()
        && snapshot.data.growth_journal_entries.iter().all(|captured| {
            let captured_agent = match captured.agent.as_str() {
                "codex" => crate::domain::usage::Agent::Codex,
                "claude_code" => crate::domain::usage::Agent::ClaudeCode,
                _ => return false,
            };
            ack.journal_entries
                .iter()
                .filter(|ack_entry| {
                    let key = &ack_entry.logical_key;
                    key.device_id == captured.device_id
                        && key.cycle_id == captured.cycle_id
                        && key.bucket_date == captured.bucket_date
                        && key.agent == captured_agent
                        && key.generation == captured.generation
                        && ack_entry.revision == captured.revision
                        && ack_entry.payload_hash == captured.payload_hash
                })
                .count()
                == 1
        });
    let ack_matches_journal_confirmation = ack.journal_entries.iter().all(|ack_entry| {
        journal
            .entries
            .iter()
            .filter(|entry| {
                entry.device_id == ack_entry.logical_key.device_id
                    && entry.cycle_id == ack_entry.logical_key.cycle_id
                    && entry.bucket_date == ack_entry.logical_key.bucket_date
                    && entry.agent == ack_entry.logical_key.agent
                    && entry.generation == ack_entry.logical_key.generation
                    && entry.revision == ack_entry.revision
                    && entry.payload_hash == ack_entry.payload_hash
            })
            .count()
            == 1
    });

    if shop.account_id != snapshot.target_account_id
        || shop.current_cycle_id != *expected_cycle
        || planet.current_cycle_id != *expected_cycle
        || Some(timeline.account_id.as_str()) != snapshot.target_account_id.strip_prefix("account:")
        || timeline.current_cycle_id != *expected_cycle
        || canonical.raw.device_id != snapshot.provenance.device_id
        || canonical.raw.current_cycle_id != *expected_cycle
        || canonical != &snapshot.canonical_payload
        || canonical.canonical_version != ack.canonical_version
        || ack.lineage_id != snapshot.provenance.lineage_id
        || ack.device_id != snapshot.provenance.device_id
        || ack.ingest_watermark != snapshot.provenance.ingest_watermark
        || ack.occurrence_count != snapshot.provenance.occurrence_count
        || ack.prefix_fingerprint != snapshot.provenance.prefix_fingerprint
        || ack.canonical_payload_fingerprint != canonical_fingerprint
        || !ack_matches_captured_journal
        || !ack_matches_journal_confirmation
        || !journal_metadata_matches_capture(snapshot, journal)
    {
        return Err(SyncError::InvalidResponse);
    }

    Ok(())
}

fn one_row<T>(mut rows: Vec<T>) -> Result<T, SyncError> {
    if rows.len() != 1 {
        return Err(SyncError::InvalidResponse);
    }
    Ok(rows.remove(0))
}

#[cfg(test)]
mod tests {
    use super::{
        get_my_shop_effect_timeline_rpc_body, guest_import_rpc_body, one_row,
        reset_my_planet_rpc_body, upload_planet_state_with_effects_rpc_body,
        validate_reset_shop_result, MySyncPolicy, SupabaseSyncClient, SyncError, UploadBody,
        WorldSummary,
    };
    use crate::domain::cosmetic_shop::{
        ActiveEffects, AvatarEquipment, AvatarEquipmentItem, GuestCosmeticImport,
        GuestCosmeticPurchase, QuoteTarget, ResetShopResult, RewardState, ShopActionResult,
        ShopActionStatus, ShopEffectTimeline, ShopQuote, ShopRequest, ShopState,
    };
    use crate::domain::guest_shop_import::{GuestImportStatus, GuestShopImportV2Request};
    use crate::domain::planet::{
        PlanetActivityDayContribution, PlanetAvatar, PlanetDeviceContribution,
        PlanetDeviceContributionSnapshot, PlanetEffectContributionSegment, PlanetProfile,
        PlanetState, PlanetWalletCredit,
    };
    use crate::domain::usage::{Agent, UsageCoverage};
    use crate::sync::aggregate::DailyUsageSnapshot;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread::{self, JoinHandle};

    #[test]
    fn invite_accept_decodes_status_and_uses_invite_endpoint() {
        let (url, server) = spawn_rpc_server(MockResponse::Json(200,
            r#"[{"status":"accepted","world_id":"81000000-0000-0000-0000-000000000001"}]"#.into()));
        let client = SupabaseSyncClient::new(&url, "publishable-key");
        let result = run_async(client.accept_world_invite("account-token", &"a".repeat(64)));
        let request = server.join().unwrap();
        assert!(result.is_ok(), "invite status response must decode");
        assert_account_request(&request, "/rest/v1/rpc/accept_world_invite");
        assert_eq!(request.body, serde_json::json!({"p_code": "a".repeat(64)}));
    }

    #[test]
    fn invite_rpc_management_contracts_and_failures() {
        let cases = [
            ("create", r#"[{"invite_id":"81000000-0000-0000-0000-000000000002","code":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","created_at":"2026-10-08T00:00:00Z","expires_at":"2026-10-15T00:00:00Z"}]"#),
            ("list", r#"[{"invite_id":"81000000-0000-0000-0000-000000000002","created_at":"2026-10-08T00:00:00Z","expires_at":"2026-10-15T00:00:00Z","used_at":null,"revoked_at":null,"status":"active"}]"#),
            ("revoke", r#"[{"status":"revoked"}]"#),
        ];
        for (name, body) in cases {
            let (url, server) = spawn_rpc_server(MockResponse::Json(200, body.into()));
            let client = SupabaseSyncClient::new(&url, "publishable-key");
            match name {
                "create" => assert!(run_async(client.create_world_invite("account-token", "world-id")).is_ok()),
                "list" => assert_eq!(run_async(client.list_world_invites("account-token", "world-id")).unwrap().len(), 1),
                _ => assert!(run_async(client.revoke_world_invite("account-token", "invite-id")).is_ok()),
            }
            let request = server.join().unwrap();
            assert_account_request(&request, &format!("/rest/v1/rpc/{name}_world_invite{}", if name=="list" {"s"} else {""}));
            assert_eq!(request.body, if name=="revoke" {serde_json::json!({"p_invite_id":"invite-id"})} else {serde_json::json!({"p_world_id":"world-id"})});
        }
        for body in ["[]", "null", r#"[{"status":"accepted","world_id":null}]"#,
            r#"[{"status":"unavailable","world_id":"81000000-0000-0000-0000-000000000001"}]"#,
            r#"[{"status":"future","world_id":null}]"#] {
            let (url, server) = spawn_rpc_server(MockResponse::Json(200, body.into()));
            let client = SupabaseSyncClient::new(&url,"publishable-key");
            assert!(matches!(run_async(client.accept_world_invite("account-token","hidden")),Err(SyncError::InvalidResponse)));
            assert_account_request(&server.join().unwrap(),"/rest/v1/rpc/accept_world_invite");
        }
        for response in [MockResponse::Json(401,"sensitive body".into()),MockResponse::Disconnect] {
            let (url, server) = spawn_rpc_server(response);
            let client = SupabaseSyncClient::new(&url,"publishable-key");
            assert!(run_async(client.accept_world_invite("account-token","hidden")).is_err());
            assert_account_request(&server.join().unwrap(),"/rest/v1/rpc/accept_world_invite");
        }
    }

    #[test]
    fn task9_postgrest_error_code_accepts_only_bounded_uppercase_alphanumeric_code() {
        assert_eq!(
            super::task9_postgrest_error_code(
                r#"{"code":"42501","message":"sensitive server detail"}"#
            )
            .as_deref(),
            Some("42501")
        );
        assert_eq!(
            super::task9_postgrest_error_code(r#"{"code":"P0001"}"#).as_deref(),
            Some("P0001")
        );
        assert_eq!(
            super::task9_postgrest_error_code(r#"{"code":"ABCDEFGHIJKL"}"#).as_deref(),
            Some("ABCDEFGHIJKL")
        );

        for body in [
            r#"{"code":"p0001"}"#,
            r#"{"code":"42501;DROP"}"#,
            r#"{"code":"ABCDEFGHIJKLM"}"#,
            r#"{"code":403}"#,
            r#"{"message":"42501"}"#,
            "not-json",
        ] {
            assert_eq!(super::task9_postgrest_error_code(body), None);
        }
    }

    #[derive(Debug)]
    struct CapturedRequest {
        method: String,
        path: String,
        headers: std::collections::HashMap<String, String>,
        body: serde_json::Value,
    }

    enum MockResponse {
        Json(u16, String),
        Disconnect,
    }

    fn spawn_rpc_server(response: MockResponse) -> (String, JoinHandle<CapturedRequest>) {
        let (url, server) = spawn_rpc_sequence(vec![response]);
        let thread = thread::spawn(move || server.join().unwrap().remove(0));
        (url, thread)
    }

    fn spawn_rpc_sequence(
        responses: Vec<MockResponse>,
    ) -> (String, JoinHandle<Vec<CapturedRequest>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = thread::spawn(move || {
            responses
                .into_iter()
                .map(|response| {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .unwrap();
                    let captured = capture_request(&mut stream);
                    respond(&mut stream, response);
                    captured
                })
                .collect()
        });
        (format!("http://{address}"), thread)
    }

    fn respond(stream: &mut TcpStream, response: MockResponse) {
        if let MockResponse::Json(status, body) = response {
            let reason = if status == 200 { "OK" } else { "Rejected" };
            write!(stream, "HTTP/1.1 {status} {reason}\r\n").unwrap();
            write!(stream, "Content-Type: application/json\r\n").unwrap();
            write!(stream, "Content-Length: {}\r\n", body.len()).unwrap();
            write!(stream, "Connection: close\r\n\r\n{body}").unwrap();
            stream.flush().unwrap();
        }
    }

    fn capture_request(stream: &mut TcpStream) -> CapturedRequest {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            assert_ne!(count, 0, "client closed before sending its request");
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let header_text = std::str::from_utf8(&bytes[..header_end]).unwrap();
                let content_length = header_text
                    .lines()
                    .skip(1)
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if bytes.len() >= header_end + 4 + content_length {
                    let mut lines = header_text.lines();
                    let mut request_line = lines.next().unwrap().split_whitespace();
                    let method = request_line.next().unwrap().to_owned();
                    let path = request_line.next().unwrap().to_owned();
                    let headers = lines
                        .filter_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            Some((name.to_ascii_lowercase(), value.trim().to_owned()))
                        })
                        .collect();
                    let body_start = header_end + 4;
                    let body_bytes = &bytes[body_start..body_start + content_length];
                    let body = if body_bytes.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::from_slice(body_bytes).unwrap()
                    };
                    return CapturedRequest {
                        method,
                        path,
                        headers,
                        body,
                    };
                }
            }
        }
    }

    fn run_async<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn guest_import_v2_transport_request() -> GuestShopImportV2Request {
        serde_json::from_str(include_str!(
            "../../../../../supabase/tests/fixtures/shop_guest_import_v2_first_reset.json"
        ))
        .unwrap()
    }

    fn guest_import_v2_held_response(
        request: &GuestShopImportV2Request,
        status: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 2,
            "import_id": request.snapshot.import_id,
            "account_id": request.snapshot.target_account_id,
            "source_fingerprint": request.snapshot.source_fingerprint,
            "status": status,
        })
    }

    fn run_guest_import_v2_transport(
        request: &GuestShopImportV2Request,
        response: MockResponse,
    ) -> (
        Result<crate::domain::guest_shop_import::GuestShopImportV2Result, SyncError>,
        CapturedRequest,
    ) {
        let (url, server) = spawn_rpc_server(response);
        let client = SupabaseSyncClient::new(&url, "publishable-key");
        let result = run_async(client.import_guest_shop("access-token", request));
        (result, server.join().unwrap())
    }

    #[test]
    fn guest_import_v2_transport_posts_exact_immutable_request_with_bearer_auth() {
        let request = guest_import_v2_transport_request();
        let original = request.clone();
        let held = guest_import_v2_held_response(&request, "source_unverifiable").to_string();
        let (url, server) = spawn_rpc_sequence(vec![
            MockResponse::Json(200, held.clone()),
            MockResponse::Json(200, held),
        ]);
        let client = SupabaseSyncClient::new(&url, "publishable-key");

        for _ in 0..2 {
            let result = run_async(client.import_guest_shop("access-token", &request)).unwrap();
            assert_eq!(result.status, GuestImportStatus::SourceUnverifiable);
        }

        let captured = server.join().unwrap();
        assert_eq!(captured.len(), 2);
        for call in captured {
            assert_eq!(call.method, "POST");
            assert_eq!(call.path, "/rest/v1/rpc/import_guest_shop");
            assert_eq!(call.headers.get("apikey").unwrap(), "publishable-key");
            assert_eq!(
                call.headers.get("authorization").unwrap(),
                "Bearer access-token"
            );
            assert_eq!(
                call.body,
                serde_json::json!({
                    "p_import_id": original.snapshot.import_id,
                    "p_request": original,
                })
            );
        }
        assert_eq!(request, original);
    }

    #[test]
    fn guest_import_v2_transport_rejects_old_backend_auth_and_truncated_responses() {
        let request = guest_import_v2_transport_request();
        let old_backend = serde_json::json!({
            "import_id": request.snapshot.import_id,
            "status": "source_unverifiable",
        });
        let (result, _) = run_guest_import_v2_transport(
            &request,
            MockResponse::Json(200, old_backend.to_string()),
        );
        assert!(matches!(result, Err(SyncError::InvalidResponse)));

        let (result, _) =
            run_guest_import_v2_transport(&request, MockResponse::Json(401, "unauthorized".into()));
        assert!(matches!(result, Err(SyncError::Rejected(401))));

        let (result, _) = run_guest_import_v2_transport(
            &request,
            MockResponse::Json(200, "{\"schema_version\":".into()),
        );
        assert!(matches!(result, Err(SyncError::InvalidResponse)));
    }

    #[test]
    fn guest_import_v2_transport_rejects_wrong_typed_correlation_and_result_shape() {
        let request = guest_import_v2_transport_request();
        let fields = [
            ("schema_version", serde_json::json!(1)),
            (
                "import_id",
                serde_json::json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
            ),
            (
                "account_id",
                serde_json::json!("account:00000000-0000-4000-a000-000000000099"),
            ),
            ("source_fingerprint", serde_json::json!("0".repeat(64))),
        ];
        for (field, value) in fields {
            let mut response = guest_import_v2_held_response(&request, "source_unverifiable");
            response[field] = value;
            let (result, _) = run_guest_import_v2_transport(
                &request,
                MockResponse::Json(200, response.to_string()),
            );
            assert!(
                matches!(result, Err(SyncError::InvalidResponse)),
                "field {field}"
            );
        }

        let mut missing_ack = guest_import_v2_held_response(&request, "imported");
        let (result, _) = run_guest_import_v2_transport(
            &request,
            MockResponse::Json(200, missing_ack.to_string()),
        );
        assert!(matches!(result, Err(SyncError::InvalidResponse)));

        missing_ack = guest_import_v2_held_response(&request, "active_account");
        missing_ack["shop_state"] = serde_json::to_value(empty_shop_state()).unwrap();
        let (result, _) = run_guest_import_v2_transport(
            &request,
            MockResponse::Json(200, missing_ack.to_string()),
        );
        assert!(matches!(result, Err(SyncError::InvalidResponse)));
    }

    fn valid_imported_v2_transport_response(
        request: &GuestShopImportV2Request,
    ) -> serde_json::Value {
        serde_json::to_value(
            crate::storage::guest_shop_import_v2::guest_import_v2_capture_tests::
                imported_result_for(request),
        )
        .unwrap()
    }

    #[test]
    fn guest_import_v2_transport_accepts_valid_imported_result() {
        let request = guest_import_v2_transport_request();
        let response = valid_imported_v2_transport_response(&request);
        let (result, captured) =
            run_guest_import_v2_transport(&request, MockResponse::Json(200, response.to_string()));

        let result = result.expect("a correlated imported result should pass transport validation");
        assert_eq!(result.status, GuestImportStatus::Imported);
        assert_eq!(result.import_id, request.snapshot.import_id);
        assert_eq!(result.account_id, request.snapshot.target_account_id);
        assert_eq!(captured.method, "POST");
        assert_eq!(captured.path, "/rest/v1/rpc/import_guest_shop");
        assert_eq!(captured.body["p_import_id"], request.snapshot.import_id);
        assert_eq!(
            captured.body["p_request"],
            serde_json::to_value(request).unwrap()
        );
    }

    #[test]
    fn guest_import_v2_transport_rejects_duplicate_nested_canonical_version() {
        let request = guest_import_v2_transport_request();
        let valid_response = valid_imported_v2_transport_response(&request);
        let mut raw_response = valid_response.to_string();
        let canonical_object_start = raw_response
            .find("\"canonical_contribution\":{")
            .expect("canonical contribution object is serialized");
        let canonical_version_start = canonical_object_start
            + raw_response[canonical_object_start..]
                .find("\"canonical_version\":")
                .expect("canonical version is serialized");
        let value_start = canonical_version_start
            + raw_response[canonical_version_start..]
                .find(':')
                .expect("canonical version has a value")
            + 1;
        let value_end = value_start
            + raw_response[value_start..]
                .find(|character| character == ',' || character == '}')
                .expect("canonical version value ends before the next field");
        let valid_version = raw_response[value_start..value_end].to_owned();
        assert_ne!(valid_version, "0");
        raw_response.replace_range(
            value_start..value_end,
            &format!("0,\"canonical_version\":{valid_version}"),
        );

        let (result, _) =
            run_guest_import_v2_transport(&request, MockResponse::Json(200, raw_response));

        assert!(matches!(result, Err(SyncError::InvalidResponse)));
    }

    #[test]
    fn guest_import_v2_transport_rejects_imported_cycle_version_ack_and_correlation_mismatches() {
        let request = guest_import_v2_transport_request();
        let valid_response = valid_imported_v2_transport_response(&request);
        let mut wrong_cycle = valid_response.clone();
        wrong_cycle["effect_timeline"]["current_cycle_id"] =
            serde_json::json!("00000000-0000-4000-a000-000000000099");
        let mut wrong_version = valid_response.clone();
        wrong_version["schema_version"] = serde_json::json!(1);
        let mut wrong_correlation = valid_response.clone();
        wrong_correlation["account_id"] =
            serde_json::json!("account:00000000-0000-4000-a000-000000000099");
        let captured_journal_state = request.snapshot.data.growth_journal_state.as_ref();
        let mut wrong_journal_generation = valid_response.clone();
        wrong_journal_generation["journal_confirmation"]["generation"] =
            serde_json::json!(captured_journal_state.map_or(0, |state| state.generation) + 1);
        let mut unexpected_journal_deletion = valid_response.clone();
        unexpected_journal_deletion["journal_confirmation"]["deleted_at_utc"] =
            serde_json::json!("2026-10-03T05:26:26.000000Z");
        let expected_new_cycle = &request
            .snapshot
            .provenance
            .reset_receipt
            .result
            .new_cycle_id;
        let journal_cycles = valid_response["journal_confirmation"]["cycles"]
            .as_array()
            .unwrap();
        let old_cycle_index = journal_cycles
            .iter()
            .position(|cycle| cycle["cycle_id"] != *expected_new_cycle)
            .expect("first-reset fixture contains a prior cycle");
        let new_cycle_index = journal_cycles
            .iter()
            .position(|cycle| cycle["cycle_id"] == *expected_new_cycle)
            .expect("first-reset fixture contains a new cycle");
        let mut missing_old_cycle = valid_response.clone();
        missing_old_cycle["journal_confirmation"]["cycles"]
            .as_array_mut()
            .unwrap()
            .remove(old_cycle_index);
        let mut missing_new_cycle = valid_response.clone();
        missing_new_cycle["journal_confirmation"]["cycles"]
            .as_array_mut()
            .unwrap()
            .remove(new_cycle_index);
        let mut extra_cycle = valid_response.clone();
        let mut extra_cycle_metadata = journal_cycles[old_cycle_index].clone();
        extra_cycle_metadata["cycle_id"] =
            serde_json::json!("00000000-0000-4000-a000-000000000099");
        extra_cycle["journal_confirmation"]["cycles"]
            .as_array_mut()
            .unwrap()
            .push(extra_cycle_metadata);
        let mut changed_old_cycle = valid_response.clone();
        changed_old_cycle["journal_confirmation"]["cycles"][old_cycle_index]["wallet_credit"] =
            serde_json::json!(1);
        let mut changed_new_cycle = valid_response.clone();
        let current_start = chrono::DateTime::parse_from_rfc3339(
            journal_cycles[new_cycle_index]["started_at_utc"]
                .as_str()
                .expect("new cycle has a start timestamp"),
        )
        .unwrap()
            + chrono::Duration::seconds(1);
        changed_new_cycle["journal_confirmation"]["cycles"][new_cycle_index]["started_at_utc"] =
            serde_json::json!(current_start.to_rfc3339_opts(chrono::SecondsFormat::Micros, true));
        let mut equivalent_utc_journal_timestamps = valid_response.clone();
        for cycle in equivalent_utc_journal_timestamps["journal_confirmation"]["cycles"]
            .as_array_mut()
            .unwrap()
        {
            for key in ["started_at_utc", "ended_at_utc", "wallet_credit_at_utc"] {
                if let Some(timestamp) = cycle[key].as_str() {
                    let utc = chrono::DateTime::parse_from_rfc3339(timestamp)
                        .unwrap()
                        .with_timezone(&chrono::Utc)
                        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
                    cycle[key] = serde_json::json!(utc);
                }
            }
        }
        let captured_ack_entry = valid_response["ack"]["journal_entries"]
            .as_array()
            .unwrap()
            .first()
            .unwrap()
            .clone();
        let mut missing_ack = valid_response.clone();
        missing_ack.as_object_mut().unwrap().remove("ack");
        let mut missing_captured_journal_ack = valid_response.clone();
        missing_captured_journal_ack["ack"]["journal_entries"] = serde_json::json!([]);
        let mut duplicate_ack = valid_response.clone();
        duplicate_ack["ack"]["journal_entries"]
            .as_array_mut()
            .unwrap()
            .push(captured_ack_entry.clone());
        let mut extra_ack = valid_response.clone();
        let mut extra_entry = captured_ack_entry.clone();
        extra_entry["logical_key"]["cycle_id"] =
            serde_json::json!("00000000-0000-4000-a000-000000000099");
        extra_ack["ack"]["journal_entries"]
            .as_array_mut()
            .unwrap()
            .push(extra_entry);
        let mut wrong_ack_revision = valid_response.clone();
        wrong_ack_revision["ack"]["journal_entries"][0]["revision"] = serde_json::json!(2);
        let mut unmatched_confirmation = valid_response;
        unmatched_confirmation["journal_confirmation"]["entries"][0]["payload_hash"] =
            serde_json::json!("0".repeat(64));

        let cases = [
            ("cycle", wrong_cycle, false),
            ("schema version", wrong_version, false),
            ("account correlation", wrong_correlation, false),
            (
                "journal confirmation generation",
                wrong_journal_generation,
                false,
            ),
            (
                "unexpected journal deletion",
                unexpected_journal_deletion,
                false,
            ),
            ("missing old journal cycle", missing_old_cycle, false),
            ("missing new journal cycle", missing_new_cycle, false),
            ("extra journal cycle", extra_cycle, false),
            ("changed old journal cycle", changed_old_cycle, false),
            ("changed new journal cycle", changed_new_cycle, false),
            (
                "equivalent UTC journal timestamps",
                equivalent_utc_journal_timestamps,
                true,
            ),
            ("ACK object", missing_ack, false),
            (
                "captured journal ACK entry",
                missing_captured_journal_ack,
                false,
            ),
            ("duplicate captured journal ACK entry", duplicate_ack, false),
            ("extra journal ACK entry", extra_ack, false),
            ("captured journal ACK revision", wrong_ack_revision, false),
            ("journal confirmation entry", unmatched_confirmation, false),
        ];
        let outcomes = cases
            .iter()
            .map(|(case, response, should_succeed)| {
                let (result, _) = run_guest_import_v2_transport(
                    &request,
                    MockResponse::Json(200, response.to_string()),
                );
                (*case, result.is_ok(), *should_succeed)
            })
            .collect::<Vec<_>>();
        let mismatches = outcomes
            .iter()
            .filter(|(_, actual, expected)| actual != expected)
            .collect::<Vec<_>>();
        assert!(
            mismatches.is_empty(),
            "unexpected imported journal validation outcomes: {mismatches:?}"
        );
    }

    #[test]
    fn guest_import_v2_transport_rejects_held_status_with_imported_success_fields() {
        let request = guest_import_v2_transport_request();
        let mut response = valid_imported_v2_transport_response(&request);
        response["status"] = serde_json::json!("source_unverifiable");
        let (result, _) =
            run_guest_import_v2_transport(&request, MockResponse::Json(200, response.to_string()));

        assert!(matches!(result, Err(SyncError::InvalidResponse)));
    }

    fn empty_shop_state() -> ShopState {
        let empty_item = || AvatarEquipmentItem {
            sku: None,
            version: 0,
        };
        ShopState {
            account_id: "account-1".into(),
            current_cycle_id: "cycle-1".into(),
            catalog_revision: 1,
            state_revision: 1,
            available_balance: 42,
            products: vec![],
            landscape_instances: vec![],
            placements: vec![],
            removed_natural_keys: vec![],
            avatar_owned_skus: vec![],
            avatar_equipment: AvatarEquipment {
                head: empty_item(),
                outfit: empty_item(),
                face: empty_item(),
                back: empty_item(),
            },
            effects: ActiveEffects::default(),
            reward_state: RewardState::default(),
            action_unavailable_reason: None,
            guest_import_pending: false,
            guest_import_error: None,
        }
    }

    fn empty_planet_state(cycle_id: &str) -> PlanetState {
        PlanetState {
            version: 1,
            profile: Some(PlanetProfile {
                nickname: "Reset Test".into(),
                avatar: PlanetAvatar::Masculine,
            }),
            timezone: "UTC".into(),
            current_cycle_id: cycle_id.into(),
            cycle_started_at_utc: "2026-10-01T00:00:00Z".into(),
            last_reset_at_utc: None,
            wallet_balance: 0,
            wallet_credits: vec![],
            current_planet_tokens: 0,
            lifetime_tokens: 0,
            growth_credit: 0.0,
            stage: 0,
            progress_to_next: 0.0,
            incomplete: false,
            can_reset: true,
            reset_available_at_utc: None,
            objects: vec![],
            removed_natural_keys: vec![],
        }
    }

    fn reset_shop_result(request_id: &str, cycle_id: &str) -> ResetShopResult {
        let mut state = empty_shop_state();
        state.current_cycle_id = cycle_id.into();
        ResetShopResult {
            action: ShopActionResult {
                status: ShopActionStatus::Reset,
                request_id: request_id.into(),
                confirmed_quote: None,
                state,
            },
            planet_state: empty_planet_state(cycle_id),
        }
    }

    fn purchase_quote() -> ShopQuote {
        ShopQuote {
            target: QuoteTarget::Purchase {
                sku: "garden_lamp".into(),
            },
            catalog_revision: 1,
            effect_revision: 2,
            price: 50_000,
        }
    }

    fn assert_account_request(request: &CapturedRequest, path: &str) {
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, path);
        assert_eq!(
            request.headers.get("apikey").map(String::as_str),
            Some("publishable-key")
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer account-token")
        );
    }

    #[test]
    fn reset_rpc_body_contains_only_the_typed_request_id_and_expected_old_cycle() {
        let request_id = uuid::Uuid::parse_str("70000000-0000-0000-0000-000000000001").unwrap();
        let body = reset_my_planet_rpc_body(request_id, "expected-old-cycle");

        assert_eq!(
            body,
            serde_json::json!({
                "p_request_id": "70000000-0000-0000-0000-000000000001",
                "p_cycle_id": "expected-old-cycle"
            })
        );
        assert_eq!(body.as_object().unwrap().len(), 2);
    }

    #[test]
    fn reset_response_requires_a_consistent_composite_and_matching_request_id() {
        let request_id = "70000000-0000-0000-0000-000000000001";
        let result = reset_shop_result(request_id, "server-generated-cycle");
        let wire = serde_json::to_value(&result).unwrap();
        let decoded: ResetShopResult = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded.action.request_id, request_id);
        assert_eq!(
            decoded.action.state.current_cycle_id,
            decoded.planet_state.current_cycle_id
        );
        assert!(validate_reset_shop_result(decoded, request_id, "expected-old-cycle").is_ok());

        assert!(
            serde_json::from_value::<ResetShopResult>(
                serde_json::to_value(empty_planet_state("legacy-cycle")).unwrap()
            )
            .is_err(),
            "a legacy planet-only response must not decode as a reset result"
        );

        let mut extra_field = wire;
        extra_field["ignored"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ResetShopResult>(extra_field).is_err());

        assert!(matches!(
            validate_reset_shop_result(
                reset_shop_result(
                    "70000000-0000-0000-0000-000000000002",
                    "server-generated-cycle"
                ),
                request_id,
                "expected-old-cycle",
            ),
            Err(SyncError::InvalidResponse),
        ));

        let mut split_cycles = reset_shop_result(request_id, "server-generated-cycle");
        split_cycles.planet_state.current_cycle_id = "another-cycle".into();
        assert!(matches!(
            validate_reset_shop_result(split_cycles, request_id, "expected-old-cycle"),
            Err(SyncError::InvalidResponse),
        ));

        assert!(
            matches!(
                validate_reset_shop_result(
                    reset_shop_result(request_id, "expected-old-cycle"),
                    request_id,
                    "expected-old-cycle",
                ),
                Err(SyncError::InvalidResponse),
            ),
            "a reset success must return the server-generated next cycle"
        );
    }

    #[test]
    fn canonical_shop_rpc_methods_send_typed_bodies_and_decode_current_responses() {
        let state = empty_shop_state();
        let (url, server) = spawn_rpc_server(MockResponse::Json(
            200,
            serde_json::to_string(&state).unwrap(),
        ));
        let client = SupabaseSyncClient::new(&url, "publishable-key");
        let decoded = run_async(client.get_my_shop_state("account-token")).unwrap();
        assert_eq!(decoded, state);
        let request = server.join().unwrap();
        assert_account_request(&request, "/rest/v1/rpc/get_my_shop_state");
        assert_eq!(request.body, serde_json::json!({}));

        let quote = purchase_quote();
        let (url, server) = spawn_rpc_server(MockResponse::Json(
            200,
            serde_json::to_string(&quote).unwrap(),
        ));
        let client = SupabaseSyncClient::new(&url, "publishable-key");
        let decoded = run_async(client.quote_shop_action("account-token", &quote.target)).unwrap();
        assert_eq!(decoded, quote);
        let request = server.join().unwrap();
        assert_account_request(&request, "/rest/v1/rpc/quote_shop_action");
        assert_eq!(
            request.body,
            serde_json::json!({ "p_target": quote.target })
        );

        let shop_request = ShopRequest::Purchase {
            request_id: "stable-request-id".into(),
            quote: quote.clone(),
        };
        let result = ShopActionResult {
            status: ShopActionStatus::Purchased,
            request_id: "stable-request-id".into(),
            confirmed_quote: Some(quote),
            state,
        };
        let (url, server) = spawn_rpc_server(MockResponse::Json(
            200,
            serde_json::to_string(&result).unwrap(),
        ));
        let client = SupabaseSyncClient::new(&url, "publishable-key");
        let decoded = run_async(client.apply_shop_action("account-token", &shop_request)).unwrap();
        assert_eq!(decoded, result);
        let request = server.join().unwrap();
        assert_account_request(&request, "/rest/v1/rpc/apply_shop_action");
        assert_eq!(
            request.body,
            serde_json::json!({ "p_request": shop_request })
        );
        assert_eq!(request.body["p_request"]["request_id"], "stable-request-id");
    }

    #[test]
    fn canonical_shop_rpc_methods_preserve_transport_rejection_and_decode_errors() {
        let (url, server) = spawn_rpc_server(MockResponse::Json(401, "{}".into()));
        let client = SupabaseSyncClient::new(&url, "publishable-key");
        assert_eq!(
            run_async(client.get_my_shop_state("account-token")),
            Err(SyncError::Rejected(401)),
        );
        assert_account_request(&server.join().unwrap(), "/rest/v1/rpc/get_my_shop_state");

        let (url, server) = spawn_rpc_server(MockResponse::Json(200, "not json".into()));
        let client = SupabaseSyncClient::new(&url, "publishable-key");
        assert_eq!(
            run_async(client.get_my_shop_state("account-token")),
            Err(SyncError::InvalidResponse),
        );
        assert_account_request(&server.join().unwrap(), "/rest/v1/rpc/get_my_shop_state");

        let (url, server) = spawn_rpc_server(MockResponse::Disconnect);
        let client = SupabaseSyncClient::new(&url, "publishable-key");
        assert_eq!(
            run_async(client.get_my_shop_state("account-token")),
            Err(SyncError::Transport),
        );
        assert_account_request(&server.join().unwrap(), "/rest/v1/rpc/get_my_shop_state");
    }

    #[test]
    fn sharing_reads_public_world_projection_without_guest_import_transport() {
        let (url, server) = spawn_rpc_sequence(vec![
            MockResponse::Json(
                200,
                r#"[{"id":"shared-world","name":"Research","timezone":"UTC","owner_id":"owner-id"}]"#.into(),
            ),
            MockResponse::Json(
                200,
                r#"[{"nickname":"Nova","avatar":"feminine","stage":0,"current_planet_tokens":0,"lifetime_tokens":1000,"growth_credit":1.0,"progress_to_next":0.2,"incomplete":false,"objects":[],"equipped_cosmetics":[{"slot_id":"head","sku":"avatar_hat"}],"token_rank":1,"civilization_rank":1}]"#.into(),
            ),
        ]);
        let client = SupabaseSyncClient::new(&url, "publishable-key");

        let world = run_async(client.current_world("account-token"))
            .unwrap()
            .unwrap();
        let planets = run_async(client.world_planets("account-token", &world.id)).unwrap();

        assert_eq!(world.id, "shared-world");
        assert_eq!(planets.len(), 1);
        assert_eq!(planets[0].nickname, "Nova");
        assert_eq!(planets[0].equipped_cosmetics[0].sku, "avatar_hat");
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].path,
            "/rest/v1/worlds?select=id,name,timezone,owner_id"
        );
        assert!(requests[0].body.is_null());
        assert_eq!(requests[1].method, "POST");
        assert_eq!(requests[1].path, "/rest/v1/rpc/get_world_planets");
        assert_eq!(
            requests[1].body,
            serde_json::json!({"p_world_id":"shared-world"})
        );
        assert!(!requests.iter().any(|request| {
            request.path.contains("guest") || request.body.get("p_request").is_some()
        }));
    }

    #[test]
    fn guest_import_mock_transport_preserves_a_source_unverifiable_hold() {
        let import = GuestCosmeticImport {
            import_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
            wallet_credits: vec![PlanetWalletCredit {
                previous_cycle_id: "guest-cycle".into(),
                amount: 1_000_000,
                created_at_utc: "2026-10-02T00:00:00Z".into(),
            }],
            purchases: vec![],
        };
        let response = serde_json::json!({
            "import_id": import.import_id,
            "status": "source_unverifiable",
            "available_balance": 0
        });
        let (url, server) = spawn_rpc_server(MockResponse::Json(200, response.to_string()));
        let client = SupabaseSyncClient::new(&url, "publishable-key");

        let result = run_async(client.import_guest_cosmetics("account-token", &import)).unwrap();

        assert_eq!(result.import_id, import.import_id);
        assert_eq!(result.status, "source_unverifiable");
        assert_eq!(result.available_balance, 0);
        let request = server.join().unwrap();
        assert_account_request(&request, "/rest/v1/rpc/import_my_guest_cosmetics");
        assert_eq!(request.body["p_import_id"], import.import_id);
        assert_eq!(request.body["p_wallet_credits"][0]["amount"], 1_000_000);
        assert_eq!(request.body["p_purchases"], serde_json::json!([]));
    }

    #[test]
    fn personal_effect_sync_exposes_typed_timeline_and_contribution_methods() {
        let _timeline_method = SupabaseSyncClient::get_my_shop_effect_timeline;
        let _contribution_method = SupabaseSyncClient::upload_planet_state_with_effects;
    }

    #[test]
    fn personal_effect_timeline_uses_empty_rpc_body_and_strict_typed_response() {
        let response = serde_json::json!({
            "account_id": "00000000-0000-0000-0000-000000000021",
            "current_cycle_id": "cycle-1",
            "effect_revision": 7,
            "server_time_utc": "2026-10-02T00:00:00Z",
            "reward_timezone": "UTC",
            "cycle_bounds": [{
                "cycle_id": "cycle-1",
                "started_at_utc": "2026-10-01T00:00:00Z",
                "ended_at_utc": null
            }],
            "intervals": [{
                "cycle_id": "cycle-1",
                "revision": 7,
                "started_at_utc": "2026-10-01T00:00:00Z",
                "ended_at_utc": null,
                "active_instance_ids": [],
                "effects": {
                    "token_earning_bps": 100,
                    "civilization_growth_bps": 0,
                    "shop_discount_bps": 0,
                    "reset_cooldown_bps": 0,
                    "natural_removal_discount_bps": 0,
                    "era_reward_tokens": 0,
                    "streak_reward_tokens": 0
                }
            }]
        });
        let timeline: ShopEffectTimeline = serde_json::from_value(response.clone()).unwrap();

        assert_eq!(serde_json::to_value(timeline).unwrap(), response);
        assert_eq!(
            get_my_shop_effect_timeline_rpc_body(),
            serde_json::json!({})
        );
        let _method = SupabaseSyncClient::get_my_shop_effect_timeline;
    }

    #[test]
    fn effect_contribution_rpc_body_is_flat_canonical_and_has_no_wallet_claims() {
        let mut state = empty_planet_state("cycle-1");
        state.wallet_balance = 99_000;
        state.wallet_credits = vec![PlanetWalletCredit {
            previous_cycle_id: "old-cycle".into(),
            amount: 99_000,
            created_at_utc: "2026-10-01T00:00:00Z".into(),
        }];
        let contribution = PlanetDeviceContributionSnapshot {
            raw: PlanetDeviceContribution {
                device_id: "00000000-0000-0000-0000-000000000022".into(),
                current_cycle_id: "cycle-1".into(),
                lifetime_tokens: 150,
                current_planet_tokens: 100,
                daily_tokens: std::collections::BTreeMap::from([("2026-10-01".into(), 100)]),
                incomplete: false,
            },
            canonical_version: 8,
            daily_segments: vec![PlanetEffectContributionSegment {
                cycle_id: "cycle-1".into(),
                date: "2026-10-01".into(),
                effect_revision: 7,
                tokens: 100,
            }],
            activity_days: vec![PlanetActivityDayContribution {
                reward_date: "2026-10-01".into(),
                cycle_id: "cycle-1".into(),
                first_occurred_at_utc: "2026-10-01T00:00:00Z".into(),
                tokens: 100,
            }],
        };

        let body = upload_planet_state_with_effects_rpc_body(&state, &contribution);

        assert_eq!(body["p_state"]["wallet_balance"], 0);
        assert_eq!(body["p_state"]["wallet_credits"], serde_json::json!([]));
        assert_eq!(
            body["p_device_contribution"],
            serde_json::json!({
                "device_id": "00000000-0000-0000-0000-000000000022",
                "current_cycle_id": "cycle-1",
                "lifetime_tokens": 150,
                "current_planet_tokens": 100,
                "daily_tokens": {"2026-10-01": 100},
                "incomplete": false,
                "canonical_version": 8,
                "daily_segments": [{
                    "cycle_id": "cycle-1",
                    "date": "2026-10-01",
                    "effect_revision": 7,
                    "tokens": 100
                }],
                "activity_days": [{
                    "reward_date": "2026-10-01",
                    "cycle_id": "cycle-1",
                    "first_occurred_at_utc": "2026-10-01T00:00:00Z",
                    "tokens": 100
                }]
            })
        );
        let _method = SupabaseSyncClient::upload_planet_state_with_effects;
    }

    #[test]
    fn failed_shop_action_retries_keep_the_same_request_id_without_hidden_retry() {
        let quote = purchase_quote();
        let request = ShopRequest::Purchase {
            request_id: "retry-stable-request-id".into(),
            quote: quote.clone(),
        };
        let result = ShopActionResult {
            status: ShopActionStatus::Purchased,
            request_id: "retry-stable-request-id".into(),
            confirmed_quote: Some(quote),
            state: empty_shop_state(),
        };
        let success = serde_json::to_string(&result).unwrap();

        for (failure, expected_error) in [
            (
                MockResponse::Json(401, "{}".into()),
                SyncError::Rejected(401),
            ),
            (
                MockResponse::Json(200, "not json".into()),
                SyncError::InvalidResponse,
            ),
            (MockResponse::Disconnect, SyncError::Transport),
        ] {
            let (url, server) =
                spawn_rpc_sequence(vec![failure, MockResponse::Json(200, success.clone())]);
            let client = SupabaseSyncClient::new(&url, "publishable-key");

            assert_eq!(
                run_async(client.apply_shop_action("account-token", &request)),
                Err(expected_error),
            );
            assert_eq!(
                run_async(client.apply_shop_action("account-token", &request)).unwrap(),
                result,
            );

            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 2);
            for captured in &requests {
                assert_account_request(captured, "/rest/v1/rpc/apply_shop_action");
                assert_eq!(
                    captured.body["p_request"]["request_id"],
                    "retry-stable-request-id"
                );
            }
            assert_eq!(requests[0].body, requests[1].body);
        }
    }

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
        let planet: crate::domain::planet::WorldPlanet =
            serde_json::from_value(value.clone()).unwrap();
        assert_eq!(planet.equipped_cosmetics[0].slot_id, "sky");
        assert_eq!(planet.equipped_cosmetics[0].sku, "star_cluster");
        assert!(value.get("wallet_balance").is_none());

        let mut contaminated = value;
        contaminated["wallet_balance"] = serde_json::json!(100000);
        assert!(
            serde_json::from_value::<crate::domain::planet::WorldPlanet>(contaminated).is_err()
        );
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
        assert_eq!(
            body["p_wallet_credits"][0]["previous_cycle_id"],
            "guest-cycle"
        );
        assert_eq!(
            body["p_purchases"][0]["purchase_id"],
            import.purchases[0].purchase_id
        );
        assert!(body.get("user_id").is_none());
    }

    #[test]
    fn task9_local_api_transport_has_no_proxy_and_rejects_redirects() {
        let builder = super::task9_local_api_http_client_builder();
        let config = format!("{builder:?}");

        assert!(
            !config.contains("proxies:"),
            "the Task 9 client builder must disable all proxy matchers: {config}"
        );
        assert!(
            config.contains("redirect_policy: Policy(None)"),
            "the Task 9 client builder must disable redirects: {config}"
        );
        assert!(
            config.contains("connect_timeout: 5s"),
            "the Task 9 client must keep the shared connect timeout: {config}"
        );
        assert!(
            config.contains("timeout: 20s"),
            "the Task 9 client must keep the shared request timeout: {config}"
        );
    }

    #[test]
    fn task9_local_api_constructor_keeps_the_supabase_url_and_key_fields() {
        let client = SupabaseSyncClient::new_task9_local_api_e2e(
            "http://127.0.0.1:50259/",
            "task9-publishable-key",
            50259,
        );

        assert_eq!(client.base_url, "http://127.0.0.1:50259");
        assert_eq!(client.publishable_key, "task9-publishable-key");
    }

    #[test]
    fn task9_local_api_url_port_requires_a_canonical_loopback_origin_and_owned_port() {
        for (url, port) in [
            ("http://127.0.0.1:49152", 49152),
            ("http://127.0.0.1:50259", 50259),
            ("http://127.0.0.1:65535", 65535),
        ] {
            assert_eq!(super::task9_local_api_origin_port(url), Some(port));
            assert!(super::task9_local_api_url_matches_owned_port(url, port));
        }

        for url in [
            "https://api.example.com:50259",
            "http://api.example.com:50259",
            "https://127.0.0.1:50259",
            "http://localhost:50259",
            "http://user:password@127.0.0.1:50259",
            "http://127.0.0.1:50259/path",
            "http://127.0.0.1:50259/",
            "http://127.0.0.1:50259?query=1",
            "http://127.0.0.1:50259#fragment",
            "http://127.0.0.2:50259",
            "http://192.168.1.10:50259",
            "http://127.0.0.1:49151",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:0",
            "http://127.0.0.1",
            "http://127.0.0.1:050259",
        ] {
            assert_eq!(super::task9_local_api_origin_port(url), None, "{url}");
        }
        assert!(!super::task9_local_api_url_matches_owned_port(
            "http://127.0.0.1:50259",
            50260
        ));
    }

    #[test]
    fn task9_local_api_constructor_rejects_mismatch_and_nonlocal_origins_before_build() {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        for (url, port) in [
            ("http://127.0.0.1:50260", 50259),
            ("https://127.0.0.1:50259", 50259),
            ("http://localhost:50259", 50259),
        ] {
            assert!(catch_unwind(AssertUnwindSafe(|| {
                SupabaseSyncClient::new_task9_local_api_e2e(url, "task9-key", port)
            }))
            .is_err());
        }
    }
}
