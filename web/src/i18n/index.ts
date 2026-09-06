// Arcadia ships an English-only interface. `S` is the single string bundle
// consumed by every page; see docs/decisions/0004 for the original rationale.
//
// `LANG_NAMES` and the `Lang` union are also used by the per-knowledge-base
// ontology/corpus language selector (Settings.tsx, KbSettings.tsx) — that
// setting picks the language of the *documents being ingested*, not the UI,
// so it stays independent of interface localization.
import { en, type Strings } from "./en";

export type { Strings };

export type Lang = "en" | "zh";

/** Corpus/ontology language labels, in their own language. */
export const LANG_NAMES: Record<Lang, string> = { en: "English", zh: "中文" };

export const S: Strings = en;
