import { getResourceRegistrySnapshot } from "../../../runtime/resources/ResourceRegistry";
import type { PageMemorySample } from "./types";

type ChromiumPerformanceMemory = {
  usedJSHeapSize: number;
  totalJSHeapSize: number;
};

export function samplePageMemory(): PageMemorySample {
  const canvases = Array.from(document.querySelectorAll("canvas"));
  const images = Array.from(document.images);
  const memory = (performance as Performance & { memory?: ChromiumPerformanceMemory }).memory;
  const canvasPixels = canvases.reduce((sum, canvas) => sum + canvas.width * canvas.height, 0);
  const imageDecodedEstimatedBytes = images.reduce((sum, image) => (
    sum + image.naturalWidth * image.naturalHeight * 4
  ), 0);
  return {
    capturedAtMs: Date.now(),
    jsHeapUsedBytes: memory?.usedJSHeapSize ?? null,
    jsHeapTotalBytes: memory?.totalJSHeapSize ?? null,
    domNodes: document.getElementsByTagName("*").length,
    canvasCount: canvases.length,
    canvasPixels,
    canvasEstimatedBytes: canvasPixels * 4,
    imageCount: images.length,
    imageDecodedEstimatedBytes,
    audioCount: document.querySelectorAll("audio").length,
    registry: getResourceRegistrySnapshot(),
  };
}
