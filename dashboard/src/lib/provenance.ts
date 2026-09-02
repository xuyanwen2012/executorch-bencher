// Provenance details the importer stashes inside a run's free-form
// `input_parameters` object. They are the answer to "what exactly was
// built?" when the working tree was dirty, so the run detail view lifts
// them out of the JSON blob and shows them beside the git fields.
// See `src/bin/import_observer_log.rs`, which writes these keys.

/** Names of the files that were modified when a dirty build was measured. */
export function modifiedFiles(inputParameters: unknown): string[] | null {
  if (typeof inputParameters !== "object" || inputParameters === null) return null;
  const value = (inputParameters as Record<string, unknown>)["git_modified_files"];
  if (!Array.isArray(value)) return null;
  const files = value.filter((entry): entry is string => typeof entry === "string");
  return files.length > 0 ? files : null;
}

/** The importer's free-text note about the commit, when it recorded one. */
export function gitNotes(inputParameters: unknown): string | null {
  if (typeof inputParameters !== "object" || inputParameters === null) return null;
  const value = (inputParameters as Record<string, unknown>)["git_notes"];
  return typeof value === "string" && value !== "" ? value : null;
}
