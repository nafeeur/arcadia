import { request } from "./api";
export interface Summary {
  documents: number;
  facts: number;
  pending_changes: number;
  traces: number;
  failed_documents: number;
  historical_chunks: number;
}
export interface Evidence {
  n: number;
  document_id?: string;
  chunk_id?: string;
  filename: string;
  text?: string;
  excerpt?: string;
  proposed?: boolean;
}
export interface Trace {
  id: string;
  question: string;
  answer: string;
  created_at: string;
  source_count: number;
  evidence: Evidence[];
  tool_exchange: unknown[];
  metadata: { model?: string; redacted?: boolean };
}
export interface Change {
  id: string;
  document_id: string;
  title: string;
  reason: string;
  content: string;
  before: string;
  filename: string;
  status: "pending" | "approved" | "rejected";
  stale: boolean;
  created_at: string;
  decision_note?: string;
}
export interface Impact {
  facts: {
    id: string;
    subject: string;
    predicate: string;
    object: string;
    confidence: number;
  }[];
  derived_count: number;
  answers: { id: string; question: string; created_at: string }[];
}
export interface ReplayResult {
  as_of?: string;
  id: string;
  answer: string;
  evidence: Evidence[];
  created_at?: string;
  metadata: {
    model?: string;
    change_id?: string;
    answer_changed: boolean;
    validation: { invalid_citations: number[]; has_citations: boolean };
  };
}
const base = (kb: string) => `/api/v1/kbs/${kb}/arcadia`;
const post = (body: unknown) => ({
  method: "POST",
  body: JSON.stringify(body),
});
export const arc = {
  summary: (kb: string) => request<Summary>(`${base(kb)}/overview`),
  changes: (kb: string) =>
    request<{ changes: Change[] }>(`${base(kb)}/changes`),
  change: (kb: string, id: string) =>
    request<Change>(`${base(kb)}/changes/${id}`),
  propose: (
    kb: string,
    body: {
      document_id: string;
      title: string;
      reason: string;
      content: string;
    },
  ) => request<{ id: string }>(`${base(kb)}/changes`, post(body)),
  decide: (kb: string, id: string, approve: boolean, note: string) =>
    request(`${base(kb)}/changes/${id}/decide`, post({ approve, note })),
  impact: (kb: string, doc: string) =>
    request<Impact>(`${base(kb)}/impact/${doc}`),
  traces: (kb: string, offset = 0) =>
    request<{ traces: Trace[] }>(`${base(kb)}/traces?offset=${offset}`),
  trace: (kb: string, id: string) =>
    request<{ trace: Trace; replays: ReplayResult[] }>(
      `${base(kb)}/traces/${id}`,
    ),
  replay: (kb: string, id: string, as_of?: string, change_id?: string) =>
    request<ReplayResult>(
      `${base(kb)}/traces/${id}/replay`,
      post({ as_of, change_id }),
    ),
  search: (kb: string, q: string, as_of?: string) =>
    request<{
      results: {
        id: string;
        document_id: string;
        filename: string;
        text: string;
      }[];
    }>(`/api/v1/kbs/${kb}/search`, post({ q, as_of, top_k: 20 })),
};
