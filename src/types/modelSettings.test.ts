import { describe, expect, it } from "vitest";
import { createInitialModelSettings, resolveNoteModel } from "./modelSettings";

describe("resolveNoteModel", () => {
  it("prefers a configured note model and falls back to the conversation model", () => {
    const settings = createInitialModelSettings();
    settings.providers[0].models = [{
      id: "chat-model",
      apiModel: "chat-model",
      displayName: "Chat Model",
      contextWindowTokens: 128_000,
      enabled: true,
    }];
    settings.providers[1].models = [{
      id: "note-model",
      apiModel: "note-model",
      displayName: "Note Model",
      contextWindowTokens: 128_000,
      enabled: true,
    }];
    settings.defaultProviderId = settings.providers[0].id;
    settings.defaultModelId = "chat-model";
    settings.noteProviderId = settings.providers[1].id;
    settings.noteModelId = "note-model";

    expect(resolveNoteModel(settings, settings.providers[0].id, "chat-model")?.model.id)
      .toBe("note-model");

    settings.providers[1].models[0].enabled = false;
    expect(resolveNoteModel(settings, settings.providers[0].id, "chat-model")?.model.id)
      .toBe("chat-model");
  });
});
