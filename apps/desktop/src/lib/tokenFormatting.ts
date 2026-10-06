const TOKEN_UNITS = ["", "K", "M", "B", "T"];

export function formatTokenAmount(value: number): string {
  if (!Number.isFinite(value)) return "—";
  let amount = Math.abs(value);
  let unit = 0;
  while (amount >= 1000 && unit < TOKEN_UNITS.length - 1) {
    amount /= 1000;
    unit += 1;
  }
  // Round from the original quantity to avoid binary errors after scaling.
  let rounded = Math.round(Math.abs(value) / (1000 ** unit / 100)) / 100;
  if (rounded >= 1000 && unit < TOKEN_UNITS.length - 1) {
    rounded /= 1000;
    unit += 1;
  }
  return `${value < 0 && rounded !== 0 ? "-" : ""}${rounded}${TOKEN_UNITS[unit]}`;
}
