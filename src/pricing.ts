import type { LocalPriceTable, ModelPrice } from "./efficiency";

export const PRICE_STORAGE_KEY = "vibebar.local-price-table.v1";
export const emptyPriceTable: LocalPriceTable = Object.freeze({}) as LocalPriceTable;

const priceKeys: (keyof ModelPrice)[] = ["input", "output", "reasoning", "cacheRead", "cacheWrite"];

function isSafeKey(value: string): boolean {
  return value.length > 0
    && value.length <= 256
    && !value.split("\u0000").join("").match(/[\u0001-\u001F\u007F]/);
}

function isModelPrice(value: unknown): value is ModelPrice {
  if (value == null || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return priceKeys.every((key) => typeof candidate[key] === "number"
    && Number.isFinite(candidate[key])
    && (candidate[key] as number) >= 0);
}

function sanitizePriceTable(value: unknown): LocalPriceTable {
  if (value == null || typeof value !== "object" || Array.isArray(value)) return {};
  const table: LocalPriceTable = {};
  for (const [key, price] of Object.entries(value)) {
    if (isSafeKey(key) && isModelPrice(price)) {
      table[key] = { ...price };
    }
  }
  return table;
}

export function readPriceTable(storage: Storage | null | undefined): LocalPriceTable {
  if (storage == null) return {};
  try {
    const raw = storage.getItem(PRICE_STORAGE_KEY);
    return raw == null ? {} : sanitizePriceTable(JSON.parse(raw));
  } catch {
    return {};
  }
}

export function writePriceTable(storage: Storage | null | undefined, table: LocalPriceTable): void {
  if (storage == null) return;
  try {
    storage.setItem(PRICE_STORAGE_KEY, JSON.stringify(sanitizePriceTable(table)));
  } catch {
    // Local pricing is optional; a blocked storage implementation must not affect the dashboard.
  }
}
