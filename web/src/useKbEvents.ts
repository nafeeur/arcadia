// KB event stream subscription: receiving an event only triggers a react-query
// invalidate-and-refetch (events carry no business data, so this is naturally
// idempotent). EventSource auto-reconnects on disconnect; replaces polling in
// Library/Review.
//
// **Invalidation is coalesced.** Extracting a document fires a graph event for
// every fact it drops — invalidating once per event used to make the graph
// page refetch overview a dozen-plus times in those few seconds, each time
// getting the same graph, with only the last one mattering. So events just
// record the key, wait a brief settle period, then invalidate once: a burst
// of events costs one refetch, and that last refetch is guaranteed to include
// every change that came before it. Idempotence is unchanged — this just
// turns "refresh on every event" into "refresh on the last one".
import { useEffect } from "react";
import { useQueryClient, type QueryKey } from "@tanstack/react-query";

/** Quiet period between a burst of events. Far shorter than the gap between facts landing during extraction, so this delay is imperceptible */
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
    // A finished mapping-exploration run also emits review: the Pending tab needs to refresh along with it
    es.addEventListener("review", () => invalidate(["review", kbId], ["mappings", kbId], ["arc-changes", kbId], ["arc-change", kbId], ["arc-summary", kbId]));
    // A memory turn extracted a fact awaiting confirmation (0015): the confirmation card in the conversation grows in response
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
