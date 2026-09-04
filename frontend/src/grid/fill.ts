// Fill / copy helpers: adjust relative cell references in a formula when it is
// moved to a new location, mirroring Excel's fill behaviour. `$`-absolute
// references (e.g. `$A$1`, `$A1`, `A$1`) stay fixed; plain `A1` references shift
// by the drag offset. Sheet-qualified refs (`Sheet1!A1`) and the sheet prefix are
// preserved. String literals and function names are never touched.

import { columnLabel, parseColumnLabel } from "./coords";

/** Split a formula into literal (string) and code segments. */
function splitSegments(src: string): { text: string; isStr: boolean }[] {
  const out: { text: string; isStr: boolean }[] = [];
  let buf = "";
  let inStr = false;
  let i = 0;
  while (i < src.length) {
    const ch = src[i];
    if (ch === '"') {
      if (!inStr) {
        if (buf) {
          out.push({ text: buf, isStr: false });
        }
        inStr = true;
        buf = '"';
        i++;
      } else if (src[i + 1] === '"') {
        buf += '""';
        i += 2;
      } else {
        buf += '"';
        i++;
        inStr = false;
        out.push({ text: buf, isStr: true });
        buf = "";
      }
    } else {
      buf += ch;
      i++;
    }
  }
  if (buf) out.push({ text: buf, isStr: inStr });
  return out;
}

// A cell reference: optional sheet prefix (`Sheet1!`), then an optional `$`, a
// run of column letters, an optional `$`, and a run of row digits. It must not be
// immediately followed by a letter/digit/`(`/`.` so we don't swallow function
// names (`SUM(`) or ranges-as-identifiers.
const REF =
  /([^A-Za-z0-9_.$]|^)((?:[A-Za-z][A-Za-z0-9_.]*!)?)(\$?)([A-Za-z]+)(\$?)([0-9]+)(?![A-Za-z0-9_.(])/g;

function shiftRef(
  _m: string,
  lead: string,
  sheet: string,
  colDollar: string,
  col: string,
  rowDollar: string,
  row: string,
  dCol: number,
  dRow: number,
): string {
  let colStr = col;
  let rowStr = row;
  if (colDollar === "" && dCol !== 0) {
    const c = parseColumnLabel(col) + dCol;
    if (c >= 0) colStr = columnLabel(c);
  }
  if (rowDollar === "" && dRow !== 0) {
    const r = parseInt(row, 10) + dRow;
    if (r >= 1) rowStr = String(r);
  }
  return lead + sheet + colDollar + colStr + rowDollar + rowStr;
}

export function adjustFormula(src: string, dCol: number, dRow: number): string {
  if (dCol === 0 && dRow === 0) return src;
  return splitSegments(src)
    .map((seg) => (seg.isStr ? seg.text : seg.text.replace(REF, (...a) => shiftRef(a[0], a[1], a[2], a[3], a[4], a[5], a[6], dCol, dRow))))
    .join("");
}
