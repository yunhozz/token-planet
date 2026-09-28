import { render } from "@testing-library/react";
import { expect, it } from "vitest";
import { FormattedNumber } from "../FormattedNumber";

it("keeps the formatted value intact and allows line breaks after thousands separators", () => {
  const { container } = render(<FormattedNumber value={7_400_000_000_000_000} />);
  expect(container.textContent).toBe("7,400,000,000,000,000");
  expect(container.querySelectorAll("wbr")).toHaveLength(5);
});
