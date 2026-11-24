import { describe, expect, it } from "vitest";
import { lastUsed, maskedKey } from "./api-keys.js";

describe("lastUsed", () => {
  it("says Never when the key was not used", () => {
    expect(lastUsed(null)).toBe("Never");
    expect(lastUsed(undefined)).toBe("Never");
    expect(lastUsed("not a date")).toBe("Never");
  });

  it("formats a timestamp", () => {
    expect(lastUsed("2026-11-24T10:00:00.000Z")).not.toBe("Never");
  });
});

describe("maskedKey", () => {
  it("shows the prefix only", () => {
    expect(maskedKey("ab12cd34")).toBe("ab12cd34…");
  });
});
