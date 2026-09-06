// Header-bar alerts (0005): bell + unread badge + popover panel.
//
// **A popover, not a page**: alerts are something you glance at in passing,
// not a place you go to deliberately. Making it a page would force people to
// leave what they're doing, and the cost of that is that nobody looks.
//
// One alert = one incident, written once and never changed — there is no "resolved".
// "Read" is per-person — one person reading it shouldn't clear it from anyone else's unread count.
import { type Ref, useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bell, Search, X } from "lucide-react";

import { api, type AlertGroup } from "../api";
import { S } from "../i18n";
import { toast } from "../toast";
import {
  Button,
  Chip,
  cn,
  IconButton,
  Input,
  LinkButton,
  Pager,
} from "../ui";
import { usePopoverFlip } from "../ui/popoverFlip";

const PAGE = 8;

/** The line shown per detail: object name — raw error text */
function line(d: AlertGroup["lines"][number]): string | null {
  const parts = [d.name ?? d.job, d.error].filter(Boolean);
  return parts.length ? parts.join(" — ") : null;
}

/** Which alert kinds get a "run again" action: the ones where jobs don't come back on their own once the underlying issue (top up credit, fix the endpoint) is fixed */
const REQUEUE_KINDS = new Set(["llm.out_of_credit", "llm.unreachable"]);

function AlertRow({
  g,
  onRead,
  onRequeue,
  requeuing,
}: {
  g: AlertGroup;
  onRead: (g: AlertGroup) => void;
  onRequeue: (g: AlertGroup) => void;
  requeuing: boolean;
}) {
  // An unrecognized kind still has to render: a new alert source can ship
  // before the frontend catches up, and "an alert I don't recognize" beats
  // "nothing shown at all"
  const worded = S.alerts.kinds[g.kind];
  const lines = g.lines.map(line).filter((l): l is string => !!l);
  // `count` is the whole group; `lines` only carries the first few back — the
  // difference is "N more"
  const rest = g.count - lines.length;
  return (
    // div, not button: the row already has an action button inside it, and a button nested in a button is invalid HTML
    <div
      role="button"
      tabIndex={0}
      // **Only a click counts as read**, not a hover. Moving the mouse over a
      // row of alerts doesn't mean they were seen, and once marked read they
      // never come back on their own. One click marks the whole group.
      onClick={() => {
        if (g.unread > 0) onRead(g);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" && g.unread > 0) onRead(g);
      }}
      className="u-row-shell flex w-full cursor-pointer gap-3 border-b border-line px-4 py-3 text-left last:border-b-0"
    >
      {/* Unread is just a red dot. Outlining or shading the whole row would turn
          the panel into a wall of red when there are many alerts, while a dot
          only takes the small amount of space it deserves, and disappears once read */}
      <span
        className={cn(
          "mt-[7px] h-1.5 w-1.5 rounded-full shrink-0",
          g.unread > 0 ? "bg-danger" : "bg-transparent",
        )}
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 flex-wrap">
          <span
            className={cn(
              "text-body",
              g.unread > 0 ? "font-medium text-ink" : "text-ink-2",
            )}
          >
            {worded?.title ?? S.alerts.unknownKind(g.kind)}
          </span>
          {g.count > 1 && <Chip tone="neutral">{g.count}</Chip>}
          <Chip tone={g.kb_name ? "neutral" : "violet"}>
            {g.kb_name ?? S.alerts.system}
          </Chip>
        </div>
        {worded && (
          <p className="mt-1 text-small text-ink-3">{worded.hint}</p>
        )}
        {lines.length > 0 && (
          <ul className="mt-1 space-y-1">
            {lines.map((l, i) => (
              <li key={i} className="text-fine text-ink-2 break-words">
                {l}
              </li>
            ))}
            {rest > 0 && (
              <li className="text-fine text-ink-3">
                {S.alerts.andMore(rest)}
              </li>
            )}
          </ul>
        )}
        {/* Timestamp is the latest occurrence in the group */}
        <p className="u-num mt-2 text-fine text-ink-3">
          {new Date(g.latest_at).toLocaleString()}
        </p>
        {/* Resume once fixed: puts jobs that failed during this incident window back
            on the queue (#216). Running out of credit is the one failure kind where
            someone does a concrete thing (tops up, fixes the endpoint) and then wants
            the work to continue — the action belongs on the alert, closing the loop
            here instead of needing a separate queue page */}
        {REQUEUE_KINDS.has(g.kind) && (
          <Button variant="secondary" size="sm" className="mt-2"
            type="button"
            disabled={requeuing}
            onClick={(e) => {
              e.stopPropagation();
              onRequeue(g);
            }}
          >
            {S.alerts.runAgain}
          </Button>
        )}
      </div>
    </div>
  );
}

function Panel({ panelRef }: { panelRef: Ref<HTMLDivElement> }) {
  const [q, setQ] = useState("");
  const [page, setPage] = useState(0);
  const qc = useQueryClient();

  // Reset to page 1 after a search: staying on page 4 of a result that now
  // only has 2 pages would render the panel blank, and people would read
  // that as "no alerts"
  useEffect(() => {
    setPage(0);
  }, [q]);

  const list = useQuery({
    queryKey: ["alerts", "list", q, page],
    queryFn: () => api.alerts({ q, limit: PAGE, offset: page * PAGE }),
    // Keep the previous page while paging, so the panel height doesn't collapse and then pop back
    placeholderData: (prev) => prev,
  });

  const read = useMutation({
    mutationFn: (g: AlertGroup) =>
      api.alertReadGroup({
        kb_id: g.kb_id,
        kind: g.kind,
        from: g.earliest_at,
        to: g.latest_at,
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["alerts"] }),
  });
  const readAll = useMutation({
    mutationFn: () => api.alertsReadAll(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["alerts"] }),
  });
  // The time window starts at this group's earliest occurrence — failures before that aren't part of this incident
  const requeue = useMutation({
    mutationFn: (g: AlertGroup) =>
      api.requeueJobs(g.kb_id, { failed_since: g.earliest_at }),
    onSuccess: (r) => {
      toast.success(S.alerts.requeued(r.requeued));
      qc.invalidateQueries({ queryKey: ["jobs"] });
    },
    onError: (e) => toast.error(String(e)),
  });

  const groups = list.data?.items ?? [];
  const total = list.data?.total ?? 0;

  return (
    // top-0, not top-9: the panel needs to grow from the bell's **own position**, aligned to the top-right corner
    <div
      ref={panelRef}
      className="u-menu-glass absolute right-0 top-0 w-[420px] rounded-xl shadow-2xl z-50 overflow-hidden"
    >
      <div className="flex items-center gap-2 pl-4 pr-8 py-3 border-b border-line">
        <span className="text-body font-medium text-ink">
          {S.alerts.title}
        </span>
      </div>

      {/* Same treatment as the library's filter box: input-dark + left icon + a clear button on the right when there's a value, Esc to clear */}
      <div className="px-4 py-3 border-b border-line">
        <div className="relative">
          <Search
            size={13}
            className="absolute left-2.5 top-1/2 -translate-y-1/2 text-ink-3 pointer-events-none"
          />
          <Input size="sm" className="w-full pl-8 pr-8"
            placeholder={S.alerts.searchPlaceholder}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setQ("")}
          />
          {q && (
            <IconButton size="sm" label={S.ui.close} className="absolute right-2 top-1/2 -translate-y-1/2"
              onClick={() => setQ("")}
            >
              <X size={12} />
            </IconButton>
          )}
        </div>
      </div>

      <div className="max-h-[420px] overflow-y-auto">
        {groups.length === 0 ? (
          <div className="px-4 py-8 text-center">
            <p className="text-body text-ink-2">
              {q ? S.alerts.noMatch : S.alerts.empty}
            </p>
            {!q && (
              <p className="mt-1 text-small text-ink-3">
                {S.alerts.emptyHint}
              </p>
            )}
          </div>
        ) : (
          groups.map((g) => (
            <AlertRow
              key={`${g.kb_id ?? "system"}|${g.kind}|${g.latest_at}`}
              g={g}
              onRead={(x) => read.mutate(x)}
              onRequeue={(x) => requeue.mutate(x)}
              requeuing={requeue.isPending}
            />
          ))
        )}
      </div>

      {/* Footer: list-wide actions sit next to pagination, farthest from the cursor */}
      {groups.length > 0 && (
        <div className="flex items-center gap-3 px-4 py-2 border-t border-line">
          {groups.some((g) => g.unread > 0) && (
            <LinkButton onClick={() => readAll.mutate()}>
              {S.alerts.markAllRead}
            </LinkButton>
          )}
          <Pager
            className="ml-auto"
            total={total}
            pageSize={PAGE}
            page={page}
            onPage={setPage}
          />
        </div>
      )}
    </div>
  );
}

export function AlertBell() {
  // Same in-place-morph animation as the user menu: the two panels sit right next to each other, so even a slight animation mismatch shows up after a couple of clicks
  const { open, setOpen, close, rootRef, anchorRef, panelRef } =
    usePopoverFlip<HTMLButtonElement, HTMLDivElement>();
  const unread = useQuery({
    queryKey: ["alerts", "unread"],
    queryFn: () => api.alertsUnread(),
    // Push is the primary path; this is just a fallback for when the stream drops
    refetchInterval: 120_000,
  });
  const n = unread.data?.unread ?? 0;

  return (
    <div ref={rootRef} className="relative">
      <IconButton
        size="sm"
        ref={anchorRef}
        label={S.alerts.badgeLabel}
        aria-expanded={open}
        className={cn("relative", open && "bg-surface-2 text-ink")}
        onClick={() => (open ? close() : setOpen(true))}
      >
        <Bell size={15} />
        {/* The badge is also just a dot, not a number. "Something unread" is
            binary — opening it tells you how many. A number would also keep
            climbing with retries, and three digits would distort the bell shape */}
        {n > 0 && (
          <span className="absolute top-1 right-1 h-1.5 w-1.5 rounded-full bg-danger" />
        )}
      </IconButton>
      {open && (
        <>
          <Panel panelRef={panelRef} />
          {/* The close button is the panel's **sibling**, not its child: inside
              the panel, `right-0 top-0` would be relative to the panel's own
              padding box, and u-menu-glass has a 0.667px hairline border (one
              physical pixel at DPR 1.5), so it would always be off by that much.
              Placed here, the positioning ancestor is this div wrapping the bell —
              the same box as the bell, so the alignment is exact by construction.

              The cursor is sitting right at this spot after opening the panel, so
              this has to be "click again to close". Putting "mark all read" here
              instead would make a misclick the default action, and it clears every
              alert in every knowledge base at once */}
          <IconButton
            size="sm"
            label={S.alerts.close}
            className="absolute right-0 top-0 z-[60]"
            onClick={close}
          >
            <X size={15} />
          </IconButton>
        </>
      )}
    </div>
  );
}
