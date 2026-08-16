import { describe, expect, it } from "vitest";

import { shouldAutoRefreshOnMount, shouldUseDemoFallback } from "./appRuntime";

describe("shouldAutoRefreshOnMount", () => {
  it("refreshes immediately for the main dashboard window", () => {
    expect(shouldAutoRefreshOnMount(false)).toBe(true);
  });

  it("does not refresh immediately for the hidden popover window", () => {
    expect(shouldAutoRefreshOnMount(true)).toBe(false);
  });

  it("allows demo fallback only outside the Tauri runtime", () => {
    expect(shouldUseDemoFallback(false)).toBe(true);
    expect(shouldUseDemoFallback(true)).toBe(false);
  });
});
