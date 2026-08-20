import { describe, expect, it } from "vitest";

import type { LocalPriceTable } from "./efficiency";
import { PRICE_STORAGE_KEY, readPriceTable, writePriceTable } from "./pricing";

function memoryStorage(): Storage {
  const values = new Map<string, string>();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => { values.set(key, value); },
    removeItem: (key) => { values.delete(key); },
    clear: () => { values.clear(); },
    key: (index) => [...values.keys()][index] ?? null,
    get length() { return values.size; },
  };
}

describe("local model price table", () => {
  it("returns an empty price table when storage has no configuration", () => {
    expect(readPriceTable(memoryStorage())).toEqual({});
  });

  it("round-trips only finite non-negative model rates", () => {
    const storage = memoryStorage();
    const table: LocalPriceTable = {
      "nan\u0000qwen3.6": { input: 1.2, output: 4.8, reasoning: 0, cacheRead: 0.2, cacheWrite: 1 },
    };

    writePriceTable(storage, table);

    expect(readPriceTable(storage)).toEqual(table);
  });

  it("discards malformed JSON and negative rates", () => {
    const storage = memoryStorage();
    storage.setItem(PRICE_STORAGE_KEY, '{"nan\\u0000qwen3.6":{"input":-1}}');

    expect(readPriceTable(storage)).toEqual({});
  });

  it("does not throw when browser storage is unavailable", () => {
    const brokenStorage = {
      getItem: () => { throw new Error("blocked"); },
      setItem: () => { throw new Error("blocked"); },
    } as unknown as Storage;

    expect(readPriceTable(brokenStorage)).toEqual({});
    expect(() => writePriceTable(brokenStorage, {})).not.toThrow();
  });
});
