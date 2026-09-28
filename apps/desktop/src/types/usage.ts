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

export type WorldSnapshot = {
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
