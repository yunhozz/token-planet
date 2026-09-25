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
