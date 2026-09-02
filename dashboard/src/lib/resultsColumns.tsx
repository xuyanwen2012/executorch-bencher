import type { ReactNode } from "react";
import type { components } from "../api/client";
import { AbsentDash } from "../components/State";
import { DeviceClassBadge } from "../components/Status";
import type { ColumnSpec } from "./collapse";
import { modelLabel } from "./format";
import { parseModelName } from "./model";

export type ResultRow = components["schemas"]["ResultRowResponse"];

/** Platform-specific dimensions read as "—" on the other platform, so a
 * column that is constant across every visible row still collapses. */
export const ABSENT_GLYPH = "—";
const orAbsent = (v: string | number | null | undefined) => (v === null || v === undefined ? ABSENT_GLYPH : String(v));

const NOT_APPLICABLE = "Not applicable on this platform";

export interface ResultColumn extends ColumnSpec<ResultRow> {
  /** Rendered cell; defaults to the collapsing value in mono. */
  render?: (row: ResultRow) => ReactNode;
  align?: "right";
  /** Constrains a column that can carry a very long value. */
  width?: string;
}

/** The configuration-key columns of the results table, in display order. */
export const KEY_COLUMNS: readonly ResultColumn[] = [
  {
    key: "platform",
    label: "Platform",
    value: (r) => r.platform,
    render: (r) => <span className="text-ink-2">{r.platform}</span>,
  },
  {
    key: "model",
    label: "Model",
    value: (r) => modelLabel(r.model_asset.original_name),
    width: "w-[7.5rem] max-w-[7.5rem]",
    render: (r) => {
      const model = parseModelName(r.model_asset.original_name);
      return (
        <span className="block" title={model.label}>
          <span className="clip font-medium text-ink">{model.identity}</span>
          {model.qualifiers ? <span className="clip font-mono text-[11px] text-ink-3">{model.qualifiers}</span> : null}
        </span>
      );
    },
  },
  {
    key: "class",
    label: "Class",
    value: (r) => r.device_class,
    render: (r) => <DeviceClassBadge deviceClass={r.device_class} />,
  },
  {
    key: "device",
    label: "Host",
    value: (r) => r.device_serial,
    render: (r) => (
      <span className="block">
        <span className="block text-ink">{r.device_serial}</span>
        {r.device_model ? <span className="block text-[11px] text-ink-3">{r.device_model}</span> : null}
      </span>
    ),
  },
  {
    key: "accelerator",
    label: "Accelerator",
    value: (r) => orAbsent(r.host_accelerator),
    width: "w-[8rem] max-w-[8rem]",
    render: (r) =>
      r.host_accelerator ? (
        <span className="clip" title={r.host_accelerator}>
          {r.host_accelerator}
        </span>
      ) : (
        <AbsentDash title={NOT_APPLICABLE} />
      ),
  },
  {
    key: "driver",
    label: "SUMD driver",
    value: (r) => orAbsent(r.sumd_driver_version),
    render: (r) => r.sumd_driver_version ?? <AbsentDash title={NOT_APPLICABLE} />,
  },
  {
    key: "bsp",
    label: "BSP",
    value: (r) => orAbsent(r.bsp_version),
    render: (r) => r.bsp_version ?? <AbsentDash title={NOT_APPLICABLE} />,
  },
  { key: "gpu", label: "GPU MHz", value: (r) => orAbsent(r.gpu_clock_mhz), align: "right" },
  { key: "mif", label: "MIF MHz", value: (r) => orAbsent(r.mif_clock_mhz), align: "right" },
  { key: "int", label: "INT MHz", value: (r) => orAbsent(r.int_clock_mhz), align: "right" },
  { key: "tokens", label: "Input tok", value: (r) => String(r.input_token_count), align: "right" },
];

/** The cell for `column`: its renderer, or its collapsing value with the
 * absent glyph turned into a labelled marker for assistive technology. */
export function renderCell(column: ResultColumn, row: ResultRow): ReactNode {
  if (column.render) return column.render(row);
  const value = column.value(row);
  return value === ABSENT_GLYPH ? <AbsentDash title={NOT_APPLICABLE} /> : value;
}
