use std::collections::BTreeMap;

use super::planet::{PlanetAvatar, PlanetState, PlanetWalletCredit};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShopCategory {
    Landscape,
    Avatar,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementZone {
    Ground,
    Sky,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AvatarSlot {
    Head,
    Outfit,
    Face,
    Back,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShopEffectType {
    TokenEarning,
    CivilizationGrowth,
    ShopDiscount,
    ResetCooldown,
    NaturalRemovalDiscount,
    EraReward,
    StreakReward,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShopProduct {
    pub sku: String,
    pub category: ShopCategory,
    pub display_name: String,
    pub price: u64,
    pub catalog_revision: u32,
    pub purchasable: bool,
    pub placement_zone: Option<PlacementZone>,
    pub avatar_slot: Option<AvatarSlot>,
    pub effect_type: Option<ShopEffectType>,
    /// Percent effects use basis points; era and streak rewards use tokens.
    pub effect_value: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LandscapeInstance {
    pub instance_id: String,
    pub sku: String,
    pub variation_index: u8,
    pub seed: String,
    pub variation_version: u32,
    #[serde(default)]
    pub placement_version: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LandscapePlacement {
    pub instance_id: String,
    pub cycle_id: String,
    pub x: f64,
    pub y: f64,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AvatarEquipmentItem {
    pub sku: Option<String>,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AvatarEquipment {
    pub head: AvatarEquipmentItem,
    pub outfit: AvatarEquipmentItem,
    pub face: AvatarEquipmentItem,
    pub back: AvatarEquipmentItem,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveEffects {
    pub token_earning_bps: u16,
    pub civilization_growth_bps: u16,
    pub shop_discount_bps: u16,
    pub reset_cooldown_bps: u16,
    pub natural_removal_discount_bps: u16,
    pub era_reward_tokens: u64,
    pub streak_reward_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShopEffectTimeline {
    pub account_id: String,
    pub current_cycle_id: String,
    /// Account-global high-water revision; sparse visible intervals may end earlier.
    pub effect_revision: u64,
    pub server_time_utc: String,
    pub reward_timezone: String,
    pub cycle_bounds: Vec<ShopCycleBound>,
    pub intervals: Vec<ShopEffectInterval>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShopCycleBound {
    pub cycle_id: String,
    pub started_at_utc: String,
    pub ended_at_utc: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShopEffectInterval {
    pub cycle_id: String,
    pub revision: u64,
    pub started_at_utc: String,
    pub ended_at_utc: Option<String>,
    pub active_instance_ids: Vec<String>,
    pub effects: ActiveEffects,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectContribution {
    pub device_id: String,
    pub cycle_id: String,
    pub date: String,
    pub effect_revision: u64,
    pub tokens: u64,
    pub growth_bps: u16,
    pub wallet_bps: u16,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RewardState {
    pub reward_timezone: String,
    pub settled_cycle_tokens: u64,
    pub era_reward_tokens: u64,
    pub streak_reward_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShopState {
    pub account_id: String,
    pub current_cycle_id: String,
    pub catalog_revision: u32,
    pub state_revision: u64,
    pub available_balance: u64,
    pub products: Vec<ShopProduct>,
    pub landscape_instances: Vec<LandscapeInstance>,
    pub placements: Vec<LandscapePlacement>,
    #[serde(default)]
    pub removed_natural_keys: Vec<NaturalObjectKey>,
    pub avatar_owned_skus: Vec<String>,
    pub avatar_equipment: AvatarEquipment,
    pub effects: ActiveEffects,
    pub reward_state: RewardState,
    pub action_unavailable_reason: Option<String>,
    pub guest_import_pending: bool,
    pub guest_import_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NaturalObjectKey {
    pub cycle_id: String,
    pub stage: u8,
    pub ordinal: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuoteTarget {
    Purchase { sku: String },
    RemoveNatural { key: NaturalObjectKey },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShopQuote {
    pub target: QuoteTarget,
    pub catalog_revision: u32,
    pub effect_revision: u64,
    pub price: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShopRequest {
    Purchase {
        request_id: String,
        quote: ShopQuote,
    },
    Place {
        request_id: String,
        cycle_id: String,
        instance_id: String,
        expected_version: u64,
        x: f64,
        y: f64,
    },
    Retrieve {
        request_id: String,
        cycle_id: String,
        instance_id: String,
        expected_version: u64,
    },
    EquipAvatar {
        request_id: String,
        slot: AvatarSlot,
        sku: Option<String>,
        expected_version: u64,
    },
    RemoveNatural {
        request_id: String,
        key: NaturalObjectKey,
        expected_version: u64,
        quote: ShopQuote,
    },
    ResetPlanet {
        request_id: String,
        cycle_id: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShopActionStatus {
    Purchased,
    Placed,
    Retrieved,
    Equipped,
    Unequipped,
    Removed,
    Reset,
    LimitReached,
    AlreadyOwned,
    InsufficientBalance,
    QuoteChanged,
    CatalogMismatch,
    VersionConflict,
    CycleMismatch,
    NotOwned,
    AlreadyRemoved,
    RequestConflict,
    InvalidPlacement,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShopActionResult {
    pub status: ShopActionStatus,
    pub request_id: String,
    #[serde(default)]
    pub confirmed_quote: Option<ShopQuote>,
    pub state: ShopState,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResetShopResult {
    pub action: ShopActionResult,
    pub planet_state: PlanetState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShopError {
    InvalidProduct,
    InvalidPlacement,
    InvalidDiscount,
    InvalidContribution,
    ArithmeticOverflow,
}

impl std::fmt::Display for ShopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidProduct => "unknown or incompatible shop product",
            Self::InvalidPlacement => "landscape placement is outside the allowed area",
            Self::InvalidDiscount => "discount exceeds the supported range",
            Self::InvalidContribution => "effect contribution is inconsistent",
            Self::ArithmeticOverflow => "shop calculation exceeds the supported integer range",
        })
    }
}

impl std::error::Error for ShopError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CosmeticSlot {
    pub slot_id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CosmeticProduct {
    pub sku: String,
    pub slot_id: String,
    pub display_name: String,
    pub price: u64,
    pub catalog_revision: u32,
    pub purchasable: bool,
    #[serde(default)]
    pub placement_zone: Option<PlacementZone>,
    #[serde(default)]
    pub avatar_slot: Option<AvatarSlot>,
    #[serde(default)]
    pub effect_type: Option<ShopEffectType>,
    #[serde(default)]
    pub effect_value: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EquippedCosmetic {
    pub slot_id: String,
    pub sku: String,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CosmeticPurchaseStatus {
    Purchased,
    AlreadyOwned,
    InsufficientBalance,
    CatalogMismatch,
    RequestConflict,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CosmeticPurchaseResult {
    pub purchase_id: String,
    pub sku: String,
    pub status: CosmeticPurchaseStatus,
    pub price: u64,
    pub available_balance: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestCosmeticPurchase {
    pub purchase_id: String,
    pub sku: String,
    pub price: u64,
    pub purchased_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestCosmeticImport {
    pub import_id: String,
    pub wallet_credits: Vec<PlanetWalletCredit>,
    pub purchases: Vec<GuestCosmeticPurchase>,
}

/// This describes only a durable local capture. It does not prove server freshness,
/// source provenance, or eligibility to grant imported credits.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestShopImportDisposition {
    LocalIntegrityValidated,
    SourceUnverifiable,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestShopImportStatus {
    pub snapshot: GuestShopImportSnapshot,
    /// False means the immutable captured payload no longer matches the local source.
    pub source_matches_current: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestShopImportSnapshot {
    pub import_id: String,
    pub target_account_id: String,
    pub source_account_id: String,
    pub source_fingerprint: String,
    pub disposition: GuestShopImportDisposition,
    pub data: GuestShopImportData,
}

/// Explicitly typed guest state. Raw usage records, source paths, prompts, and log contents
/// are excluded; only aggregates and growth-journal proof metadata are captured.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestShopImportData {
    pub world_timezone: String,
    pub planet_timezone: String,
    pub reward_timezone: String,
    pub planet_device_id: String,
    pub shop_state_revision: u64,
    pub profile: Option<GuestPlanetProfile>,
    pub activation_at_utc: String,
    pub current_cycle: GuestShopCycle,
    pub historical_cycles: Vec<GuestShopCycle>,
    pub last_reset_at_utc: Option<String>,
    pub reset_available_at_utc: Option<String>,
    pub effect_timeline_state: Option<GuestEffectTimelineState>,
    pub effect_cycle_bounds_authoritative: bool,
    pub contribution_canonical_version: Option<u64>,
    pub natural_objects: Vec<GuestNaturalObject>,
    pub landscape_instances: Vec<GuestLandscapeInstance>,
    pub placements: Vec<GuestLandscapePlacement>,
    pub landscape_edit_versions: Vec<GuestLandscapeEditVersion>,
    pub avatar_owned: Vec<GuestAvatarOwned>,
    pub avatar_equipment: Vec<GuestAvatarEquipment>,
    pub cosmetic_equipment: Vec<GuestCosmeticEquipment>,
    pub pending_purchases: Vec<GuestPendingPurchase>,
    pub cosmetic_purchases: Vec<GuestShopPurchase>,
    pub purchases: Vec<GuestShopPurchase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub purchase_proofs: Vec<GuestPurchaseProof>,
    pub natural_removals: Vec<GuestNaturalRemoval>,
    pub removal_debits: Vec<GuestRemovalDebit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removal_proofs: Vec<GuestRemovalProof>,
    pub effect_history: Vec<ShopEffectInterval>,
    pub effect_cycle_bounds: Vec<ShopCycleBound>,
    pub effect_contributions: Vec<GuestEffectContribution>,
    pub activity_days: Vec<GuestActivityDay>,
    pub game_rewards: Vec<GuestGameReward>,
    pub wallet_credits: Vec<GuestShopWalletCredit>,
    /// Local wallet claims are preserved as evidence only; they are not grantable credits.
    pub unverified_planet_wallet_claims: Vec<GuestUnverifiedWalletClaim>,
    pub cycle_settlements: Vec<GuestCycleSettlement>,
    pub era_progress: Vec<GuestEraProgress>,
    pub daily_agent_totals: Vec<GuestDailyAgentTotal>,
    pub usage_aggregates: Vec<GuestUsageAggregate>,
    pub lifetime_usage_tokens: Option<u64>,
    pub current_cycle_usage_tokens: Option<u64>,
    pub cycle_usage_totals: Vec<GuestCycleUsageTotal>,
    pub growth_journal_state: Option<GuestGrowthJournalState>,
    pub growth_journal_cycles: Vec<GuestGrowthJournalCycle>,
    pub growth_journal_entries: Vec<GuestGrowthJournalEntry>,
    pub reset_settlement_proofs: Vec<GuestResetSettlementProof>,
    pub integrity_issues: Vec<GuestShopImportIntegrityIssue>,
    /// True when a reset, wallet, or settlement record cannot be tied to a complete typed proof.
    pub reset_receipts_unverifiable: bool,
    pub legacy_partial_import_pending: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GuestShopImportIntegrityIssue {
    UnknownLandscapeSku {
        sku: String,
    },
    UnknownAvatarSku {
        sku: String,
    },
    UnknownPurchaseSku {
        sku: String,
    },
    UnknownLegacyCosmeticSku {
        sku: String,
    },
    TooManyLandscapeInstances {
        sku: String,
        count: u64,
    },
    PlacementMissingInstance {
        instance_id: String,
    },
    PlacementCycleMismatch {
        instance_id: String,
        cycle_id: String,
    },
    InvalidPlacementGeometry {
        instance_id: String,
    },
    InvalidTimestamp {
        field: String,
    },
    InvalidTimezone {
        field: String,
    },
    InvalidEffectTimeline {
        reason: String,
    },
    ActivityCycleUnknown {
        cycle_id: String,
    },
    TooManyNaturalObjects {
        count: usize,
    },
    PurchaseProofUnverifiable {
        purchase_id: String,
    },
    RemovalProofUnverifiable {
        request_id: String,
    },
    ResetProofUnverifiable {
        previous_cycle_id: String,
    },
    NaturalTombstoneUnverifiable {
        cycle_id: String,
        stage: u8,
        ordinal: u32,
    },
    LandscapeOwnershipUnverifiable {
        instance_id: String,
    },
    AvatarOwnershipUnverifiable {
        purchase_id: String,
    },
    AvatarEquipmentNotOwned {
        slot: String,
        sku: String,
    },
    AvatarEquipmentSlotMismatch {
        slot: String,
        sku: String,
    },
    EffectReferencesUnknownInstance {
        instance_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestPlanetProfile {
    pub nickname: String,
    pub avatar: PlanetAvatar,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestShopCycle {
    pub cycle_id: String,
    pub started_at_utc: Option<String>,
    pub ended_at_utc: Option<String>,
    pub is_current: bool,
    pub settled_bonus_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestEffectTimelineState {
    pub current_cycle_id: String,
    pub effect_revision: u64,
    pub server_time_utc: String,
    pub reward_timezone: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestPendingPurchase {
    pub sku: String,
    pub purchase_id: String,
    pub catalog_revision: u32,
    pub created_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestNaturalObject {
    pub cycle_id: String,
    pub stage: u8,
    pub ordinal: u32,
    pub kind: String,
    pub x: u8,
    pub y: u8,
    pub seed: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestLandscapeInstance {
    pub instance_id: String,
    pub sku: String,
    pub variation_index: u8,
    pub seed: String,
    pub variation_version: u32,
    pub acquired_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestLandscapePlacement {
    pub instance_id: String,
    pub cycle_id: String,
    pub x: f64,
    pub y: f64,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestLandscapeEditVersion {
    pub instance_id: String,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestAvatarOwned {
    pub sku: String,
    pub purchase_id: String,
    pub price: u64,
    pub acquired_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestAvatarEquipment {
    pub slot: String,
    pub sku: Option<String>,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestCosmeticEquipment {
    pub cycle_id: String,
    pub slot_id: String,
    pub sku: Option<String>,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestShopPurchase {
    pub purchase_id: String,
    pub sku: String,
    pub price: u64,
    pub purchased_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestPurchaseProof {
    pub request_id: String,
    pub quote: ShopQuote,
    pub status: ShopActionStatus,
    pub ownership: GuestPurchaseOwnershipProof,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GuestPurchaseOwnershipProof {
    Landscape { instance_id: String },
    Avatar { sku: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestNaturalRemoval {
    pub cycle_id: String,
    pub stage: u8,
    pub ordinal: u32,
    pub version: u64,
    pub removed_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestRemovalDebit {
    pub request_id: String,
    pub amount: u64,
    pub created_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestRemovalProof {
    pub request_id: String,
    pub target: NaturalObjectKey,
    pub quote: ShopQuote,
    pub status: ShopActionStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestEffectContribution {
    pub device_id: String,
    pub cycle_id: String,
    pub date: String,
    pub effect_revision: u64,
    pub canonical_version: u64,
    pub tokens: u64,
    pub growth_bps: u16,
    pub wallet_bps: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestActivityDay {
    pub reward_date: String,
    pub cycle_id: String,
    pub first_occurred_at_utc: String,
    pub canonical_version: u64,
    pub tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestGameReward {
    pub reward_id: String,
    pub trigger_key: String,
    pub kind: String,
    pub cycle_id: String,
    pub amount: u64,
    pub effects: ActiveEffects,
    pub awarded_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestShopWalletCredit {
    pub credit_id: String,
    pub trigger_key: String,
    pub cycle_id: String,
    pub amount: u64,
    pub created_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestUnverifiedWalletClaim {
    pub previous_cycle_id: String,
    pub claimed_amount: u64,
    pub created_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestCycleSettlement {
    pub cycle_id: String,
    pub amount: u64,
    pub settled_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestEraProgress {
    pub cycle_id: String,
    pub stage: u8,
    pub trigger_key: String,
    pub effects: ActiveEffects,
    pub awarded_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestUsageAggregate {
    pub cycle_id: Option<String>,
    pub bucket_date: String,
    pub agent: String,
    pub event_count: u64,
    pub total_tokens: Option<u64>,
    pub coverage: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestDailyAgentTotal {
    pub bucket_date: String,
    pub agent: String,
    pub total_tokens: Option<u64>,
    pub coverage: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestCycleUsageTotal {
    pub cycle_id: String,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestGrowthJournalState {
    pub generation: u64,
    pub deleted_at_utc: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestGrowthJournalCycle {
    pub cycle_id: String,
    pub started_at_utc: Option<String>,
    pub ended_at_utc: Option<String>,
    pub wallet_credit: Option<u64>,
    pub wallet_credit_at_utc: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestGrowthJournalEntry {
    pub device_id: String,
    pub cycle_id: String,
    pub bucket_date: String,
    pub agent: String,
    pub revision: u64,
    pub acknowledged_revision: u64,
    pub generation: u64,
    pub present: bool,
    pub confirmed_tokens: Option<u64>,
    pub coverage: String,
    pub payload_hash: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestResetSettlementProof {
    pub request_id: String,
    pub previous_cycle_id: String,
    pub new_cycle_id: String,
    pub reset_at_utc: String,
    pub raw_wallet_claim: Option<GuestUnverifiedWalletClaim>,
    pub settled_bonus_tokens: Option<u64>,
    pub final_effect_revision: Option<u64>,
    pub final_effects: Option<ActiveEffects>,
    pub final_active_instance_ids: Vec<String>,
    pub old_cycle_started_at_utc: Option<String>,
    pub new_cycle_started_at_utc: Option<String>,
    pub reset_available_at_utc: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CosmeticEquipStatus {
    Equipped,
    Unequipped,
    VersionConflict,
    CycleMismatch,
    CatalogMismatch,
    NotOwned,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CosmeticEquipResult {
    pub status: CosmeticEquipStatus,
    pub cycle_id: String,
    pub slot_id: String,
    pub sku: Option<String>,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GuestCosmeticImportResult {
    pub import_id: String,
    pub status: String,
    pub available_balance: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CosmeticPurchaseAction {
    pub result: Option<CosmeticPurchaseResult>,
    pub state: CosmeticShopState,
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CosmeticEquipAction {
    pub result: Option<CosmeticEquipResult>,
    pub state: CosmeticShopState,
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CosmeticShopState {
    pub slots: Vec<CosmeticSlot>,
    pub products: Vec<CosmeticProduct>,
    pub current_cycle_id: String,
    pub available_balance: u64,
    pub owned_skus: Vec<String>,
    pub equipped: Vec<EquippedCosmetic>,
    pub slot_versions: BTreeMap<String, u64>,
    pub actions_require_online: bool,
    #[serde(default)]
    pub action_unavailable_reason: Option<String>,
    #[serde(default)]
    pub guest_import_pending: bool,
    #[serde(default)]
    pub guest_import_error: Option<String>,
}

pub fn cosmetic_slots() -> Vec<CosmeticSlot> {
    [
        ("sky", "하늘"),
        ("ring", "고리"),
        ("surface", "지표"),
        ("forecourt", "앞마당"),
    ]
    .into_iter()
    .map(|(slot_id, display_name)| CosmeticSlot {
        slot_id: slot_id.into(),
        display_name: display_name.into(),
    })
    .collect()
}

pub fn cosmetic_products() -> Vec<CosmeticProduct> {
    shop_products()
        .into_iter()
        .map(|product| {
            let slot_id = match (
                product.category,
                product.placement_zone,
                product.avatar_slot,
            ) {
                (ShopCategory::Landscape, Some(PlacementZone::Sky), _) => "sky",
                (ShopCategory::Landscape, _, _) => "surface",
                (_, _, Some(AvatarSlot::Head)) => "head",
                (_, _, Some(AvatarSlot::Outfit)) => "outfit",
                (_, _, Some(AvatarSlot::Face)) => "face",
                (_, _, Some(AvatarSlot::Back)) => "back",
                _ => "surface",
            };
            CosmeticProduct {
                sku: product.sku,
                slot_id: slot_id.into(),
                display_name: product.display_name,
                price: product.price,
                catalog_revision: product.catalog_revision,
                purchasable: product.purchasable,
                placement_zone: product.placement_zone,
                avatar_slot: product.avatar_slot,
                effect_type: product.effect_type,
                effect_value: product.effect_value,
            }
        })
        .collect()
}

pub fn legacy_cosmetic_products() -> Vec<CosmeticProduct> {
    [
        ("star_cluster", "sky", "별무리", 100_000, false),
        ("aurora", "sky", "오로라", 500_000, false),
        ("thin_ring", "ring", "얇은 고리", 100_000, false),
        ("double_ring", "ring", "이중 고리", 500_000, false),
        ("flag", "surface", "깃발", 100_000, false),
        ("crystal_tower", "surface", "수정탑", 500_000, false),
        ("star_cluster_v2", "sky", "별무리", 500_000, true),
        ("aurora_v2", "sky", "오로라", 2_000_000, true),
        ("thin_ring_v2", "ring", "얇은 고리", 500_000, true),
        ("double_ring_v2", "ring", "이중 고리", 2_000_000, true),
        ("flag_v2", "surface", "깃발", 500_000, true),
        ("crystal_tower_v2", "surface", "수정탑", 2_000_000, true),
        ("meteor_shower", "sky", "유성우", 1_000_000, true),
        ("moonlets", "ring", "작은 위성들", 3_000_000, true),
        ("flower_garden", "surface", "꽃 정원", 1_000_000, true),
        ("observatory", "surface", "천문대", 5_000_000, true),
        ("pond", "forecourt", "연못", 750_000, true),
        ("lantern", "forecourt", "등불", 1_500_000, true),
        ("rover", "forecourt", "탐사 로버", 3_000_000, true),
        ("greenhouse", "forecourt", "온실", 5_000_000, true),
    ]
    .into_iter()
    .map(
        |(sku, slot_id, display_name, price, purchasable)| CosmeticProduct {
            sku: sku.into(),
            slot_id: slot_id.into(),
            display_name: display_name.into(),
            price,
            catalog_revision: 1,
            purchasable,
            placement_zone: None,
            avatar_slot: None,
            effect_type: None,
            effect_value: 0,
        },
    )
    .collect()
}

pub fn shop_products() -> Vec<ShopProduct> {
    use AvatarSlot::{Back, Face, Head, Outfit};
    use PlacementZone::{Ground, Sky};
    use ShopCategory::{Avatar, Landscape};
    use ShopEffectType::{
        CivilizationGrowth, EraReward, NaturalRemovalDiscount, ResetCooldown, ShopDiscount,
        StreakReward, TokenEarning,
    };

    let mut products = Vec::with_capacity(48);
    let landscape = [
        (
            "token_earning",
            TokenEarning,
            [
                ("land_pond", "연못", Ground),
                ("land_well", "우물", Ground),
                ("land_greenhouse", "온실", Ground),
                ("land_reservoir", "저수지", Ground),
            ],
        ),
        (
            "civilization_growth",
            CivilizationGrowth,
            [
                ("land_crystal", "수정탑", Ground),
                ("land_school", "학교", Ground),
                ("land_observatory", "천문대", Ground),
                ("land_laboratory", "연구소", Ground),
            ],
        ),
        (
            "shop_discount",
            ShopDiscount,
            [
                ("land_market", "시장", Ground),
                ("land_trading_post", "교역소", Ground),
                ("land_freight", "화물 터미널", Ground),
                ("land_bazaar", "대형 상가", Ground),
            ],
        ),
        (
            "reset_cooldown",
            ResetCooldown,
            [
                ("land_rover", "탐사 로버", Ground),
                ("land_clocktower", "시계탑", Ground),
                ("land_launchpad", "발사대", Ground),
                ("land_portal", "포털", Ground),
            ],
        ),
        (
            "natural_removal_discount",
            NaturalRemovalDiscount,
            [
                ("land_toolbox", "정리 도구함", Ground),
                ("land_excavator", "굴착기", Ground),
                ("land_cutter", "암석 절단기", Ground),
                ("land_recycler", "재활용 로봇", Ground),
            ],
        ),
        (
            "era_reward",
            EraReward,
            [
                ("land_flag", "깃발", Ground),
                ("land_thin_ring", "얇은 고리", Sky),
                ("land_double_ring", "이중 고리", Sky),
                ("land_moonlets", "작은 위성들", Sky),
            ],
        ),
        (
            "streak_reward",
            StreakReward,
            [
                ("land_lantern", "등불", Ground),
                ("land_stars", "별무리", Sky),
                ("land_aurora", "오로라", Sky),
                ("land_meteors", "유성우", Sky),
            ],
        ),
        (
            "civilization_growth_extra",
            CivilizationGrowth,
            [
                ("land_garden", "꽃 정원", Ground),
                ("land_tree", "장식 나무", Ground),
                ("land_bench", "벤치", Ground),
                ("land_fountain", "분수", Ground),
            ],
        ),
    ];
    let prices = [5_000_000_u64, 15_000_000, 40_000_000, 100_000_000];
    let reward_values = [500_000_u64, 1_500_000, 4_000_000, 10_000_000];
    let streak_values = [10_000_u64, 30_000, 80_000, 200_000];
    let effect_bps = [100_u64, 150, 200, 300];
    for (_group, effect, items) in landscape {
        for (index, (sku, display_name, zone)) in items.into_iter().enumerate() {
            let effect_value = match effect {
                EraReward => reward_values[index],
                StreakReward => streak_values[index],
                _ => effect_bps[index],
            };
            products.push(ShopProduct {
                sku: sku.into(),
                category: Landscape,
                display_name: display_name.into(),
                price: prices[index],
                catalog_revision: 1,
                purchasable: true,
                placement_zone: Some(zone),
                avatar_slot: None,
                effect_type: Some(effect),
                effect_value,
            });
        }
    }

    let avatars = [
        (
            Head,
            [
                ("avatar_explorer_hat", "탐험가 모자"),
                ("avatar_crown", "왕관"),
                ("avatar_space_helmet", "우주 헬멧"),
                ("avatar_halo", "홀로그램 관"),
            ],
        ),
        (
            Outfit,
            [
                ("avatar_workwear", "작업복"),
                ("avatar_labwear", "연구복"),
                ("avatar_spacesuit", "우주복"),
                ("avatar_nebula_suit", "성운 의상"),
            ],
        ),
        (
            Face,
            [
                ("avatar_glasses", "안경"),
                ("avatar_sunglasses", "선글라스"),
                ("avatar_goggles", "고글"),
                ("avatar_hud", "HUD 바이저"),
            ],
        ),
        (
            Back,
            [
                ("avatar_backpack", "배낭"),
                ("avatar_cape", "망토"),
                ("avatar_jetpack", "제트팩"),
                ("avatar_wings", "에너지 날개"),
            ],
        ),
    ];
    let avatar_prices = [100_000_000_u64, 200_000_000, 350_000_000, 500_000_000];
    for (slot, items) in avatars {
        for (index, (sku, display_name)) in items.into_iter().enumerate() {
            products.push(ShopProduct {
                sku: sku.into(),
                category: Avatar,
                display_name: display_name.into(),
                price: avatar_prices[index],
                catalog_revision: 1,
                purchasable: true,
                placement_zone: None,
                avatar_slot: Some(slot),
                effect_type: None,
                effect_value: 0,
            });
        }
    }
    products
}

pub fn legacy_equivalent(new_sku: &str) -> Option<&'static str> {
    match new_sku {
        "star_cluster_v2" => Some("star_cluster"),
        "aurora_v2" => Some("aurora"),
        "thin_ring_v2" => Some("thin_ring"),
        "double_ring_v2" => Some("double_ring"),
        "flag_v2" => Some("flag"),
        "crystal_tower_v2" => Some("crystal_tower"),
        _ => None,
    }
}

#[cfg(test)]
mod effect_timeline_tests {
    use super::{ActiveEffects, ShopCycleBound, ShopEffectInterval, ShopEffectTimeline};

    #[test]
    fn confirmed_effect_timeline_uses_only_the_personal_wire_contract() {
        let timeline = ShopEffectTimeline {
            account_id: "00000000-0000-0000-0000-000000000031".into(),
            current_cycle_id: "cycle-current".into(),
            effect_revision: 1,
            server_time_utc: "2026-10-01T00:00:00+00:00".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![ShopCycleBound {
                cycle_id: "cycle-current".into(),
                started_at_utc: "2026-09-24T00:00:00+00:00".into(),
                ended_at_utc: None,
            }],
            intervals: vec![ShopEffectInterval {
                cycle_id: "cycle-current".into(),
                revision: 1,
                started_at_utc: "2026-09-24T00:00:00+00:00".into(),
                ended_at_utc: None,
                active_instance_ids: vec!["instance-1".into()],
                effects: ActiveEffects {
                    token_earning_bps: 100,
                    ..ActiveEffects::default()
                },
            }],
        };

        assert_eq!(
            serde_json::to_value(timeline).unwrap(),
            serde_json::json!({
                "account_id": "00000000-0000-0000-0000-000000000031",
                "current_cycle_id": "cycle-current",
                "effect_revision": 1,
                "server_time_utc": "2026-10-01T00:00:00+00:00",
                "reward_timezone": "UTC",
                "cycle_bounds": [{
                    "cycle_id": "cycle-current",
                    "started_at_utc": "2026-09-24T00:00:00+00:00",
                    "ended_at_utc": null
                }],
                "intervals": [{
                    "cycle_id": "cycle-current",
                    "revision": 1,
                    "started_at_utc": "2026-09-24T00:00:00+00:00",
                    "ended_at_utc": null,
                    "active_instance_ids": ["instance-1"],
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
            })
        );
    }

    #[test]
    fn cycle_bounds_are_required_in_the_personal_timeline_contract() {
        let timeline = ShopEffectTimeline {
            account_id: "00000000-0000-0000-0000-000000000031".into(),
            current_cycle_id: "cycle-current".into(),
            effect_revision: 0,
            server_time_utc: "2026-10-01T00:00:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![],
            intervals: vec![],
        };
        let mut wire = serde_json::to_value(timeline).unwrap();
        wire.as_object_mut().unwrap().remove("cycle_bounds");
        assert!(serde_json::from_value::<ShopEffectTimeline>(wire).is_err());
    }
}
