// Column collapsing for the results table: a configuration-key column
// whose value is identical across every visible row is hidden and listed
// in a "shared configuration" line instead. See specs/benchmark-dashboard -
// "Results page collapses columns that do not vary".

export interface ColumnSpec<Row, K extends string = string> {
  key: K;
  label: string;
  value: (row: Row) => string;
}

export interface Collapsed<K extends string> {
  /** Columns whose values differ across rows (shown in the table). */
  varying: K[];
  /** Columns with one value across all rows, with that value. */
  shared: { key: K; label: string; value: string }[];
}

export function collapseColumns<Row, K extends string>(
  rows: readonly Row[],
  columns: readonly ColumnSpec<Row, K>[],
): Collapsed<K> {
  if (rows.length === 0) {
    return { varying: columns.map((c) => c.key), shared: [] };
  }
  const varying: K[] = [];
  const shared: Collapsed<K>["shared"] = [];
  for (const column of columns) {
    const first = column.value(rows[0] as Row);
    const constant = rows.every((row) => column.value(row) === first);
    if (constant) {
      shared.push({ key: column.key, label: column.label, value: first });
    } else {
      varying.push(column.key);
    }
  }
  return { varying, shared };
}
