// Answers still being generated live outside the component.
//
// **Navigate away once and it's gone.** The streaming `turns` used to be Chat's component state, and
// leaving the conversation page unmounts that component: the state is gone, the fetch is still running,
// and its callbacks write into an already-dead component. Coming back remounts the component, which reads
// from the store — but the store only gets that row once generation finishes, so all you see is the
// question you asked. Come back later and it's fine, because by then it's been persisted.
//
// The server-side half (generation doesn't die with the connection) is a separate fix; this half solves
// **whether you can see it when you come back**. Both are needed: the server preserves the answer, this
// preserves the stream.
//
// **Keyed by conversation, not a singleton.** This table used to be a single slot, on the assumption that
// only one answer could ever be in flight at a time. That assumption doesn't hold, and Chat itself is what
// disproves it — switching KB doesn't abort ("switching KB shouldn't kill an answer being written in
// another KB"), starting a new conversation doesn't abort ("starting a new one doesn't mean giving up on
// the last one"), and the send guard is scoped per-conversation (explicitly rejecting a global lock that
// would make sending impossible). Put the three "doesn't abort"s together and two concurrent answers become
// a routinely reachable state, which a single slot can't hold: the second one's start overwrites the slot,
// while the first one's callbacks are still writing into "the last turn of the current slot" — the two
// answers interleave character by character; whichever finishes first prematurely tears down the other's
// stop button, leaving it with no way to be stopped.
//
// So it became a table instead: whoever starts an answer gets a handle, and every read and write is
// addressed by name. `send`'s guard doesn't need to change — it always asked "is this particular answer
// streaming", and now that question is finally only about this particular answer.
import type { ChatStep, Source } from "./api";

export interface Turn {
  role: "user" | "assistant";
  content: string;
  steps?: ChatStep[];
  sources?: Source[];
  error?: string;
}

/** A snapshot entry: plain data, meant for rendering. abort doesn't go into the snapshot — rendering
    shouldn't be able to touch it in passing */
export interface Live {
  kbId: string;
  /** null for a new conversation before the server hands back an id; kbId distinguishes two new
      conversations that have neither gotten an id yet */
  conversationId: string | null;
  turns: Turn[];
  streaming: boolean;
}

interface Slot {
  live: Live;
  abort: () => void;
}

const lives = new Map<string, Slot>();
const listeners = new Set<() => void>();

// The snapshot is replaced wholesale: useSyncExternalStore skips unrelated renders via reference
// equality — **no change to another answer should change this one's rendering**, and this old comment
// only became literally true once things were keyed.
let snapshot: readonly Live[] = [];

function emit() {
  snapshot = [...lives.values()].map((s) => s.live);
  listeners.forEach((l) => l());
}

// A new conversation without an id yet is placeholder-keyed with an internal token; identify remaps it
// to the real id once it arrives.
let pendingSeq = 0;

export interface LiveHandle {
  /** A new conversation gets its id back from the server: remap this entry from its placeholder token
      to the real id */
  identify: (conversationId: string) => void;
  /** Mutate the last turn of this answer (the assistant's turn). Only this changes during generation */
  patchLast: (f: (t: Turn) => Turn) => void;
  /** Ends (normally, on error, or because someone hit stop).
   *
   * **Doesn't clear.** An earlier version did clear, and that version had a nasty bug: navigating away
   * unmounts the component, and "hand the final result back to the component" gets called on a component
   * that's already dead — a no-op. So the store ended up empty, the new component had already claimed this
   * answer earlier and so wouldn't go read the store again, and coming back showed a completely blank
   * conversation — not even the question you'd asked.
   *
   * At that moment this is the only place still holding onto the content, so it's kept: only `streaming`
   * gets set to false. The next `begin` will clear out entries that have finished (see begin), and
   * switching to a different conversation that can't claim this one naturally falls back to reading the
   * store. */
  finish: () => void;
  /** streamChat's abort isn't available until it returns: begin hands out a placeholder first, then
      swaps in the real abort once it has one */
  setAbort: (abort: () => void) => void;
}

export const liveAnswer = {
  /** `useSyncExternalStore` 要求同一个快照对象在没变时保持同一引用 */
  get: (): readonly Live[] => snapshot,
  subscribe: (l: () => void) => {
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  },
  /** 认领「正在看的这一场」。按会话找；kbId 只在两个都还没拿到 id 的新会话
      之间起区分作用。找不到就是这一场不在场——展示回落到库里的历史 */
  entry: (kbId: string | null, conversationId: string | null): Live | null =>
    snapshot.find((e) => e.kbId === kbId && e.conversationId === conversationId) ?? null,
  /** 开一场。同会话追问会替换同 key 的旧条目；同时清掉所有已结束的条目——
   *
   * 清除只在有人发新消息时发生，而被清的会话若再被打开，认领不上、自然去
   * 读库，内容一致（服务端在 done 时已落库）。不清的话这张表无界增长；
   * 进行中的条目永不清——那正是本模块存在的意义。 */
  begin: (
    kbId: string,
    conversationId: string | null,
    turns: Turn[],
    abort: () => void,
  ): LiveHandle => {
    for (const [k, s] of lives) if (!s.live.streaming) lives.delete(k);
    let key = conversationId ?? `__pending__${++pendingSeq}`;
    const slot: Slot = { live: { kbId, conversationId, turns, streaming: true }, abort };
    lives.set(key, slot);
    emit();
    return {
      identify: (id: string) => {
        const current = lives.get(key);
        if (!current) return;
        lives.delete(key);
        key = id;
        current.live = { ...current.live, conversationId: id };
        lives.set(key, current);
        emit();
      },
      patchLast: (f) => {
        const current = lives.get(key);
        if (!current || current.live.turns.length === 0) return;
        const turns = [...current.live.turns];
        turns[turns.length - 1] = f(turns[turns.length - 1]);
        current.live = { ...current.live, turns };
        emit();
      },
      finish: () => {
        const current = lives.get(key);
        if (!current || !current.live.streaming) return;
        current.live = { ...current.live, streaming: false };
        emit();
      },
      setAbort: (a) => {
        const current = lives.get(key);
        if (current) current.abort = a;
      },
    };
  },
  /** 停止按钮专用：abort + finish 正在看的这一场。别的场照常写它们自己的条目 */
  stop: (kbId: string, conversationId: string | null) => {
    for (const s of lives.values()) {
      if (s.live.kbId === kbId && s.live.conversationId === conversationId) {
        s.abort();
        s.live = { ...s.live, streaming: false };
        emit();
        return;
      }
    }
  },
};
