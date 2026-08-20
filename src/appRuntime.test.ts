import { describe, expect, it } from "vitest";

import {
  createRefreshController,
  openFullDashboard,
  shouldAutoRefreshOnMount,
  shouldUseDemoFallback,
  transitionRefresh,
} from "./appRuntime";

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

describe("transitionRefresh", () => {
  const liveSnapshot = { generatedAt: "2026-08-20T09:00:00.000Z" };
  const refreshedSnapshot = { generatedAt: "2026-08-20T10:00:00.000Z" };
  const demoSnapshot = { generatedAt: "demo" };

  it("retains the last live snapshot and marks it stale after a Tauri refresh failure", () => {
    expect(transitionRefresh({
      previousSnapshot: liveSnapshot,
      isTauriRuntime: true,
      demoSnapshot,
      outcome: { kind: "failure", error: "collector unavailable" },
    })).toEqual({ snapshot: liveSnapshot, preview: false, freshness: "stale", error: "collector unavailable" });
  });

  it("keeps Tauri unavailable when no live snapshot exists", () => {
    expect(transitionRefresh({
      previousSnapshot: null,
      isTauriRuntime: true,
      demoSnapshot,
      outcome: { kind: "failure", error: "collector unavailable" },
    })).toEqual({ snapshot: null, preview: false, freshness: "unavailable", error: "collector unavailable" });
  });

  it("uses demo data only for a browser preview failure", () => {
    expect(transitionRefresh({
      previousSnapshot: null,
      isTauriRuntime: false,
      demoSnapshot,
      outcome: { kind: "failure", error: "invoke is unavailable" },
    })).toEqual({ snapshot: demoSnapshot, preview: true, freshness: "preview", error: "invoke is unavailable" });
  });

  it("stores a successful snapshot and clears stale failure state", () => {
    expect(transitionRefresh({
      previousSnapshot: liveSnapshot,
      isTauriRuntime: true,
      demoSnapshot,
      outcome: { kind: "success", snapshot: refreshedSnapshot },
    })).toEqual({ snapshot: refreshedSnapshot, preview: false, freshness: "fresh", error: null });
  });
});

describe("createRefreshController", () => {
  it("retains the successful snapshot for a later failure without requiring a React re-render", () => {
    const controller = createRefreshController({
      isTauriRuntime: true,
      demoSnapshot: { generatedAt: "demo" },
    });
    const liveSnapshot = { generatedAt: "2026-08-20T10:00:00.000Z" };

    expect(controller.transition({ kind: "success", snapshot: liveSnapshot })).toEqual({
      snapshot: liveSnapshot,
      preview: false,
      freshness: "fresh",
      error: null,
    });
    expect(controller.transition({ kind: "failure", error: "collector unavailable" })).toEqual({
      snapshot: liveSnapshot,
      preview: false,
      freshness: "stale",
      error: "collector unavailable",
    });
  });
});

describe("openFullDashboard", () => {
  it("returns a compact error when the dashboard command rejects", async () => {
    await expect(openFullDashboard(async () => {
      throw new Error("main window unavailable");
    })).resolves.toBe("Could not open full dashboard.");
  });
});
