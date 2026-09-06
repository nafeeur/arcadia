/**
 * Source kinds — **this frontend list is written in exactly one place**.
 *
 * The backend's copy is the `SourceKind` enum in `crates/utopia-core`, which drives both
 * creation-time validation and sync-time dispatch; `utopia-store`'s tests read this file
 * and check the two against each other — one side having an extra or missing kind turns
 * `cargo test` red. The two used to be hand-written independently, and five connectors
 * got a sync branch and a spot in the UI without making it into the creation allowlist:
 * selectable, but not actually creatable (#247).
 *
 * Order here is the order shown in the "create source" dialog.
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

/** A KB also has two kinds people can't create: the `memory` every KB comes with, and legacy `upload` data */
export type SourceKind = CreatableSourceKind | "memory" | "upload";
