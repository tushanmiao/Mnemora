import type { ResourceRegistrySnapshot } from "../../../runtime/resources/ResourceRegistry";

export type MemoryProcessSample = {
  pid: number;
  parentPid: number | null;
  role: string;
  name: string;
  workingSetBytes: number;
  privateBytes: number | null;
};

export type MemoryProcessTreeSample = {
  capturedAtMs: number;
  rootPid: number;
  totalWorkingSetBytes: number;
  totalPrivateBytes: number | null;
  processes: MemoryProcessSample[];
};

export type PageMemorySample = {
  capturedAtMs: number;
  jsHeapUsedBytes: number | null;
  jsHeapTotalBytes: number | null;
  domNodes: number;
  canvasCount: number;
  canvasPixels: number;
  canvasEstimatedBytes: number;
  imageCount: number;
  imageDecodedEstimatedBytes: number;
  audioCount: number;
  registry: ResourceRegistrySnapshot;
};

export type MemoryTimelineSample = {
  scene: string;
  process: MemoryProcessTreeSample;
  page: PageMemorySample;
};
