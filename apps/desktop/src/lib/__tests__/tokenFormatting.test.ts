import { describe, expect, it } from "vitest";
import { formatTokenAmount } from "../tokenFormatting";

describe("formatTokenAmount", () => {
  it.each([
    [0, "0"],
    [999, "999"],
    [1000, "1K"],
    [1005, "1.01K"],
    [13200, "13.2K"],
    [239824, "239.82K"],
    [999994, "999.99K"],
    [999995, "1M"],
    [999999, "1M"],
    [1000000, "1M"],
    [1000000000, "1B"],
    [1000000000000, "1T"],
    [999999999, "1B"],
    [999999999999, "1T"],
    [-13200, "-13.2K"],
    [NaN, "—"],
    [Infinity, "—"],
    [-Infinity, "—"],
    [7400000000000000, "7400T"],
  ] as const)("formats %s as %s", (value, expected) => {
    expect(formatTokenAmount(value)).toBe(expected);
  });
});
