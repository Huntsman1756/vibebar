import { describe, expect, it } from "vitest";

import { shouldAutoRefreshOnMount } from "./appRuntime";

describe("shouldAutoRefreshOnMount", () => {
  it("refreshes immediately for the main dashboard window", () => {
    expect(shouldAutoRefreshOnMount(false)).toBe(true);
  });

  it("does not refresh immediately for the hidden popover window", () => {
    expect(shouldAutoRefreshOnMount(true)).toBe(false);
  });
});
