/**
 * Source kinds — **this front-end list is written exactly once, here.**
 *
 * The backend counterpart is the `SourceKind` enum in `crates/utopia-core`; creation
 * validation and sync dispatch both derive from it. `utopia-store`'s tests read this
 * file and reconcile the two — one extra or missing kind on either side and `cargo test`
 * goes red. Previously both sides were hand-written separately; five connectors got a
 * sync branch and a UI entry but never made the creation allowlist: selectable, but
 * couldn't be created (#247).
 *
 * Order here is the order shown in the create-source dialog.
 */
export const CREATABLE_SOURCE_KINDS = [
  "folder",
  "url",
  "rss",
  "github_issues",
  "jira_issues",
  "s3",
  "azure_blob",
  "gcs",
  "webdav",
  "notion",
  "api",
  "custom",
] as const;

export type CreatableSourceKind = (typeof CREATABLE_SOURCE_KINDS)[number];

/** Two more kinds exist in a KB that users can't create: every KB's built-in `memory`, and legacy `upload` */
export type SourceKind = CreatableSourceKind | "memory" | "upload";
