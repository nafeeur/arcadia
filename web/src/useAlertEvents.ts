// Alert event subscription. **Global, not per-KB** — the topbar badge spans KBs, and system-level alerts have no KB at all.
//
// The event the server pushes carries no data and checks no permissions (see alerts_routes::stream): on receipt we just refetch,
// and the list query decides what each viewer can see. So this doesn't need to know the current KB either.
import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";

export function useAlertEvents() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const es = new EventSource("/api/v1/alerts/events");
    es.addEventListener("alert", () => {
      queryClient.invalidateQueries({ queryKey: ["alerts"] });
    });
    return () => es.close();
  }, [queryClient]);
}
