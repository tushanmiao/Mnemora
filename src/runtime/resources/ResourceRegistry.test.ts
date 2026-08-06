import { describe, expect, it, vi } from "vitest";
import {
  getResourceRegistrySnapshot,
  registerResource,
  releaseBackgroundResources,
  releaseResourcesForOwner,
} from "./ResourceRegistry";

describe("ResourceRegistry", () => {
  it("tracks bounded resources and releases by owner", () => {
    const release = vi.fn();
    registerResource({ owner: "test-owner", kind: "cache", estimatedBytes: 128, release });
    expect(getResourceRegistrySnapshot().estimatedBytes).toBeGreaterThanOrEqual(128);
    releaseResourcesForOwner("test-owner");
    expect(release).toHaveBeenCalledOnce();
  });

  it("releases only resources marked for background eviction", () => {
    const background = vi.fn();
    const active = vi.fn();
    const backgroundHandle = registerResource({ owner: "background", kind: "canvas", backgroundReleasable: true, release: background });
    const activeHandle = registerResource({ owner: "active", kind: "worker", release: active });
    releaseBackgroundResources();
    expect(background).toHaveBeenCalledOnce();
    expect(active).not.toHaveBeenCalled();
    backgroundHandle.release();
    activeHandle.release();
  });
});
