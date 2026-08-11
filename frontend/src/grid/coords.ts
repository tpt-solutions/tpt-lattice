// A1-style address <-> (column, row) helpers. Columns are 0-based; row 0 == "1".

export function columnLabel(col: number): string {
  let c = col + 1;
  let s = "";
  while (c > 0) {
    const rem = (c - 1) % 26;
    s = String.fromCharCode(65 + rem) + s;
    c = Math.floor((c - 1) / 26);
  }
  return s;
}

export function parseColumnLabel(label: string): number {
  let col = 0;
  for (const ch of label.toUpperCase()) {
    col = col * 26 + (ch.charCodeAt(0) - 64);
  }
  return col - 1; // 1-based input -> 0-based
}

export function toA1(col: number, row: number): string {
  return `${columnLabel(col)}${row + 1}`;
}

export function parseA1(a1: string): { col: number; row: number } | null {
  const m = a1.match(/^([A-Za-z]+)(\d+)$/);
  if (!m) return null;
  return { col: parseColumnLabel(m[1]), row: parseInt(m[2], 10) - 1 };
}
