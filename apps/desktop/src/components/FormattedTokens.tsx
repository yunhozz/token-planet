import { formatTokenAmount } from "../lib/tokenFormatting";

export function FormattedTokens({ value }: { value: number }) {
  return <>{formatTokenAmount(value)}</>;
}
