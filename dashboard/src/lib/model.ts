// Model naming. Registered `.pte` filenames encode the whole workload in
// one string, e.g. `llama3_2-1b_vulkan_8da4w.pte`. The results and runs
// tables read far better when the family and parameter size are separated
// from the backend and quantization, so a reviewer can scan a column of
// models without decoding every character.

export interface ModelName {
  /** The full label (filename without `.pte`); always safe to show. */
  label: string;
  /** Family and parameter size, e.g. `llama3.2 1B`, else the whole label. */
  identity: string;
  /** Backend and quantization, e.g. `vulkan · 8da4w`; empty when unparsed. */
  qualifiers: string;
}

/**
 * `<family>[_<minor>]-<size><b|m>[_<qualifier>...]`, the convention the
 * exporter uses. Anything that does not match keeps its label intact.
 */
const CONVENTION = /^([a-z][a-z\d]*)(?:_(\d+(?:_\d+)*))?-(\d+(?:\.\d+)?)([bm])(?:_(.+))?$/i;

export function parseModelName(originalName: string): ModelName {
  const label = originalName.replace(/\.pte$/i, "");
  const match = CONVENTION.exec(label);
  if (!match) return { label, identity: label, qualifiers: "" };
  const [, family = label, minor, size = "", unit = "", rest] = match;
  const version = minor ? `${family}.${minor.replace(/_/g, ".")}` : family;
  return {
    label,
    identity: `${version} ${size}${unit.toUpperCase()}`,
    qualifiers: rest ? rest.split("_").join(" · ") : "",
  };
}
