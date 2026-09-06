import { afterEach, describe, expect, it, vi } from "vitest";
import { NoteEditSession } from "./noteEditSession";
import { noteEditingApi, type NoteEditingSnapshot, type SaveNoteRequest } from "../api/noteEditing";
import type { LibraryNote } from "../../library/types";

function fixture() {
  let base: NoteEditingSnapshot = { note: { id: "one", title: "Title", content: "base", updatedAt: 1 } as LibraryNote,
    noteVersion: "1", contentHash: "hash-1", diskHash: "hash-1", sourceMissing: false, externalContent: null, drafts: [] };
  const api = { ...noteEditingApi, load: vi.fn(async () => base), checkpoint: vi.fn(async () => undefined),
    save: vi.fn(async (request: SaveNoteRequest) => {
      base = { ...base, noteVersion: String(Number(base.noteVersion) + 1), note: { ...base.note, title: request.title, content: request.markdown } };
      return { operationId: request.operationId, draftGeneration: request.draftGeneration, noteId: request.noteId, noteVersion: base.noteVersion,
        contentHash: `hash-${base.noteVersion}`, title: request.title, committedMarkdown: request.markdown, updatedAt: 2 };
    }), discard: vi.fn(async () => undefined) };
  return { api, session: new NoteEditSession("one", api) };
}
afterEach(() => vi.useRealTimers());
describe("shared versioned note session", () => {
  it("does not replace new input with a late save receipt", async () => {
    vi.useFakeTimers();
    const { api, session } = fixture(); await session.load();
    let release!: () => void;
    const original = api.save.getMockImplementation()!;
    api.save.mockImplementationOnce(async (request) => { await new Promise<void>((resolve) => { release = resolve; }); return original(request); });
    session.edit({ content: "first" });
    const saved = session.save();
    await vi.waitFor(() => expect(api.save).toHaveBeenCalled());
    session.edit({ content: "second" }); release(); await saved;
    expect(session.snapshot().content).toBe("second");
    expect(session.snapshot().base!.note.content).toBe("first");
    expect(session.dirty).toBe(true); session.releaseTimers();
  });
  it("retains the frozen operation on retry", async () => {
    vi.useFakeTimers();
    const { api, session } = fixture(); await session.load();
    session.edit({ content: "first" }); api.save.mockRejectedValueOnce("NOTE_STORAGE_UNAVAILABLE");
    await expect(session.save()).rejects.toBe("NOTE_STORAGE_UNAVAILABLE");
    session.edit({ content: "second" }); await session.save();
    expect(api.save.mock.calls[0][0]).toEqual(api.save.mock.calls[1][0]);
    expect(session.snapshot().content).toBe("second"); session.releaseTimers();
  });
  it("retains drafts and fails flush on conflict", async () => {
    vi.useFakeTimers();
    const { api, session } = fixture(); await session.load(); session.edit({ content: "mine" });
    api.save.mockRejectedValue("NOTE_VERSION_CONFLICT");
    await expect(session.save()).rejects.toBe("NOTE_VERSION_CONFLICT");
    expect(session.snapshot().phase).toBe("conflict");
    expect(api.checkpoint).toHaveBeenCalled(); expect(session.snapshot().content).toBe("mine"); session.releaseTimers();
  });
  it("does not snapshot active composition", async () => {
    vi.useFakeTimers();
    const { api, session } = fixture(); await session.load(); session.composition(true); session.edit({ content: "输入" });
    await session.checkpoint(); await expect(session.save()).rejects.toThrow();
    expect(api.checkpoint).not.toHaveBeenCalled();
    session.composition(false); await session.checkpoint(); expect(api.checkpoint).toHaveBeenCalledOnce(); session.releaseTimers();
  });
  it("freezes the requested generation before a slow draft checkpoint", async () => {
    vi.useFakeTimers();
    const { api, session } = fixture(); await session.load();
    let release!: () => void;
    api.checkpoint.mockImplementationOnce(() => new Promise<undefined>((resolve) => { release = () => resolve(undefined); }));
    session.edit({ content: "complete input" });
    const saved = session.save();
    await vi.waitFor(() => expect(api.checkpoint).toHaveBeenCalled());
    session.composition(true); session.edit({ content: "unfinished IME" });
    release(); await saved;
    expect(api.save.mock.calls[0][0].markdown).toBe("complete input");
    expect(session.snapshot().content).toBe("unfinished IME");
    session.composition(false); session.releaseTimers();
  });
  it("honors long autosave delays while preserving drafts every two seconds", async () => {
    vi.useFakeTimers();
    const { api, session } = fixture(); await session.load(); session.configure(true, 5000);
    for (let index = 0; index < 8; index++) { session.edit({ content: `text-${index}` }); await vi.advanceTimersByTimeAsync(400); }
    expect(api.checkpoint).toHaveBeenCalled(); expect(api.save).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1800); expect(api.save).toHaveBeenCalledOnce(); session.releaseTimers();
  });
  it("recognizes Error objects as conflicts without discarding local text", async () => {
    vi.useFakeTimers();
    const { api, session } = fixture(); await session.load(); session.edit({ content: "local" });
    api.save.mockRejectedValueOnce(new Error("NOTE_VERSION_CONFLICT: changed"));
    await expect(session.save()).rejects.toThrow("NOTE_VERSION_CONFLICT");
    expect(session.snapshot().phase).toBe("conflict"); expect(session.snapshot().content).toBe("local"); session.releaseTimers();
  });
  it("reports checkpoint failure instead of claiming a recoverable draft", async () => {
    vi.useFakeTimers();
    const { api, session } = fixture(); await session.load(); session.edit({ content: "local" });
    api.checkpoint.mockRejectedValueOnce(new Error("disk full"));
    await expect(session.save()).rejects.toThrow("disk full");
    expect(session.snapshot().phase).toBe("error"); expect(session.snapshot().checkpointGeneration).toBe(-1);
    expect(api.save).not.toHaveBeenCalled(); session.releaseTimers();
  });
});
