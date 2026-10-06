import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { FormattedTokens } from "../FormattedTokens";
it("renders compact token amount", () => {
  render(<FormattedTokens value={239824} />);
  expect(screen.getByText("239.82K")).toBeInTheDocument();
});
