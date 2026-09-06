// Current workspace/knowledge-base context: both switchable and remembered in localStorage; a workspace with no KB auto-creates "General".
import { useCallback, useSyncExternalStore } from "react";
import { useLocation, useNavigate, useParams } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, DEFAULT_ONTOLOGY_PACKS, type Kb, type Workspace } from "./api";
import { kbStore, wsStore } from "./wsStore";

/** The KB id from the current path. **Every page lives under /kb/$kbId, so read it straight from the path** —
 *  no need to wait for the KB list to load; whatever the link says, goes. Outside that scope (account pages, etc.)
 *  fall back to the remembered one. */
export function useKbId(): string {
  const params = useParams({ strict: false }) as { kbId?: string };
  const { kb } = useKb();
  return params.kbId ?? kb?.id ?? "";
}

/** Switching KB lands on the same page, but **doesn't carry along what's in the page**.
 *
 *  It used to just replace the kbId in the path wholesale, so `/kb/A/chat/someConversation` became
 *  `/kb/B/chat/someConversation` — the conversation belongs to A, the page asks B for it, gets a 404,
 *  and falls back to a new conversation. The single-slot liveAnswer used to paper over this step (it
 *  claimed a conversation without asking who owns it); once keyed by KB (#259), the claim correctly
 *  failed, and the 404 surfaced (#261).
 *
 *  So only the first segment after kbId is kept: chat, graph, library... a conversation id or document
 *  id one level deeper belongs to that KB, and switching KB should drop it. The document page is itself
 *  a single document, so after switching it lands on the new KB's Library. Outside /kb scope, behavior
 *  is unchanged. */
export function samePageInKb(pathname: string, fromKbId: string, toKbId: string): string {
  const prefix = `/kb/${fromKbId}`;
  if (!pathname.startsWith(prefix)) return pathname.replace(fromKbId, toKbId);
  const section = pathname.slice(prefix.length).split("/").filter(Boolean)[0] ?? "graph";
  return `/kb/${toKbId}/${section === "doc" ? "library" : section}`;
}

export function useKb(): {
  kb: Kb | null;
  kbs: Kb[];
  workspace: Workspace | null;
  workspaces: Workspace[];
  setWorkspace: (id: string) => void;
  setKb: (id: string) => void;
} {
  const queryClient = useQueryClient();
  const selectedId = useSyncExternalStore(wsStore.subscribe, wsStore.get);
  const selectedKbId = useSyncExternalStore(kbStore.subscribe, kbStore.get);

  const workspaces = useQuery({ queryKey: ["workspaces"], queryFn: api.workspaces });
  const list = workspaces.data ?? [];
  const ws = list.find((w) => w.id === selectedId) ?? list[0] ?? null;

  const kbs = useQuery({
    queryKey: ["kbs", ws?.id],
    queryFn: async () => {
      const existing = await api.kbs(ws!.id);
      if (existing.length > 0) return existing;
      // An empty workspace auto-creates General — creating a KB is now an admin action, a non-admin
      // gets 403: silently wait for an admin to create it (in practice the first user is admin, so
      // General is always there).
      // **Ships with default ontology packs.** This used to not pass packs, so a fresh deployment's
      // first KB had zero classes, and its first batch of documents came out as entirely uncategorized
      // entities; the "default schema.org" in the create-KB dialog only took effect for the second KB.
      // The first KB is exactly the one most people ever use.
      try {
        const created = await api.createKb(ws!.id, {
          name: "General",
          ontology_packs: DEFAULT_ONTOLOGY_PACKS,
        });
        queryClient.invalidateQueries({ queryKey: ["kbs", ws!.id] });
        return [created];
      } catch {
        return [];
      }
    },
    enabled: !!ws,
  });

  const kbList = kbs.data ?? [];
  /* **If the URL has one, the URL wins**: the two answer different questions — the address bar says
     "what does this link point to", localStorage says "what was I last looking at".
     A link someone shared with me must beat my own memory, otherwise I open it and see my own KB —
     different data, identical-looking UI. */
  const routeParams = useParams({ strict: false }) as { kbId?: string };
  const wantedKbId = routeParams.kbId ?? selectedKbId;
  const kb = kbList.find((k) => k.id === wantedKbId) ?? kbList[0] ?? null;

  /* **Switching KB is a navigation, not just a store write.**
     The "URL wins" rule above is correct, but it costs this: every in-scope page has kbId written into
     its address, so `selectedKbId` never gets a turn. Writing only the store means the value changes and
     the component re-renders, but the computed result is still the same KB — so the header dropdown is
     **entirely dead** under `/kb/$kbId/*`: clicking does nothing, it only takes effect after a refresh
     (the home-page redirect reads the stored value).

     So the navigation is folded into `setKb` itself, instead of expecting every call site to remember to
     also wire up a `navigate` — the two spots that got missed were exactly that (the header dropdown, and
     Chat's scope switcher), while the three call sites that got it right were all "jump to a specific page"
     ones that happened to carry the KB along. A convention that can be forgotten will be forgotten.

     Stay on the current page: switching KB on the ontology page should show that other KB's ontology,
     not bounce you back to the graph. When there's no kbId in the address (account pages, etc.) just
     record it — that scope was never meant to be dragged along; the caller decides where to go. */
  const navigate = useNavigate();
  const pathname = useLocation({ select: (l) => l.pathname });
  const currentKbId = routeParams.kbId;
  const setKb = useCallback(
    (id: string) => {
      kbStore.set(id);
      if (currentKbId && currentKbId !== id) {
        navigate({ to: samePageInKb(pathname, currentKbId, id), replace: false });
      }
    },
    [navigate, pathname, currentKbId],
  );

  return {
    kb,
    kbs: kbList,
    workspace: ws,
    workspaces: list,
    setWorkspace: wsStore.set,
    setKb,
  };
}
