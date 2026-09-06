import { useEffect, useMemo, useSyncExternalStore } from "react";
import { noteEditingApi, type NoteDraft, type NoteEditingSnapshot, type NoteSaveReason, type SaveNoteRequest } from "../api/noteEditing";

export type NoteSessionState = {
  base: NoteEditingSnapshot | null;
  title: string;
  content: string;
  generation: number;
  phase: "loading" | "idle" | "saving" | "error" | "conflict";
  error: string;
  checkpointGeneration: number;
  conflict: NoteEditingSnapshot | null;
};

export class NoteEditSession {
  readonly sessionId = crypto.randomUUID();
  private listeners = new Set<() => void>();
  private state: NoteSessionState = { base: null, title: "", content: "", generation: 0, phase: "loading", error: "", checkpointGeneration: -1, conflict: null };
  private loading: Promise<void> | null = null;
  private writing: Promise<void> | null = null;
  private checkpointing: Promise<void> = Promise.resolve();
  private pending: SaveNoteRequest | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private draftTimer: ReturnType<typeof setTimeout> | null = null;
  private maxTimer: ReturnType<typeof setTimeout> | null = null;
  private maxSaveTimer: ReturnType<typeof setTimeout> | null = null;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private retries = 0;
  private composing = 0;
  pendingAssets = 0;
  editorStateJson: Record<string, unknown> | null = null;
  editorScrollTop = 0;
  private autosave = true;
  private delay = 700;
  views = 0;
  lastUsed = Date.now();
  constructor(readonly noteId: string, private api = noteEditingApi) {}
  snapshot = () => this.state;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  private publish(update: Partial<NoteSessionState>) {
    this.state = { ...this.state, ...update };
    this.lastUsed = Date.now();
    this.listeners.forEach((listener) => listener());
  }
  get dirty() {
    const { base, title, content } = this.state;
    return !!base && (title !== base.note.title || content !== base.note.content || base.externalContent !== null);
  }
  load = (): Promise<void> => {
    if (this.loading) return this.loading;
    if (this.writing) return this.writing.then(() => this.load());
    const generation = this.state.generation;
    this.loading = this.api.load(this.noteId).then((base) => {
      if (this.state.base && (this.dirty || this.state.generation !== generation)) {
        this.publish({ base: { ...this.state.base, drafts: base.drafts, stagedImages: base.stagedImages } });
        if (base.noteVersion !== this.state.base.noteVersion || base.diskHash !== this.state.base.diskHash) {
          this.publish({ phase: "conflict", conflict: base, error: "文件已变化，本地草稿已保留。" });
        }
        return;
      }
      this.publish({ base, title: base.note.title, content: base.note.content,
        phase: base.externalContent !== null ? "conflict" : base.sourceMissing ? "error" : "idle",
        conflict: base.externalContent !== null ? base : null,
        error: base.sourceMissing ? "NOTE_SOURCE_MISSING: 笔记文件不可用。" : "" });
    }).catch((error: unknown) => { this.publish({ phase: "error", error: String(error) }); })
      .finally(() => { this.loading = null; });
    return this.loading;
  };
  configure(autosave: boolean, delay: number) {
    this.autosave = autosave;
    this.delay = delay;
    this.releaseTimers();
    if (this.dirty) this.schedule();
  }
  composition(active: boolean) {
    this.composing = Math.max(0, this.composing + (active ? 1 : -1));
    if (!this.composing && this.dirty) this.schedule();
  }
  edit = (update: { title?: string; content?: string }) => {
    if (!this.state.base) return;
    if ((update.title === undefined || update.title === this.state.title) && (update.content === undefined || update.content === this.state.content)) return;
    this.publish({ ...update, generation: this.state.generation + 1 });
    this.schedule();
  };
  private schedule() {
    if (this.timer) clearTimeout(this.timer);
    if (this.draftTimer) clearTimeout(this.draftTimer);
    this.draftTimer = setTimeout(() => { void this.checkpoint().catch(() => undefined); }, 500);
    this.timer = setTimeout(() => { if (this.autosave) void this.save("typing").catch(() => undefined); }, this.delay);
    this.maxTimer ??= setTimeout(() => {
      this.maxTimer = null;
      void this.checkpoint().catch(() => undefined);
    }, 2000);
    if (this.autosave) this.maxSaveTimer ??= setTimeout(() => {
      this.maxSaveTimer = null;
      void this.save("typing").catch(() => undefined);
    }, Math.max(2000, this.delay));
  }
  checkpoint = async () => {
    if (!this.dirty || this.composing || !this.state.base) return;
    const { title, content, generation, base } = this.state;
    const draft: NoteDraft = { noteId: this.noteId, sessionId: this.sessionId, generation,
      baseVersion: base.noteVersion, title, content, updatedAt: 0 };
    const operation = this.checkpointing.catch(() => undefined).then(() => this.api.checkpoint(draft)).then(() => {
      this.publish({ checkpointGeneration: Math.max(this.state.checkpointGeneration, generation) });
    }).catch((error: unknown) => {
      this.publish({ phase: this.state.phase === "conflict" ? "conflict" : "error", error: String(error) });
      throw error;
    });
    this.checkpointing = operation;
    return operation;
  };
  save = async (reason: NoteSaveReason = "explicitSave"): Promise<void> => {
    if (this.composing) throw new Error("请先完成当前输入。");
    if (this.writing) { await this.writing; if (this.dirty) return this.save(reason); return; }
    if (!this.state.base) throw new Error(this.state.error || "笔记尚未加载。");
    if (this.state.phase === "conflict") throw new Error("NOTE_VERSION_CONFLICT: 请先处理版本冲突。");
    if (!this.dirty && !this.pending) return;
    this.releaseTimers();
    // Freeze before any asynchronous checkpoint. Input and IME may resume
    // while the filesystem is busy; they belong to the next generation.
    const base = this.state.base;
    const request = this.pending ?? {
      noteId: this.noteId, sessionId: this.sessionId, operationId: crypto.randomUUID(),
      draftGeneration: this.state.generation, expectedNoteVersion: base.noteVersion,
      expectedContentHash: base.contentHash, expectedDiskHash: base.diskHash,
      title: this.state.title.trim() || "未命名笔记", markdown: this.state.content,
      acceptExternalChange: base.externalContent !== null, reason,
    };
    const run = async () => {
      await this.checkpoint();
      await this.checkpointing;
      this.pending = request;
      this.publish({ phase: "saving", error: "" });
      try {
        const receipt = await this.api.save(request);
        if (receipt.operationId !== request.operationId || receipt.noteId !== this.noteId || receipt.draftGeneration !== request.draftGeneration) {
          throw new Error("NOTE_OPERATION_MISMATCH: 保存回执不匹配。");
        }
        const note = { ...base.note, title: receipt.title, content: receipt.committedMarkdown, updatedAt: receipt.updatedAt };
        this.pending = null;
        this.retries = 0;
        this.publish({ base: { ...base, note, noteVersion: receipt.noteVersion, contentHash: receipt.contentHash,
          diskHash: receipt.contentHash, externalContent: null, sourceMissing: false },
          ...(this.state.generation === receipt.draftGeneration ? { title: note.title, content: note.content } : {}),
          phase: "idle", error: "" });
      } catch (error: unknown) {
        const code = /NOTE_[A-Z_]+/.exec(String(error))?.[0];
        const conflict = code === "NOTE_VERSION_CONFLICT";
        if (conflict || code?.startsWith("NOTE_CONTENT_") || code === "NOTE_OPERATION_MISMATCH") this.pending = null;
        this.publish({ phase: conflict ? "conflict" : "error", error: String(error) });
        if (conflict) this.publish({ conflict: await this.api.load(this.noteId).catch(() => null) });
        throw error;
      }
    };
    this.writing = run();
    try { await this.writing; } catch (error) {
      if (this.autosave && /NOTE_STORAGE_UNAVAILABLE/.test(String(error)) && this.retries < 3) {
        this.retryTimer = setTimeout(() => { this.retryTimer = null; void this.save(reason).catch(() => undefined); }, 1000 * 2 ** this.retries++);
      }
      throw error;
    } finally { this.writing = null; }
    if (this.dirty && this.autosave) this.schedule();
  };
  resolve = async (choice: "local" | "disk", observed: NoteEditingSnapshot) => {
    const latest = await this.api.load(this.noteId);
    if (latest.noteVersion !== observed.noteVersion || latest.diskHash !== observed.diskHash) {
      this.publish({ conflict: latest }); throw new Error("文件再次变化，请重新比较。");
    }
    this.pending = null;
    const content = choice === "disk" ? latest.externalContent ?? latest.note.content : this.state.content;
    this.publish({ base: latest, content, title: choice === "disk" ? latest.note.title : this.state.title,
      generation: this.state.generation + 1, phase: "idle", conflict: null, error: "" });
    // An external file equal to the selected content still needs a DB revision.
    if (latest.externalContent !== null && !this.dirty) this.edit({ content });
    await this.save("restore");
  };
  recover = async (draft: NoteDraft) => {
    this.edit({ title: draft.title, content: draft.content });
    await this.checkpoint();
    await this.discard(draft);
  };
  discard = async (draft: NoteDraft) => {
    await this.api.discard(draft);
    if (this.state.base) this.publish({ base: { ...this.state.base, drafts: this.state.base.drafts.filter((entry) => entry.sessionId !== draft.sessionId) } });
  };
  releaseTimers() {
    if (this.timer) clearTimeout(this.timer);
    if (this.draftTimer) clearTimeout(this.draftTimer);
    if (this.maxTimer) clearTimeout(this.maxTimer);
    if (this.maxSaveTimer) clearTimeout(this.maxSaveTimer);
    if (this.retryTimer) clearTimeout(this.retryTimer);
    this.timer = this.draftTimer = this.maxTimer = this.maxSaveTimer = null;
    this.retryTimer = null;
  }
  get isComposing() { return this.composing > 0; }
  get isWriting() { return this.writing !== null || this.pending !== null; }
}

const sessions = new Map<string, NoteEditSession>();
export function getNoteEditSession(noteId: string) {
  let session = sessions.get(noteId);
  if (!session) { session = new NoteEditSession(noteId); sessions.set(noteId, session); }
  return session;
}
const EMPTY: NoteSessionState = { base: null, title: "", content: "", generation: 0, phase: "idle", error: "", checkpointGeneration: -1, conflict: null };
const subscribeEmpty = () => () => undefined;
export function useNoteEditSession(noteId: string | null) {
  const session = useMemo(() => noteId ? getNoteEditSession(noteId) : null, [noteId]);
  const state = useSyncExternalStore(session?.subscribe ?? subscribeEmpty, session?.snapshot ?? (() => EMPTY));
  useEffect(() => {
    if (!session) return;
    session.views++;
    if (!session.snapshot().base) void session.load();
    const reconcile = () => { if (!session.snapshot().phase.includes("saving")) void session.load(); };
    window.addEventListener("focus", reconcile);
    return () => {
      window.removeEventListener("focus", reconcile);
      session.views--;
      void session.checkpoint().catch(() => undefined);
      pruneNoteEditSessions();
    };
  }, [session]);
  return { session, ...state };
}

export function pruneNoteEditSessions() {
  let bytes = 0;
  const inactive = [...sessions.values()].filter((item) => !item.views).sort((a, b) => b.lastUsed - a.lastUsed);
  inactive.forEach((item, index) => {
    bytes += item.editorStateJson ? JSON.stringify(item.editorStateJson).length * 2 : item.snapshot().content.length * 2;
    if (index < 3 && bytes <= 64 * 1024 * 1024) return;
    // Dirty text remains in the durable session, but its detached undo DOM
    // snapshot need not occupy memory indefinitely.
    item.editorStateJson = null;
    if (!item.dirty && !item.isWriting) { item.releaseTimers(); sessions.delete(item.noteId); }
  });
}

export async function flushNoteEditors() {
  await Promise.all([...sessions.values()].map(async (session) => {
    if (session.isComposing) throw new Error("请先完成当前输入，再关闭窗口。");
    if (session.pendingAssets > 0) throw new Error("图片尚在保留，请稍后再关闭窗口。");
    if (!session.dirty && !session.isWriting) return;
    try { await session.save(); } catch { await session.checkpoint(); }
    if (session.dirty && session.snapshot().checkpointGeneration < session.snapshot().generation) throw new Error("笔记草稿尚未保留，不能关闭窗口。");
  }));
}
