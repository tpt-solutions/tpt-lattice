import { For } from "solid-js";

export interface FormulaHelpProps {
  onClose: () => void;
}

interface Section {
  title: string;
  rows: [string, string][];
}

// A concise reference for LES — the spreadsheet's deliberately Excel-departing
// formula language. Kept in sync with the evaluator's implemented functions.
const SECTIONS: Section[] = [
  {
    title: "Basics",
    rows: [
      ["=A1 + B1", "Formulas start with `=`. Standard arithmetic + - * / and parentheses."],
      ["A1 * 2", "Cell refs, numbers, text (\"hi\"), booleans (true/false)."],
      ["A1 & B1", "Concatenate text with `&` (e.g. A1 & \" \" & B1)."],
      ["RANGE(A1, B5)", "Build a range for aggregate/lookup functions (not the Excel `A1:B5` form)."],
    ],
  },
  {
    title: "Math & logic",
    rows: [
      ["SUM(RANGE(A1, A10))", "Sum a range."],
      ["AVERAGE(RANGE(...))", "Mean of a range."],
      ["MIN / MAX / ABS / SQRT / POW", "Scalar math."],
      ["IF(cond, a, b)", "Two-arg form: returns `b` when cond is false/blank."],
      ["IFERROR(x, fallback)", "Return fallback if x errors."],
      ["IFNA(x, fallback)", "Return fallback if x is #N/A."],
    ],
  },
  {
    title: "Conditional aggregates",
    rows: [
      ["SUMIF(RANGE, crit, RANGE)", "Sum where the criterion matches."],
      ["COUNTIF(RANGE, crit)", "Count matches."],
      ["AVERAGEIF(RANGE, crit, RANGE)", "Average of matches."],
      ["SUMIFS(sum, critRange, crit, ...)", "Multi-criteria sum."],
      ["> < >= <= <> =", "Criterion operators (e.g. \">10\", \"<>0\")."],
    ],
  },
  {
    title: "Lookups",
    rows: [
      ["VLOOKUP(key, RANGE, n)", "Vertical lookup: column n of the matched row."],
      ["HLOOKUP(key, RANGE, n)", "Horizontal lookup: row n of the matched row."],
      ["INDEX(RANGE, i)", "ith value in a range."],
      ["XLOOKUP(key, keys, vals)", "Return the value paired with key."],
    ],
  },
  {
    title: "Text",
    rows: [
      ["UPPER / LOWER / TRIM", "Case and whitespace helpers."],
      ["LEFT(s, n) / RIGHT(s, n) / MID(s, a, n)", "Substrings."],
      ["FIND(needle, hay) / SUBSTITUTE(s, a, b)", "Search and replace."],
      ["REPLACE(s, a, n, b)", "Replace n chars starting at a."],
    ],
  },
  {
    title: "Predicates & stats",
    rows: [
      ["ISBLANK / ISERROR / ISNUMBER / ISTEXT / ISNA", "Type/error checks."],
      ["MEDIAN / STDEV / VAR / MODE / RANK / PERCENTILE", "Statistics over a range."],
    ],
  },
];

export function FormulaHelp(props: FormulaHelpProps) {
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="LES formula reference"
      onClick={props.onClose}
      style={{
        position: "fixed",
        inset: "0",
        background: "rgba(0,0,0,0.35)",
        display: "flex",
        "align-items": "center",
        "justify-content": "center",
        "z-index": "50",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "#fff",
          color: "#111827",
          "border-radius": "8px",
          padding: "18px 22px",
          width: "min(680px, 92vw)",
          "max-height": "82vh",
          overflow: "auto",
          "box-shadow": "0 10px 30px rgba(0,0,0,0.25)",
        }}
      >
        <div style={{ display: "flex", "justify-content": "space-between", "align-items": "center" }}>
          <h2 style={{ margin: "0 0 4px", "font-size": "18px" }}>LES Formula Reference</h2>
          <button onClick={props.onClose} style={{ "font-size": "16px", cursor: "pointer" }}>
            ✕
          </button>
        </div>
        <p style={{ "margin-top": "0", color: "#6b7280", "font-size": "13px" }}>
          LES is the spreadsheet's formula language. It deliberately departs from Excel syntax in a
          few places (notably ranges use <code>RANGE(a, b)</code>).
        </p>
        <For each={SECTIONS}>
          {(section) => (
            <section style={{ "margin-top": "14px" }}>
              <h3 style={{ "font-size": "14px", "margin": "0 0 6px", color: "#2563eb" }}>
                {section.title}
              </h3>
              <table style={{ width: "100%", "border-collapse": "collapse", "font-size": "13px" }}>
                <For each={section.rows}>
                  {([expr, desc]) => (
                    <tr>
                      <td
                        style={{
                          "font-family": "ui-monospace, monospace",
                          "white-space": "nowrap",
                          padding: "3px 8px 3px 0",
                          color: "#0f172a",
                          "vertical-align": "top",
                        }}
                      >
                        {expr}
                      </td>
                      <td style={{ padding: "3px 0", color: "#374151" }}>{desc}</td>
                    </tr>
                  )}
                </For>
              </table>
            </section>
          )}
        </For>
      </div>
    </div>
  );
}
