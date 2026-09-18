// The agent state machine (issue #119, #133): owns the Run's phase, the
// Session's status and message list, and the accumulated `reasoning` /
// `content` text. It is deliberately thin — phase + text accumulation + the
// Session's own data only. The discard confirm and the Load alerts live in the
// `app/` assembly layer.
//
// The state is two blocks, one per owner: `session` is the Session's (its
// `none` / `unused` / `used` predicate and its message list), `run` is this
// Run's. They expire on different acts — `begin` / `end` clear the Session,
// a Send clears the run — so one flat interface could not say either.
//
// The machine addresses no transport: the two calls it needs are handed in by
// the composition root (`begin(open)`, `beginSend(imageDataUrl, start)`), so it
// knows no URL and builds no payload.
//
// Generation tracking: the machine's async work belongs to one of two
// lifetimes — the Session (`begin` / `end` replace it) and the Run
// (`beginSend` replaces it) — and each keeps its own counter. A callback
// captures both, so a superseded stream or snapshot is refused and can never
// write into the current state. The Session's counter is separate because the
// `read()` snapshot belongs to the Session and outlives a new run. The stream is
// why the guard exists at all: the frontend keeps the SSE open on interrupt
// (#97), so a lifecycle change can land while a previous stream is in flight.
//
// The AI Session itself is the Agent's (ADR-0021): `begin()` asks the backend
// to begin one under an InputMode, a Send appends to it, and `end()` drops it.
// The machine holds only the session's `unused` / `used` predicate.

import type { Message } from "./api";
import { asProviderError } from "./run";
import type { RunEvent, RunFailure } from "./run";

/** The phase of a Run (`idle` / `running` / `done` / `interrupted`) plus
 * `failed`, which any failure before or during a run lands in — a Load
 * failure, a refusal or a provider failure. */
export type RunPhase = "idle" | "running" | "done" | "interrupted" | "failed";

/** Whether an AI Session is live: `none` (no session), `unused` (created, no
 * Send yet), `used` (a Send has been made — the player's message is part of
 * the Session's history, whether or not the reply ever landed). The `used` half
 * drives the discard confirm and the refresh guard; the InputMode lock follows
 * the Session itself, not this predicate. */
export type SessionState = "none" | "unused" | "used";

/** The Agent's state: the Session's data and this Run's, one block each. */
export interface AgentState {
  session: {
    status: SessionState;
    /** The Session's message list, oldest first; empty until the snapshot
     * lands (and if it never does). It belongs to the Session, not to a Send. */
    messages: Message[];
  };
  run: {
    phase: RunPhase;
    /** Accumulated reasoning stream (light, collapsible in the box). */
    reasoning: string;
    /** Accumulated content stream (normal font, not collapsible). */
    content: string;
    /** The verbatim player message (the `role: user` turn), echoed by the
     * backend. */
    user: string;
    /** Present for the image form: the screenshot the player sent (data URL). */
    userImageUrl?: string;
    /** Set only when `phase === "failed"`: why the run produced no reply. */
    failure?: RunFailure;
  };
}

/** Opens a live Session and reads back its first message list. Two calls, not
 * one: the box shows the Session in between, before the snapshot lands. */
export interface SessionOpener {
  begin(): Promise<void>;
  read(): Promise<Message[]>;
}

/** Starts a Send over the live Session, handing it the two callbacks the run's
 * events arrive on. */
export type SendStarter = (
  onEvent: (e: RunEvent) => void,
  onFailure: (f: RunFailure) => void,
) => void;

export interface AgentMachine {
  /** Opens a live Session through `open` and reads its message list back. */
  begin(open: SessionOpener): Promise<void>;
  /** Starts a Send over the live Session; a no-op when none is. */
  beginSend(imageDataUrl: string | undefined, start: SendStarter): void;
  /** Ends the live Session (New Game / PlayMode switch). */
  end(): void;
  /** The machine's own state; the broadcast state is a copy of it. */
  getState(): Readonly<AgentState>;
  /** Subscribes to state changes; returns an unsubscribe. */
  onState(cb: (state: Readonly<AgentState>) => void): () => void;
}

/** A run with no accumulated text. */
function idleRun(): AgentState["run"] {
  return {
    phase: "idle",
    reasoning: "",
    content: "",
    user: "",
  };
}

/** No Session and no run: the state before `begin` and after `end`. */
function noSession(): AgentState {
  return { session: { status: "none", messages: [] }, run: idleRun() };
}

/** Builds an `AgentMachine`. The AiPlayer addresses no Session by id
 * (ADR-0021); the machine tracks only its `unused` / `used` predicate and its
 * own two generations, one per lifetime, for superseded streams and snapshots. */
export function createAgentMachine(): AgentMachine {
  let state: AgentState = noSession();
  let sessionGeneration = 0; // begin / end bump it: the Session's identity
  let runGeneration = 0; // beginSend bumps it: which run's events are current
  const listeners = new Set<(s: Readonly<AgentState>) => void>();

  const emit = (): void => {
    const snapshot: AgentState = {
      session: { ...state.session },
      run: { ...state.run },
    };
    for (const cb of listeners) cb(snapshot);
  };

  /** Replaces fields of one block, leaving the other where it is. */
  const setSession = (session: Partial<AgentState["session"]>): void => {
    state = { ...state, session: { ...state.session, ...session } };
  };
  const setRun = (run: Partial<AgentState["run"]>): void => {
    state = { ...state, run: { ...state.run, ...run } };
  };

  /** Whether a callback created under `session` / `run` still owns the state:
   * its Session is live and its run is the current one. */
  const isCurrent = (session: number, run: number): boolean =>
    session === sessionGeneration && run === runGeneration;

  const onEvent = (session: number, run: number, e: RunEvent): void => {
    if (!isCurrent(session, run)) return; // a stale stream from a superseded run
    switch (e.kind) {
      case "reasoning":
        setRun({ reasoning: state.run.reasoning + e.text });
        break;
      case "content":
        setRun({ content: state.run.content + e.text });
        break;
      case "user":
        setRun({ user: e.text });
        break;
      case "sse_done":
        setRun({ phase: "done" });
        break;
      case "interrupted":
        // The caller's own act: the reply never lands.
        setRun({ phase: "interrupted" });
        break;
      case "provider_error":
        // A mid-stream provider failure: the same vocabulary as a refusal at
        // delivery, but the Provider's own cause.
        setRun({
          phase: "failed",
          failure: { kind: "provider", error: e.error },
        });
        break;
    }
    emit();
  };

  const onFailure = (session: number, run: number, f: RunFailure): void => {
    if (!isCurrent(session, run)) return;
    setRun({ phase: "failed", failure: f });
    emit();
  };

  return {
    async begin(open) {
      const session = ++sessionGeneration;
      // The old session is being replaced: reset to `none` immediately so any
      // in-flight Run becomes stale.
      state = noSession();
      emit();
      try {
        await open.begin();
      } catch (err) {
        if (session !== sessionGeneration) return; // superseded while beginning
        setRun({
          phase: "failed",
          failure: { kind: "provider", error: asProviderError(err) },
        });
        emit();
        return;
      }
      if (session !== sessionGeneration) return; // superseded while beginning
      setSession({ status: "unused" });
      emit();
      // The snapshot lands on top of a box that is already on screen; a failed
      // read leaves the list empty and never fails the Session. A Send may
      // start while this read is in flight, but it replaces the run, not the
      // Session — the snapshot still belongs to the Session it was read for.
      const messages = await open.read();
      if (session !== sessionGeneration) return; // the Session was replaced
      setSession({ messages });
      emit();
    },
    beginSend(imageDataUrl, start) {
      if (state.session.status === "none") return; // no live session: a no-op
      // Snapshot the pair: an arrow reading the counters would take their value
      // at event time, which is exactly the staleness the guard is asking about.
      const session = sessionGeneration;
      const run = ++runGeneration;
      state = {
        // The player sent it, whatever the reply turns out to be: the Session
        // is used from this moment. An Interrupt only cuts the assistant half.
        // The message list is the Session's: a Send clears this round, not it.
        session: { status: "used", messages: state.session.messages },
        run: {
          phase: "running",
          reasoning: "",
          content: "",
          user: "",
          userImageUrl: imageDataUrl,
        },
      };
      emit();
      start(
        (e) => onEvent(session, run, e),
        (f) => onFailure(session, run, f),
      );
    },
    end() {
      sessionGeneration++; // invalidate any in-flight stream and snapshot
      state = noSession();
      emit();
    },
    getState() {
      return state;
    },
    onState(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
  };
}
