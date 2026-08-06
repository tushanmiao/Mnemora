export type ResourceKind =
  | "audio"
  | "cache"
  | "canvas"
  | "objectUrl"
  | "observer"
  | "timer"
  | "worker";

export type ResourceRegistration = {
  owner: string;
  kind: ResourceKind;
  estimatedBytes?: number;
  backgroundReleasable?: boolean;
  release: () => void;
};

export type ResourceRegistrySnapshot = {
  count: number;
  estimatedBytes: number;
  byKind: Partial<Record<ResourceKind, { count: number; estimatedBytes: number }>>;
};

type RegisteredResource = ResourceRegistration & {
  id: string;
  createdAt: number;
  lastUsedAt: number;
};

const resources = new Map<string, RegisteredResource>();

export function registerResource(registration: ResourceRegistration) {
  const id = crypto.randomUUID();
  const now = Date.now();
  resources.set(id, { ...registration, id, createdAt: now, lastUsedAt: now });
  let released = false;
  return {
    id,
    touch() {
      const resource = resources.get(id);
      if (resource) resource.lastUsedAt = Date.now();
    },
    release() {
      if (released) return;
      released = true;
      resources.delete(id);
    },
  };
}

export function releaseResourcesForOwner(owner: string) {
  releaseMatching((resource) => resource.owner === owner);
}

export function releaseBackgroundResources() {
  releaseMatching((resource) => resource.backgroundReleasable === true);
}

export function getResourceRegistrySnapshot(): ResourceRegistrySnapshot {
  const snapshot: ResourceRegistrySnapshot = { count: 0, estimatedBytes: 0, byKind: {} };
  for (const resource of resources.values()) {
    const estimatedBytes = Math.max(0, resource.estimatedBytes ?? 0);
    snapshot.count += 1;
    snapshot.estimatedBytes += estimatedBytes;
    const current = snapshot.byKind[resource.kind] ?? { count: 0, estimatedBytes: 0 };
    snapshot.byKind[resource.kind] = {
      count: current.count + 1,
      estimatedBytes: current.estimatedBytes + estimatedBytes,
    };
  }
  return snapshot;
}

function releaseMatching(predicate: (resource: RegisteredResource) => boolean) {
  const matches = [...resources.values()]
    .filter(predicate)
    .sort((left, right) => left.lastUsedAt - right.lastUsedAt);
  for (const resource of matches) {
    resources.delete(resource.id);
    try {
      resource.release();
    } catch (error) {
      console.error("释放运行资源失败", error);
    }
  }
}
