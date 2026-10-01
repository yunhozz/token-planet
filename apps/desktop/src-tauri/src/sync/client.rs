use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{sync::OnceLock, time::Duration};

use crate::domain::cosmetic_shop::{
    CosmeticEquipResult, CosmeticPurchaseResult, CosmeticShopState, GuestCosmeticImport,
    GuestCosmeticImportResult, QuoteTarget, ShopActionResult, ShopQuote, ShopRequest, ShopState,
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
    use super::{
        guest_import_rpc_body, one_row, MySyncPolicy, SupabaseSyncClient, SyncError, UploadBody,
        WorldSummary,
    };
    use crate::domain::cosmetic_shop::{
        ActiveEffects, AvatarEquipment, AvatarEquipmentItem, GuestCosmeticImport,
        GuestCosmeticPurchase, QuoteTarget, RewardState, ShopActionResult, ShopActionStatus,
        ShopQuote, ShopRequest, ShopState,
    };
    use crate::domain::planet::PlanetWalletCredit;
    use crate::domain::usage::{Agent, UsageCoverage};
    use crate::sync::aggregate::DailyUsageSnapshot;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread::{self, JoinHandle};

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

    fn spawn_rpc_sequence(responses: Vec<MockResponse>) -> (String, JoinHandle<Vec<CapturedRequest>>) {
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
            if let Some(header_end) = bytes
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
            {
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
                    let body = serde_json::from_slice(
                        &bytes[body_start..body_start + content_length],
                    )
                    .unwrap();
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
        let decoded = run_async(client.quote_shop_action("account-token", &quote.target))
            .unwrap();
        assert_eq!(decoded, quote);
        let request = server.join().unwrap();
        assert_account_request(&request, "/rest/v1/rpc/quote_shop_action");
        assert_eq!(request.body, serde_json::json!({ "p_target": quote.target }));

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
        let decoded = run_async(client.apply_shop_action("account-token", &shop_request))
            .unwrap();
        assert_eq!(decoded, result);
        let request = server.join().unwrap();
        assert_account_request(&request, "/rest/v1/rpc/apply_shop_action");
        assert_eq!(request.body, serde_json::json!({ "p_request": shop_request }));
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
            (MockResponse::Json(401, "{}".into()), SyncError::Rejected(401)),
            (
                MockResponse::Json(200, "not json".into()),
                SyncError::InvalidResponse,
            ),
            (MockResponse::Disconnect, SyncError::Transport),
        ] {
            let (url, server) = spawn_rpc_sequence(vec![
                failure,
                MockResponse::Json(200, success.clone()),
            ]);
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
                assert_eq!(captured.body["p_request"]["request_id"], "retry-stable-request-id");
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
