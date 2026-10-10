export type Agent = "codex" | "claude_code";

export type UsageCoverage =
  | "complete"
  | "partial"
  | "unavailable"
  | "unsupported"
  | "user_disabled";

type TokenBreakdown = {
  input_tokens: number | null;
  output_tokens: number | null;
  cache_read_tokens: number | null;
  cache_write_tokens: number | null;
};

export type TokenUsage = TokenBreakdown &
  (
    | { coverage: "complete"; total_tokens: number }
    | { coverage: "partial"; total_tokens: number | null }
    | { coverage: "unavailable" | "unsupported"; total_tokens: null }
    | { coverage: "user_disabled"; total_tokens: number | null }
  );

export type ScanSummary = {
  codex: TokenUsage;
  claude_code: TokenUsage;
  codex_source: SourceHealth;
  claude_code_source: SourceHealth;
  confirmed_subtotal: number | null;
  complete_total: number | null;
  scanned_at_utc: string;
};

export type SourceHealth = "ready" | "not_found" | "permission_denied" | "unsupported_format" | "usage_unavailable" | "partial" | "user_disabled";

export type PlanetOrdinal = { status: "verified" | "unknown"; current: number | null };

export type WorldSnapshot = {
  generation: number;
  planet_ordinal: PlanetOrdinal;
  usage: ScanSummary;
  growth_credit: number;
  stage: number;
  progress_to_next: number;
  incomplete: boolean;
  planet: PlanetState;
};

export type PlanetAvatar = "masculine" | "feminine";
export type PlanetProfile = { nickname: string; avatar: PlanetAvatar };
export type PlanetObject = {
  stage: number;
  ordinal: number;
  kind: string;
  x: number;
  y: number;
  seed: number;
};
export type PlanetWalletCredit = {
  previous_cycle_id: string;
  amount: number;
  created_at_utc: string;
};
export type CosmeticSlot = {
  slot_id: string;
  display_name: string;
};
export type CosmeticProduct = {
  sku: string;
  slot_id: string;
  display_name: string;
  price: number;
  catalog_revision: number;
  purchasable: boolean;
};
export type EquippedCosmetic = {
  slot_id: string;
  sku: string;
  version: number;
};
export type CosmeticPurchaseStatus = "purchased" | "already_owned" | "insufficient_balance" | "catalog_mismatch" | "request_conflict";
export type CosmeticPurchaseResult = {
  purchase_id: string;
  sku: string;
  status: CosmeticPurchaseStatus;
  price: number;
  available_balance: number;
};
export type CosmeticEquipStatus = "equipped" | "unequipped" | "version_conflict" | "cycle_mismatch" | "catalog_mismatch" | "not_owned";
export type CosmeticEquipResult = {
  status: CosmeticEquipStatus;
  cycle_id: string;
  slot_id: string;
  sku: string | null;
  version: number;
};
export type CosmeticShopState = {
  slots: CosmeticSlot[];
  products: CosmeticProduct[];
  current_cycle_id: string;
  available_balance: number;
  owned_skus: string[];
  equipped: EquippedCosmetic[];
  slot_versions: Record<string, number>;
  actions_require_online: boolean;
  action_unavailable_reason: string | null;
  guest_import_pending: boolean;
  guest_import_error: string | null;
};

export type ShopCategory = "landscape" | "avatar";
export type PlacementZone = "ground" | "sky";
export type AvatarSlot = "head" | "outfit" | "face" | "back";
export type ShopEffectType =
  | "token_earning"
  | "civilization_growth"
  | "shop_discount"
  | "reset_cooldown"
  | "natural_removal_discount"
  | "era_reward"
  | "streak_reward";
export type ShopProduct = {
  sku: string;
  category: ShopCategory;
  display_name: string;
  price: number;
  catalog_revision: number;
  purchasable: boolean;
  placement_zone: PlacementZone | null;
  avatar_slot: AvatarSlot | null;
  effect_type: ShopEffectType | null;
  /** Percent effects use basis points; era and streak rewards use tokens. */
  effect_value: number;
};
export type LandscapeInstance = {
  instance_id: string;
  sku: string;
  variation_index: number;
  seed: string;
  variation_version: number;
  placement_version: number;
};
export type LandscapePlacement = {
  instance_id: string;
  cycle_id: string;
  x: number;
  y: number;
  version: number;
};
export type AvatarEquipmentItem = { sku: string | null; version: number };
export type AvatarEquipment = Record<AvatarSlot, AvatarEquipmentItem>;
export type ActiveEffects = {
  token_earning_bps: number;
  civilization_growth_bps: number;
  shop_discount_bps: number;
  reset_cooldown_bps: number;
  natural_removal_discount_bps: number;
  era_reward_tokens: number;
  streak_reward_tokens: number;
};
export type EffectContribution = {
  device_id: string;
  cycle_id: string;
  date: string;
  effect_revision: number;
  tokens: number;
  growth_bps: number;
  wallet_bps: number;
};
export type RewardState = {
  reward_timezone: string;
  settled_cycle_tokens: number;
  era_reward_tokens: number;
  streak_reward_tokens: number;
};
export type ShopState = {
  account_id: string;
  current_cycle_id: string;
  catalog_revision: number;
  state_revision: number;
  available_balance: number;
  products: ShopProduct[];
  landscape_instances: LandscapeInstance[];
  placements: LandscapePlacement[];
  removed_natural_keys: NaturalObjectKey[];
  avatar_owned_skus: string[];
  avatar_equipment: AvatarEquipment;
  effects: ActiveEffects;
  reward_state: RewardState;
  action_unavailable_reason: string | null;
  guest_import_pending: boolean;
  guest_import_error: string | null;
};
export type NaturalObjectKey = { cycle_id: string; stage: number; ordinal: number };
export type ShopQuoteTarget =
  | { kind: "purchase"; sku: string }
  | { kind: "remove_natural"; key: NaturalObjectKey };
export type ShopQuote = {
  target: ShopQuoteTarget;
  catalog_revision: number;
  effect_revision: number;
  price: number;
};
export type ShopRequest =
  | { kind: "purchase"; request_id: string; quote: ShopQuote }
  | { kind: "place"; request_id: string; cycle_id: string; instance_id: string; expected_version: number; x: number; y: number }
  | { kind: "retrieve"; request_id: string; cycle_id: string; instance_id: string; expected_version: number }
  | { kind: "equip_avatar"; request_id: string; slot: AvatarSlot; sku: string | null; expected_version: number }
  | { kind: "remove_natural"; request_id: string; key: NaturalObjectKey; expected_version: number; quote: ShopQuote }
  | { kind: "reset_planet"; request_id: string; cycle_id: string };
export type ShopActionStatus =
  | "purchased" | "placed" | "retrieved" | "equipped" | "unequipped" | "removed" | "reset"
  | "limit_reached" | "already_owned" | "insufficient_balance" | "quote_changed"
  | "catalog_mismatch" | "version_conflict" | "cycle_mismatch" | "not_owned"
  | "already_removed" | "request_conflict" | "invalid_placement" | "unavailable";
export type ShopActionResult = {
  status: ShopActionStatus;
  request_id: string;
  confirmed_quote: ShopQuote | null;
  state: ShopState;
};
export type CosmeticPurchaseAction = {
  result: CosmeticPurchaseResult | null;
  state: CosmeticShopState;
  unavailable_reason: string | null;
};
export type CosmeticEquipAction = {
  result: CosmeticEquipResult | null;
  state: CosmeticShopState;
  unavailable_reason: string | null;
};
export type GrowthJournalCycle = {
  cycle_id: string;
  started_at_utc: string | null;
  ended_at_utc: string | null;
  wallet_credit: number | null;
  wallet_credit_at_utc: string | null;
};
export type GrowthJournalEntry = {
  device_id: string;
  cycle_id: string;
  bucket_date: string;
  agent: Agent;
  revision: number;
  generation: number;
  present: boolean;
  confirmed_tokens: number | null;
  coverage: UsageCoverage;
  payload_hash: string;
};
export type GrowthJournal = {
  generation: number;
  deleted_at_utc: string | null;
  timezone: string | null;
  cycles: GrowthJournalCycle[];
  entries: GrowthJournalEntry[];
};
export type PlanetState = {
  version: number;
  profile: PlanetProfile | null;
  timezone: string;
  current_cycle_id: string;
  cycle_started_at_utc: string;
  last_reset_at_utc: string | null;
  wallet_balance: number;
  wallet_credits: PlanetWalletCredit[];
  current_planet_tokens: number;
  lifetime_tokens: number;
  growth_credit: number;
  stage: number;
  progress_to_next: number;
  incomplete: boolean;
  can_reset: boolean;
  reset_available_at_utc: string | null;
  objects: PlanetObject[];
  removed_natural_keys: NaturalObjectKey[];
};
export type WorldPlanet = {
  nickname: string;
  avatar: PlanetAvatar;
  stage: number;
  current_planet_tokens: number;
  lifetime_tokens: number;
  growth_credit: number;
  progress_to_next: number;
  incomplete: boolean;
  objects: PlanetObject[];
  equipped_cosmetics: { slot_id: string; sku: string }[];
  token_rank: number;
  civilization_rank: number;
};
