/* Page title system: `{domain} | {page}`, brand first (product decision; the
   cost is that truncated tabs share the same prefix).
   Domains: Arcadia (main app) / Arcadia Charter (docs) / Arcadia Persona (account). */
import { useEffect } from "react";

export function usePageTitle(...parts: (string | null | undefined)[]) {
  const title = parts.filter(Boolean).join(" | ");
  useEffect(() => {
    if (title) document.title = title;
  }, [title]);
}
