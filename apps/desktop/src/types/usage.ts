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
  token_rank: number;
  civilization_rank: number;
};
