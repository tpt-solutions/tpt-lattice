import { describe, expect, it } from "vitest";
import { keyboardNavigate, type NavRequest } from "./keyboard";

// Cells populated in the grid: A1,B1,C1 (row 0) and A2 (col 0, row 1).
const cells: Record<string, boolean> = {
  "0,0": true,
  "1,0": true,
  "2,0": true,
  "0,1": true,
};
const hasData = (c: number, r: number) => cells[`${c},${r}`] === true;

function nav(over: Partial<NavRequest>): NavRequest {
  return {
    active: { col: 0, row: 0 },
    key: "ArrowDown",
    ctrlKey: false,
    pageRows: 5,
    hasData,
    lastCol: 2,
    lastRow: 1,
    ...over,
  };
}

describe("keyboardNavigate", () => {
  it("clamps arrow movement at the top-left origin", () => {
    const r = keyboardNavigate(nav({ active: { col: 0, row: 0 }, key: "ArrowUp" }));
    expect(r).toEqual({ col: 0, row: 0 });
    const l = keyboardNavigate(nav({ active: { col: 0, row: 0 }, key: "ArrowLeft" }));
    expect(l).toEqual({ col: 0, row: 0 });
  });

  it("moves by one cell for plain arrows", () => {
    expect(keyboardNavigate(nav({ key: "ArrowRight" }))).toEqual({ col: 1, row: 0 });
    expect(keyboardNavigate(nav({ key: "ArrowDown" }))).toEqual({ col: 0, row: 1 });
  });

  it("jumps to the data edge with Ctrl+Arrow", () => {
    expect(keyboardNavigate(nav({ key: "ArrowRight", ctrlKey: true }))).toEqual({
      col: 2,
      row: 0,
    });
    expect(
      keyboardNavigate(nav({ active: { col: 2, row: 0 }, key: "ArrowDown", ctrlKey: true })),
    ).toEqual({ col: 2, row: 0 });
  });

  it("Home / End behave per Excel conventions", () => {
    expect(keyboardNavigate(nav({ key: "Home" }))).toEqual({ col: 0, row: 0 });
    expect(keyboardNavigate(nav({ key: "Home", ctrlKey: true }))).toEqual({ col: 0, row: 0 });
    expect(keyboardNavigate(nav({ active: { col: 1, row: 0 }, key: "End" }))).toEqual({
      col: 2,
      row: 0,
    });
    expect(keyboardNavigate(nav({ key: "End", ctrlKey: true }))).toEqual({ col: 2, row: 1 });
  });

  it("PageUp / PageDown step by a page", () => {
    expect(keyboardNavigate(nav({ key: "PageDown" }))).toEqual({ col: 0, row: 5 });
    expect(keyboardNavigate(nav({ active: { col: 0, row: 8 }, key: "PageUp" }))).toEqual({
      col: 0,
      row: 3,
    });
  });

  it("returns null for non-navigation keys", () => {
    expect(keyboardNavigate(nav({ key: "Enter" }))).toBeNull();
    expect(keyboardNavigate(nav({ key: "a" }))).toBeNull();
  });
});
