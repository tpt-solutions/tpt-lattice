import { describe, it, expect } from "vitest";
import { adjustFormula } from "./fill";

describe("adjustFormula", () => {
  it("shifts a relative column reference right by 1", () => {
    expect(adjustFormula("=A1", 1, 0)).toBe("=B1");
  });

  it("leaves an absolute reference unchanged", () => {
    expect(adjustFormula("=$A$1", 2, 3)).toBe("=$A$1");
  });

  it("shifts a relative row reference down by 2", () => {
    expect(adjustFormula("=A1", 0, 2)).toBe("=A3");
  });

  it("adjusts the column of a sheet-qualified reference", () => {
    expect(adjustFormula("=Sheet1!A1", 1, 0)).toBe("=Sheet1!B1");
  });

  it("does not touch references inside a string literal", () => {
    expect(adjustFormula('="text A1 here"', 1, 0)).toBe('="text A1 here"');
  });

  it("preserves mixed absolute/relative axes", () => {
    expect(adjustFormula("=$A1", 1, 0)).toBe("=$A1");
    expect(adjustFormula("=A$1", 0, 2)).toBe("=A$1");
  });

  it("shifts references in a compound formula", () => {
    expect(adjustFormula("=A1+B2", 1, 1)).toBe("=B2+C3");
  });
});
