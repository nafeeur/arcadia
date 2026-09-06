/* Page title system: `{domain} | {page}`, brand first (product decision; the cost is identical prefixes once tabs get truncated).
   Domains: Utopia (main app) / Utopia Charter (docs) / Utopia Persona (account). */
import { useEffect } from "react";

export function usePageTitle(...parts: (string | null | undefined)[]) {
  const title = parts.filter(Boolean).join(" | ");
  useEffect(() => {
    if (title) document.title = title;
  }, [title]);
}
