// KB event stream subscription: on receipt, only react-query invalidation + refetch happens
// (events carry no business data, so this is naturally idempotent).
// EventSource auto-reconnects on disconnect; replaces the Library/Review polling.
//
// **Invalidation is coalesced.** While a document is being extracted, a `graph` event fires
// for every fact landed; invalidating on each one used to make the graph page refetch the
// overview a dozen times in a few seconds — same graph every time, only the last fetch matters.
// So events now just record the key and, after a short quiet period, invalidate once: a burst
// of events buys exactly one refetch, and that last refetch is guaranteed to include every
// change that preceded it. Idempotency is unchanged — it's just "refresh on every event" turned
// into "refresh on the last one".
import { useEffect } from "react";
import { useQueryClient, type QueryKey } from "@tanstack/react-query";

/** Quiet period between bursts of events. Fact-landing intervals during extraction are far shorter than this, so the delay is imperceptible */
const SETTLE_MS = 300;

export function useKbEvents(kbId: string | undefined) {
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!kbId) return;
    const pending = new Map<string, QueryKey>();
    let timer: ReturnType<typeof setTimeout> | null = null;
    const flush = () => {
      timer = null;
      const keys = [...pending.values()];
      pending.clear();
      for (const key of keys) queryClient.invalidateQueries({ queryKey: key });
    };
    const invalidate = (...keys: QueryKey[]) => {
      for (const key of keys) pending.set(JSON.stringify(key), key);
      if (timer === null) timer = setTimeout(flush, SETTLE_MS);
    };

    const es = new EventSource(`/api/v1/kbs/${kbId}/events`);
    es.addEventListener("document", () => invalidate(["documents", kbId], ["graph"], ["arc-summary", kbId], ["arc-changes", kbId], ["arc-change", kbId], ["arc-impact", kbId], ["arc-traces", kbId], ["arc-trace", kbId]));
    es.addEventListener("graph", () => invalidate(["graph"], ["arc-summary", kbId], ["arc-impact", kbId]));
    // Mapping exploration finishing also emits `review`: the Pending column needs to refresh along with it
    es.addEventListener("review", () => invalidate(["review", kbId], ["mappings", kbId], ["arc-changes", kbId], ["arc-change", kbId], ["arc-summary", kbId]));
    // A memory turn extracted a fact awaiting confirmation (0015): the confirmation card in the conversation grows out of it
    es.addEventListener("pending", () => invalidate(["pending", kbId], ["review", kbId]));
    es.addEventListener("source", () => invalidate(["sources", kbId], ["documents", kbId]));
    return () => {
      es.close();
      // On unmount, flush what's pending instead of dropping it: navigating back must show fresh data
      if (timer !== null) {
        clearTimeout(timer);
        flush();
      }
    };
  }, [kbId, queryClient]);
}
