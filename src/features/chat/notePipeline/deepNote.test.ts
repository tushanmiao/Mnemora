import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Conversation } from "../../../types/conversation";
import type { SelectedModel } from "../runtime/generationHelpers";
import { completeChat } from "../api/chat";
import {
  createLibraryNote,
  createLibraryNoteWithSources,
} from "../../library/api/library";
import { generateDeepNote, prepareDeepNote, type DeepNotePrepared } from "./deepNote";
import type { DeepNoteOutline } from "./outlineSchema";

vi.mock("../api/chat", () => ({ completeChat: vi.fn() }));
vi.mock("../../library/api/library", () => ({
  createLibraryNote: vi.fn(),
  createLibraryNoteWithSources: vi.fn(),
}));

const conversation: Conversation = {
  id: "conversation-1",
  title: "MVCC",
  messages: [{
    id: "message-1",
    conversationId: "conversation-1",
    role: "user",
    content: "解释 MVCC",
    status: "completed",
    createdAt: 1,
    updatedAt: 1,
  }],
  assistantId: null,
  providerId: "provider-1",
  modelId: "model-1",
  systemPrompt: "",
  contextSummary: "",
  compressedUntilMessageId: null,
  contextCompressionCount: 0,
  enabledSkillIds: [],
  linkedLibraryItemIds: [],
  permissionMode: "askSensitive",
  projectId: null,
  collectionId: null,
  pinned: false,
  createdAt: 1,
  updatedAt: 1,
};

const selectedModel: SelectedModel = {
  provider: {
    id: "provider-1",
    name: "Provider",
    kind: "custom",
    protocol: "openAiChatCompletions",
    authScheme: "bearer",
    baseUrl: "https://example.com/v1",
    credentialRevision: 0,
    hasApiKey: true,
    enabled: true,
    models: [],
  },
  model: {
    id: "model-1",
    apiModel: "model-1",
    displayName: "Model",
    contextWindowTokens: 128_000,
    enabled: true,
  },
};

const outline: DeepNoteOutline = {
  title: "MVCC 深度笔记",
  summary: "并发控制概览。",
  weakPoints: [],
  sections: [
    { id: "sec-1", heading: "概念", kind: "concept", brief: "解释版本可见性", needsSupplement: false, sourceMessageIds: ["message-1"] },
    { id: "sec-2", heading: "自检问题", kind: "selfcheck", brief: "检查理解", needsSupplement: true, sourceMessageIds: [] },
  ],
};

function prepared(overrides: Partial<DeepNotePrepared> = {}): DeepNotePrepared {
  return {
    conversation,
    model: selectedModel,
    transcript: "### 用户\n解释 MVCC",
    outline,
    options: { maxOutputTokens: 8_192, thinkingEnabled: false, retryAttempts: 2 },
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(createLibraryNote).mockResolvedValue({
    id: "simple-note",
    itemId: null,
    itemTitle: null,
    title: "简版笔记",
    content: "# 简版笔记",
    directoryPath: null,
    contentHash: null,
    attachments: [],
    groupName: null,
    createdAt: 1,
    updatedAt: 1,
  });
  vi.mocked(createLibraryNoteWithSources).mockImplementation(async (create) => ({
    id: "deep-note",
    itemId: null,
    itemTitle: null,
    title: create.title,
    content: create.content,
    directoryPath: null,
    contentHash: null,
    attachments: [],
    groupName: null,
    createdAt: 1,
    updatedAt: 1,
  }));
});

describe("deep note pipeline", () => {
  it("retries invalid analyst JSON once and then falls back to the simple summary", async () => {
    vi.mocked(completeChat)
      .mockResolvedValueOnce({ text: "not json" })
      .mockResolvedValueOnce({ text: "still not json" })
      .mockResolvedValueOnce({ text: "# 简版笔记\n\n正文" });

    const result = await prepareDeepNote(conversation, selectedModel, {
      maxOutputTokens: 8_192,
      thinkingEnabled: false,
      retryAttempts: 2,
    });

    expect(result.degradedNote?.title).toBe("简版笔记");
    expect(completeChat).toHaveBeenCalledTimes(3);
  });

  it("inserts a placeholder after section retries are exhausted and still persists", async () => {
    vi.mocked(completeChat).mockRejectedValue(new Error("provider unavailable"));

    const result = await generateDeepNote(prepared({ outline: { ...outline, sections: [outline.sections[0]] } }), {
      ...outline,
      sections: [outline.sections[0]],
    });

    expect(result.warnings.some((warning) => warning.includes("生成失败"))).toBe(true);
    expect(vi.mocked(createLibraryNoteWithSources).mock.calls[0][0].content)
      .toContain("[本章生成失败，可稍后重试]");
    expect(completeChat).toHaveBeenCalledTimes(2);
  });

  it("saves completed sections as a draft when cancelled between chapters", async () => {
    const controller = new AbortController();
    vi.mocked(completeChat).mockResolvedValue({ text: "## 概念\n\n版本可见性说明。" });
    const run = prepared({
      options: {
        maxOutputTokens: 8_192,
        thinkingEnabled: false,
        retryAttempts: 1,
        signal: controller.signal,
        onProgress: (progress) => {
          if (progress.phase === "drafting" && progress.current === 2) controller.abort();
        },
      },
    });

    const result = await generateDeepNote(run, outline);

    expect(result.note.title).toContain("（草稿）");
    expect(completeChat).toHaveBeenCalledTimes(1);
    expect(vi.mocked(createLibraryNoteWithSources).mock.calls[0][1])
      .toEqual(expect.arrayContaining([expect.objectContaining({ sectionId: "sec-1" })]));
  });
});
