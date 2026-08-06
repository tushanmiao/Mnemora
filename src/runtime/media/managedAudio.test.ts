import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createManagedAudio } from "./managedAudio";
import { getResourceRegistrySnapshot, releaseBackgroundResources } from "../resources/ResourceRegistry";

describe("managed audio", () => {
  const originalAudio = globalThis.Audio;

  beforeEach(() => {
    globalThis.Audio = vi.fn(() => ({
      preload: "",
      pause: vi.fn(),
      removeAttribute: vi.fn(),
      load: vi.fn(),
    })) as unknown as typeof Audio;
  });

  afterEach(() => {
    globalThis.Audio = originalAudio;
    releaseBackgroundResources();
  });

  it("registers and releases audio resources exactly once", () => {
    const managed = createManagedAudio("asset://audio.mp3", "english-entry");
    expect(getResourceRegistrySnapshot().byKind.audio?.count).toBe(1);
    managed.release();
    managed.release();
    expect(getResourceRegistrySnapshot().byKind.audio?.count ?? 0).toBe(0);
    expect(managed.audio.pause).toHaveBeenCalledTimes(1);
  });

  it("releases background audio through the registry", () => {
    createManagedAudio("asset://audio.mp3", "english-session");
    releaseBackgroundResources();
    expect(getResourceRegistrySnapshot().byKind.audio?.count ?? 0).toBe(0);
  });
});

