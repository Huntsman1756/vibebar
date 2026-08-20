export const POPOVER_OPENED_EVENT = "vibebar://popover-opened";

export type RefreshFreshness = "fresh" | "stale" | "unavailable" | "preview";

export type RefreshTransition<T> = {
  snapshot: T | null;
  preview: boolean;
  freshness: RefreshFreshness;
  error: string | null;
};

export type RefreshOutcome<T> =
  | { kind: "success"; snapshot: T }
  | { kind: "failure"; error: string };

export type RefreshController<T> = {
  transition(outcome: RefreshOutcome<T>): RefreshTransition<T>;
};

export function transitionRefresh<T>({
  previousSnapshot,
  isTauriRuntime,
  demoSnapshot,
  outcome,
}: {
  previousSnapshot: T | null;
  isTauriRuntime: boolean;
  demoSnapshot: T;
  outcome: RefreshOutcome<T>;
}): RefreshTransition<T> {
  if (outcome.kind === "success") {
    return { snapshot: outcome.snapshot, preview: false, freshness: "fresh", error: null };
  }

  if (!isTauriRuntime) {
    return { snapshot: demoSnapshot, preview: true, freshness: "preview", error: outcome.error };
  }

  if (previousSnapshot) {
    return { snapshot: previousSnapshot, preview: false, freshness: "stale", error: outcome.error };
  }

  return { snapshot: null, preview: false, freshness: "unavailable", error: outcome.error };
}

export function createRefreshController<T>({
  isTauriRuntime,
  demoSnapshot,
}: {
  isTauriRuntime: boolean;
  demoSnapshot: T;
}): RefreshController<T> {
  let latestSnapshot: T | null = null;

  return {
    transition(outcome) {
      const next = transitionRefresh({ previousSnapshot: latestSnapshot, isTauriRuntime, demoSnapshot, outcome });
      latestSnapshot = next.snapshot;
      return next;
    },
  };
}

export async function openFullDashboard(invokeDashboard: () => Promise<unknown>): Promise<string | null> {
  try {
    await invokeDashboard();
    return null;
  } catch {
    return "Could not open full dashboard.";
  }
}

export function shouldAutoRefreshOnMount(isPopover: boolean): boolean {
  return !isPopover;
}

export function shouldUseDemoFallback(isTauriRuntime: boolean): boolean {
  return !isTauriRuntime;
}
