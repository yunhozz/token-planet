use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use super::planet::PlanetWalletCredit;

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
    [("sky", "하늘"), ("ring", "고리"), ("surface", "지표")]
        .into_iter()
        .map(|(slot_id, display_name)| CosmeticSlot {
            slot_id: slot_id.into(),
            display_name: display_name.into(),
        })
        .collect()
}

pub fn cosmetic_products() -> Vec<CosmeticProduct> {
    [
        ("star_cluster", "sky", "별무리", 100_000),
        ("aurora", "sky", "오로라", 500_000),
        ("thin_ring", "ring", "얇은 고리", 100_000),
        ("double_ring", "ring", "이중 고리", 500_000),
        ("flag", "surface", "깃발", 100_000),
        ("crystal_tower", "surface", "수정탑", 500_000),
    ]
    .into_iter()
    .map(|(sku, slot_id, display_name, price)| CosmeticProduct {
        sku: sku.into(),
        slot_id: slot_id.into(),
        display_name: display_name.into(),
        price,
        catalog_revision: 1,
        purchasable: true,
    })
    .collect()
}
