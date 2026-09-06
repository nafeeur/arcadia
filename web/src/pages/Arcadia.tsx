import { useState, useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Link, useLocation, useNavigate } from "@tanstack/react-router";
import {
  ArrowUpRight,
  ArrowRight,
  FileText,
  GitPullRequest,
  History,
  Search,
  Check,
  X,
  Download,
  Plus,
  ShieldCheck,
  MessageSquare,
} from "lucide-react";
import { arc, type Change, type Evidence, type Trace } from "../arcadia-api";
import { api } from "../api";
import { S } from "../i18n";
import { useKbId } from "../kb";
import { Button, Input, Textarea, NativeSelect } from "../ui";
const A = S.arcadia;
const date = (value: string) => new Date(value).toLocaleString();
function utc(value: string) {
  if (!value) return undefined;
  const d = new Date(`${value}Z`);
  if (Number.isNaN(d.getTime())) throw new Error(A.dateInvalid);
  return d.toISOString();
}
function ErrorBox({ error }: { error: unknown }) {
  return error ? (
    <p className="arc-error" role="alert">
      {error instanceof Error ? error.message : A.error}
    </p>
  ) : null;
}
function Heading({
  title,
  intro,
  children,
}: {
  title: string;
  intro: string;
  children?: React.ReactNode;
}) {
  return (
    <header className="arc-heading">
      <div>
        <p className="arc-eyebrow">{A.edition}</p>
        <h1>{title}</h1>
        <p>{intro}</p>
      </div>
      {children}
    </header>
  );
}
function Status({ status }: { status: Change["status"] }) {
  return (
    <span className={`arc-status arc-status-${status}`}>
      {status === "pending"
        ? A.pendingLabel
        : status === "approved"
          ? A.approved
          : A.rejected}
    </span>
  );
}
function EvidenceList({ evidence }: { evidence: Evidence[] }) {
  return (
    <div className="arc-evidence-list">
      {evidence.length === 0 ? (
        <p className="arc-muted">{A.noEvidence}</p>
      ) : (
        evidence.map((e, i) => (
          <details className="arc-evidence" key={e.chunk_id ?? i}>
            <summary>
              <span className="arc-index">{e.n ?? i + 1}</span>
              <FileText size={15} />
              {e.filename}
            </summary>
            <pre>{e.text ?? e.excerpt}</pre>
          </details>
        ))
      )}
    </div>
  );
}
export function ArcadiaHome() {
  const kb = useKbId();
  const summary = useQuery({
    queryKey: ["arc-summary", kb],
    queryFn: () => arc.summary(kb),
  });
  const changes = useQuery({
    queryKey: ["arc-changes", kb],
    queryFn: () => arc.changes(kb),
  });
  const traces = useQuery({
    queryKey: ["arc-traces", kb, 0],
    queryFn: () => arc.traces(kb),
  });
  const stats = [
    [A.documents, summary.data?.documents],
    [A.facts, summary.data?.facts],
    [A.pending, summary.data?.pending_changes],
    [A.answers, summary.data?.traces],
  ] as const;
  return (
    <div className="arc-page">
      <Heading title={A.tagline} intro={A.intro}>
        <Link
          className="arc-primary-link"
          to="/kb/$kbId/chat"
          params={{ kbId: kb }}
        >
          {A.openChat}
          <ArrowUpRight size={17} />
        </Link>
      </Heading>
      <ErrorBox error={summary.error ?? changes.error ?? traces.error} />
      <div className="arc-stats">
        {stats.map(([label, n], i) => (
          <div key={label}>
            <span className="arc-eyebrow">{label}</span>
            <strong>{n === undefined ? "—" : n.toLocaleString()}</strong>
            <span className="arc-stat-number">0{i + 1}</span>
          </div>
        ))}
      </div>
      <div className="arc-home-grid">
        <section className="arc-panel">
          <div className="arc-section-title">
            <h2>{A.recentChanges}</h2>
            <Link
              className="u-link"
              to="/kb/$kbId/changes"
              params={{ kbId: kb }}
            >
              {A.viewAll}
              <ArrowRight size={14} />
            </Link>
          </div>
          {changes.isPending ? (
            <p className="arc-muted">{A.loading}</p>
          ) : changes.data?.changes.length ? (
            changes.data.changes.slice(0, 5).map((c) => (
              <Link
                key={c.id}
                to="/kb/$kbId/changes"
                params={{ kbId: kb }}
                search={{ item: c.id }}
                className="arc-list-row"
              >
                <span className="arc-row-icon">
                  <GitPullRequest size={18} />
                </span>
                <span>
                  <strong>{c.title}</strong>
                  <small>{c.filename}</small>
                </span>
                <Status status={c.status} />
              </Link>
            ))
          ) : (
            <div className="arc-empty">
              <ShieldCheck size={32} />
              <h3>{A.emptyChanges}</h3>
              <Link
                className="u-link"
                to="/kb/$kbId/changes"
                params={{ kbId: kb }}
              >
                {A.newChange}
                <ArrowRight size={14} />
              </Link>
            </div>
          )}
        </section>
        <section className="arc-panel">
          <div className="arc-section-title">
            <h2>{A.recentAnswers}</h2>
            <History size={18} />
          </div>
          {traces.isPending ? (
            <p>{A.loading}</p>
          ) : traces.data?.traces.length ? (
            traces.data.traces.slice(0, 4).map((t) => (
              <Link
                className="arc-list-row"
                key={t.id}
                to="/kb/$kbId/traces"
                params={{ kbId: kb }}
                search={{ item: t.id }}
              >
                <span>
                  <strong>{t.question}</strong>
                  <small>{date(t.created_at)}</small>
                </span>
                <ArrowUpRight size={16} />
              </Link>
            ))
          ) : (
            <div className="arc-empty">
              <MessageSquare size={32} />
              <h3>{A.emptyAnswers}</h3>
              <p>{A.emptyAnswersBody}</p>
            </div>
          )}
        </section>
      </div>
      <section className="arc-guide">
        <h2>{A.gettingStarted}</h2>
        <div>
          {[
            [A.step1, A.step1Body, "/kb/$kbId/library"],
            [A.step2, A.step2Body, "/kb/$kbId/chat"],
            [A.step3, A.step3Body, "/kb/$kbId/changes"],
          ].map(([title, body, to]) => (
            <Link
              key={title}
              to={to as "/kb/$kbId/library"}
              params={{ kbId: kb }}
            >
              <h3>
                {title}
                <ArrowUpRight size={17} />
              </h3>
              <p>{body}</p>
            </Link>
          ))}
        </div>
      </section>
      <footer className="arc-health">
        <span>
          {A.failed}: <strong>{summary.data?.failed_documents ?? "—"}</strong>
        </span>
        <span>
          {A.retained}:{" "}
          <strong>{summary.data?.historical_chunks ?? "—"}</strong>
        </span>
        <Link to="/kb/$kbId/ontology" params={{ kbId: kb }} className="u-link">
          {A.openOntology}
          <ArrowUpRight size={14} />
        </Link>
      </footer>
    </div>
  );
}

export function ArcadiaChanges() {
  const kb = useKbId();
  const qc = useQueryClient();
  const location = useLocation();
  const navigate = useNavigate();
  const selected = (location.search as { item?: string }).item ?? "";
  const setSelected = (item: string) =>
    navigate({ to: location.pathname, search: { ...location.search, item } });
  const [creating, setCreating] = useState(false);
  const [doc, setDoc] = useState("");
  const [title, setTitle] = useState("");
  const [reason, setReason] = useState("");
  const [content, setContent] = useState("");
  const [docQuery, setDocQuery] = useState("");
  const [note, setNote] = useState("");
  const [notice, setNotice] = useState("");
  const list = useQuery({
    queryKey: ["arc-changes", kb],
    queryFn: () => arc.changes(kb),
  });
  const detail = useQuery({
    queryKey: ["arc-change", kb, selected],
    queryFn: () => arc.change(kb, selected),
    enabled: !!selected,
  });
  const docs = useQuery({
    queryKey: ["arc-docs", kb, docQuery],
    queryFn: () => api.documents(kb, { q: docQuery, limit: 100, offset: 0 }),
    enabled: creating,
  });
  const impact = useQuery({
    queryKey: ["arc-impact", kb, detail.data?.document_id],
    queryFn: () => arc.impact(kb, detail.data!.document_id),
    enabled: !!detail.data,
  });
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ["arc-changes", kb] });
    qc.invalidateQueries({ queryKey: ["arc-change", kb, selected] });
    qc.invalidateQueries({ queryKey: ["arc-summary", kb] });
  };
  const submit = useMutation({
    mutationFn: () =>
      arc.propose(kb, { document_id: doc, title, reason, content }),
    onSuccess: (r) => {
      setSelected(r.id);
      setCreating(false);
      setNotice(A.proposalSaved);
      setTitle("");
      setReason("");
      setContent("");
      refresh();
    },
  });
  const decide = useMutation({
    mutationFn: (approve: boolean) => arc.decide(kb, selected, approve, note),
    onSuccess: (_, approved) => {
      setNotice(approved ? A.queued : A.rejectedNotice);
      refresh();
    },
  });
  const role = useQuery({
    queryKey: ["kbOne", kb],
    queryFn: () => api.kbDetail(kb),
  });
  const canEdit = ["editor", "admin", "owner"].includes(
    role.data?.my_role ?? "",
  );
  const canApprove = ["admin", "owner"].includes(role.data?.my_role ?? "");
  useEffect(() => {
    setCreating(false);
    setDoc("");
    setNotice("");
    setNote("");
  }, [kb]);
  const c = detail.data;
  return (
    <div className="arc-page">
      <Heading title={A.changes} intro={A.changeIntro}>
        <Button
          disabled={!canEdit}
          variant="primary"
          icon={<Plus size={16} />}
          onClick={() => {
            setCreating(!creating);
            setNotice("");
          }}
        >
          {creating ? A.cancel : A.newChange}
        </Button>
      </Heading>
      <p className="arc-muted">{A.reviewHint}</p>
      <ErrorBox
        error={
          list.error ??
          detail.error ??
          docs.error ??
          impact.error ??
          submit.error ??
          decide.error
        }
      />
      {notice && (
        <p className="arc-notice" role="status">
          {notice}
        </p>
      )}
      {creating ? (
        <form
          className="arc-proposal arc-panel"
          onSubmit={(e) => {
            e.preventDefault();
            submit.mutate();
          }}
        >
          <div className="arc-form-grid">
            <label>
              {A.searchDocs}
              <Input
                value={docQuery}
                onChange={(e) => setDocQuery(e.target.value)}
              />
            </label>
            <label>
              {A.document}
              <NativeSelect
                value={doc}
                onChange={(e) => setDoc(e.target.value)}
                required
              >
                <option value="">{A.chooseDocument}</option>
                {docs.data?.docs
                  .filter(
                    (d) =>
                      d.status === "ready" &&
                      !["queued", "extracting"].includes(d.graph_status),
                  )
                  .map((d) => (
                    <option key={d.id} value={d.id}>
                      {d.filename}
                    </option>
                  ))}
              </NativeSelect>
            </label>
            <label>
              {A.title}
              <Input
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                required
                maxLength={160}
              />
            </label>
            <label>
              {A.reason}
              <Input
                value={reason}
                onChange={(e) => setReason(e.target.value)}
                required
                maxLength={10000}
              />
            </label>
          </div>
          <label>
            {A.replacement}
            <Textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              required
              maxLength={500000}
              rows={14}
            />
          </label>
          <Button
            type="submit"
            variant="primary"
            busy={submit.isPending}
            disabled={!doc}
          >
            {A.submit}
          </Button>
        </form>
      ) : (
        <div className="arc-workbench">
          <aside className="arc-panel arc-inbox">
            {list.isPending ? (
              <p>{A.loading}</p>
            ) : !list.data?.changes.length ? (
              <div className="arc-empty">
                <GitPullRequest size={28} />
                <p>{A.emptyChanges}</p>
              </div>
            ) : (
              list.data.changes.map((item) => (
                <Button
                  key={item.id}
                  className={`arc-inbox-item ${selected === item.id ? "is-selected" : ""}`}
                  variant="ghost"
                  onClick={() => {
                    setSelected(item.id);
                    setNotice("");
                    setNote("");
                  }}
                >
                  <span>
                    <strong>{item.title}</strong>
                    <small>{item.filename}</small>
                    <Status status={item.status} />
                  </span>
                </Button>
              ))
            )}
          </aside>
          <section className="arc-panel arc-inspector">
            {detail.isPending && selected ? (
              <p>{A.loading}</p>
            ) : !c ? (
              <div className="arc-empty">
                <FileText size={32} />
                <h3>{A.selectChange}</h3>
              </div>
            ) : (
              <>
                <div className="arc-section-title">
                  <h2>{c.title}</h2>
                  <Status status={c.status} />
                </div>
                <p>{c.reason}</p>
                {c.status === "pending" && c.stale && (
                  <p className="arc-error">{A.stale}</p>
                )}
                <div className="arc-compare">
                  <section>
                    <h3>{A.before}</h3>
                    <pre>{c.before}</pre>
                  </section>
                  <section>
                    <h3>{A.after}</h3>
                    <pre>{c.content}</pre>
                  </section>
                </div>
                <div className="arc-section-title">
                  <h2>{A.impact}</h2>
                  <span>
                    {impact.data?.derived_count ?? "—"} {A.derived}
                  </span>
                </div>
                <p className="arc-muted">{A.impactNote}</p>
                <details className="arc-evidence">
                  <summary>
                    {A.affectedFacts} · {impact.data?.facts.length ?? 0}
                  </summary>
                  <div className="arc-facts">
                    {impact.data?.facts.map((f) => (
                      <p key={f.id}>
                        <strong>{f.subject}</strong>
                        <span>{f.predicate}</span>
                        <strong>{f.object}</strong>
                      </p>
                    ))}
                  </div>
                </details>
                <div className="arc-dependent">
                  {impact.data?.answers.map((t) => (
                    <Link
                      key={t.id}
                      to="/kb/$kbId/traces"
                      params={{ kbId: kb }}
                      search={{
                        item: t.id,
                        change:
                          c.status === "pending" && !c.stale ? c.id : undefined,
                      }}
                      className="arc-list-row"
                    >
                      <span>{t.question}</span>
                      <span>
                        {A.previewChange}
                        <ArrowUpRight size={14} />
                      </span>
                    </Link>
                  ))}
                </div>
                {c.status === "pending" && canApprove && (
                  <div className="arc-decision">
                    <label>
                      {A.decisionNote}
                      <Input
                        value={note}
                        onChange={(e) => setNote(e.target.value)}
                        maxLength={10000}
                      />
                    </label>
                    <div>
                      <Button
                        variant="primary"
                        disabled={c.stale}
                        busy={decide.isPending}
                        icon={<Check size={16} />}
                        onClick={() => decide.mutate(true)}
                      >
                        {A.approve}
                      </Button>
                      <Button
                        icon={<X size={16} />}
                        busy={decide.isPending}
                        onClick={() => decide.mutate(false)}
                      >
                        {A.reject}
                      </Button>
                    </div>
                  </div>
                )}
              </>
            )}
          </section>
        </div>
      )}
    </div>
  );
}

export function ArcadiaTraces() {
  const kb = useKbId();
  const qc = useQueryClient();
  const location = useLocation();
  const navigate = useNavigate();
  const selected = (location.search as { item?: string }).item ?? "";
  const setSelected = (item: string) =>
    navigate({ to: location.pathname, search: { ...location.search, item } });
  const change = (location.search as { change?: string }).change ?? "";
  const setChange = (value: string) =>
    navigate({
      to: location.pathname,
      search: { ...location.search, change: value || undefined },
    });
  const [offset, setOffset] = useState(0);
  const [at, setAt] = useState("");
  const list = useQuery({
    queryKey: ["arc-traces", kb, offset],
    queryFn: () => arc.traces(kb, offset),
  });
  const detail = useQuery({
    queryKey: ["arc-trace", kb, selected],
    queryFn: () => arc.trace(kb, selected),
    enabled: !!selected,
  });
  const changes = useQuery({
    queryKey: ["arc-changes", kb],
    queryFn: () => arc.changes(kb),
  });
  const replay = useMutation({
    mutationFn: () => arc.replay(kb, selected, utc(at), change || undefined),
    onSuccess: () => {
      setRunId("");
      qc.invalidateQueries({ queryKey: ["arc-trace", kb, selected] });
    },
  });
  useEffect(() => {
    setAt("");
    setOffset(0);
    replay.reset();
  }, [kb]);
  const t = detail.data?.trace;
  const [runId, setRunId] = useState("");
  useEffect(() => setRunId(""), [selected]);
  const result =
    detail.data?.replays.find((r) => r.id === runId) ?? detail.data?.replays[0];
  const download = (trace: Trace) => {
    const blob = new Blob([JSON.stringify(detail.data, null, 2)], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `arcadia-trace-${trace.id}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };
  return (
    <div className="arc-page">
      <Heading title={A.traces} intro={A.traceIntro} />
      <p className="arc-muted">{A.privacy}</p>
      <ErrorBox
        error={list.error ?? detail.error ?? replay.error ?? changes.error}
      />
      <div className="arc-workbench">
        <aside className="arc-panel arc-inbox">
          {list.isPending ? (
            <p>{A.loading}</p>
          ) : !list.data?.traces.length ? (
            <div className="arc-empty">
              <History size={28} />
              <h3>{A.emptyAnswers}</h3>
              <p>{A.emptyAnswersBody}</p>
            </div>
          ) : (
            list.data.traces.map((item) => (
              <Button
                key={item.id}
                variant="ghost"
                className={`arc-inbox-item ${selected === item.id ? "is-selected" : ""}`}
                onClick={() => {
                  setSelected(item.id);
                  replay.reset();
                }}
              >
                <span>
                  <strong>{item.question}</strong>
                  <small>{date(item.created_at)}</small>
                  <small>
                    {item.source_count} {A.sources}
                  </small>
                </span>
              </Button>
            ))
          )}
          <div className="arc-pagination">
            <Button
              disabled={offset === 0}
              onClick={() => setOffset(Math.max(0, offset - 50))}
            >
              {A.previous}
            </Button>
            <Button
              disabled={(list.data?.traces.length ?? 0) < 50}
              onClick={() => setOffset(offset + 50)}
            >
              {A.next}
            </Button>
          </div>
        </aside>
        <section className="arc-panel arc-inspector">
          {!t ? (
            <div className="arc-empty">
              <History size={32} />
              <h3>
                {selected && detail.isPending ? A.loading : A.traceSelect}
              </h3>
            </div>
          ) : (
            <>
              <div className="arc-section-title">
                <h2>{t.question}</h2>
                <Button
                  icon={<Download size={15} />}
                  onClick={() => download(t)}
                >
                  {A.export}
                </Button>
              </div>
              <p className="arc-muted">
                {date(t.created_at)} · {A.model}:{" "}
                {t.metadata.model ?? A.notRecorded}
              </p>
              {t.metadata.redacted ? (
                <p className="arc-error">{A.traceRedacted}</p>
              ) : (
                <>
                  {!!detail.data?.replays.length && (
                    <label>
                      {A.savedRuns}
                      <NativeSelect
                        value={result?.id ?? ""}
                        onChange={(e) => setRunId(e.target.value)}
                      >
                        {detail.data?.replays.map((r) => (
                          <option key={r.id} value={r.id}>
                            {r.created_at ? date(r.created_at) : A.notRecorded}{" "}
                            · {r.metadata.model}
                          </option>
                        ))}
                      </NativeSelect>
                    </label>
                  )}
                  <div className="arc-compare">
                    <section>
                      <h3>{A.original}</h3>
                      <pre>{t.answer}</pre>
                    </section>
                    {result && (
                      <section>
                        <h3>{A.comparison}</h3>
                        <pre>{result.answer}</pre>
                        <p className="arc-muted">
                          {result.metadata.answer_changed
                            ? A.changed
                            : A.unchanged}{" "}
                          · {result.metadata.model}
                          <br />
                          {result.as_of
                            ? `${A.asOf}: ${new Date(result.as_of).toISOString()}`
                            : result.metadata.change_id
                              ? A.proposalRun
                              : A.now}
                        </p>
                      </section>
                    )}
                  </div>
                  <div className="arc-replay">
                    <h3>{A.replay}</h3>
                    <p className="arc-muted">{A.replayNote}</p>
                    <div className="arc-form-grid">
                      <label>
                        {A.asOf}
                        <Input
                          type="datetime-local"
                          step="1"
                          value={at}
                          disabled={!!change}
                          onChange={(e) => setAt(e.target.value)}
                        />
                      </label>
                      <label>
                        {A.scenario}
                        <NativeSelect
                          disabled={!!at}
                          value={change}
                          onChange={(e) => setChange(e.target.value)}
                        >
                          <option value="">{A.noScenario}</option>
                          {changes.data?.changes
                            .filter((c) => c.status === "pending" && !c.stale)
                            .map((c) => (
                              <option key={c.id} value={c.id}>
                                {c.title}
                              </option>
                            ))}
                        </NativeSelect>
                      </label>
                    </div>
                    <Button
                      variant="primary"
                      busy={replay.isPending}
                      icon={<History size={16} />}
                      onClick={() => replay.mutate()}
                    >
                      {A.rerun}
                    </Button>
                  </div>
                  {result && (
                    <details className="arc-evidence">
                      <summary>{A.validation}</summary>
                      <p>
                        {A.invalidCitations}:{" "}
                        {result.metadata.validation.invalid_citations.join(
                          ", ",
                        ) || "0"}
                      </p>
                      {!result.metadata.validation.has_citations && (
                        <p>{A.noCitations}</p>
                      )}
                      <p>{A.semanticNote}</p>
                      <EvidenceList evidence={result.evidence} />
                    </details>
                  )}
                  <h3 className="arc-subhead">{A.evidence}</h3>
                  <EvidenceList evidence={t.evidence} />
                  <details className="arc-evidence">
                    <summary>{A.tools}</summary>
                    <pre>{JSON.stringify(t.tool_exchange, null, 2)}</pre>
                  </details>
                </>
              )}
            </>
          )}
        </section>
      </div>
    </div>
  );
}
export function ArcadiaHistory() {
  const kb = useKbId();
  const [q, setQ] = useState("");
  const [at, setAt] = useState("");
  const search = useMutation({ mutationFn: () => arc.search(kb, q, utc(at)) });
  useEffect(() => {
    search.reset();
  }, [kb]);
  return (
    <div className="arc-page">
      <Heading title={A.history} intro={A.historyIntro} />
      <form
        className="arc-search-form arc-panel"
        onSubmit={(e) => {
          e.preventDefault();
          search.mutate();
        }}
      >
        <label>
          {A.query}
          <Input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            required
            maxLength={2000}
          />
        </label>
        <label>
          {A.asOf}
          <Input
            type="datetime-local"
            step="1"
            value={at}
            onChange={(e) => setAt(e.target.value)}
          />
        </label>
        <Button
          type="submit"
          variant="primary"
          busy={search.isPending}
          icon={<Search size={16} />}
          disabled={!q.trim()}
        >
          {A.search}
        </Button>
      </form>
      <p className="arc-muted">{A.searchHint}</p>
      <ErrorBox error={search.error} />
      <div className="arc-search-results">
        {search.data && (
          <h2>
            {A.matches} / {search.data.results.length}
          </h2>
        )}
        {search.data?.results.length === 0 && (
          <div className="arc-empty arc-panel">
            <Search size={32} />
            <p>{A.noResults}</p>
          </div>
        )}
        {search.data?.results.map((r, i) => (
          <article className="arc-panel arc-result" key={r.id}>
            <div className="arc-section-title">
              <h3>
                <span className="arc-index">
                  {String(i + 1).padStart(2, "0")}
                </span>
                {r.filename}
              </h3>
            </div>
            <pre>{r.text}</pre>
          </article>
        ))}
      </div>
    </div>
  );
}
