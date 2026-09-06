// Alert event subscription. **Global, not per-KB** — the header badge spans knowledge bases, and system-level alerts have no KB at all.
//
// The server push carries no data and checks no permissions (see alerts_routes::stream):
// on receipt we just refetch, and the list query alone decides what's visible. So this
// doesn't need to know which KB is current either.
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
