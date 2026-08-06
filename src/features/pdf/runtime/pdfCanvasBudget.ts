import { registerResource } from "../../../runtime/resources/ResourceRegistry";

// 32 MiB covers one typical A4 page at DPR 2 plus a low-resolution neighbour.
// Keeping this below WebView2's previous GPU high-water mark improves memory
// recovery after leaving the reader while preserving normal reading quality.
export const PDF_CANVAS_BUDGET_BYTES = 32 * 1024 * 1024;

type CanvasReservation = {
  owner: string;
  priority: number;
  estimatedBytes: number;
  onEvict: () => void;
  registration: ReturnType<typeof registerResource>;
};

export type PdfCanvasLease = {
  scale: number;
  estimatedBytes: number;
  release: () => void;
};

export class PdfCanvasBudget {
  private reservations = new Map<string, CanvasReservation>();

  constructor(private readonly maxBytes = PDF_CANVAS_BUDGET_BYTES) {}

  reserve(options: {
    owner: string;
    width: number;
    height: number;
    requestedScale: number;
    priority: number;
    onEvict: () => void;
  }): PdfCanvasLease | null {
    this.release(options.owner);
    const width = Math.max(1, options.width);
    const height = Math.max(1, options.height);
    const requestedScale = Math.max(0.1, options.requestedScale);
    const desiredBytes = estimateCanvasBytes(width, height, requestedScale);
    let available = this.maxBytes - this.usedBytes;

    if (desiredBytes > available) {
      const evictable = [...this.reservations.values()]
        .filter((reservation) => reservation.priority > options.priority)
        .sort((left, right) => right.priority - left.priority);
      for (const reservation of evictable) {
        this.evict(reservation.owner);
        available = this.maxBytes - this.usedBytes;
        if (desiredBytes <= available) break;
      }
    }

    available = Math.max(0, this.maxBytes - this.usedBytes);
    const allowedScale = Math.min(
      requestedScale,
      Math.sqrt(available / Math.max(1, width * height * 4)),
    );
    if (!Number.isFinite(allowedScale) || allowedScale < 0.1) return null;
    const estimatedBytes = estimateCanvasBytes(width, height, allowedScale);
    const registration = registerResource({
      owner: options.owner,
      kind: "canvas",
      estimatedBytes,
      backgroundReleasable: true,
      release: () => this.evict(options.owner),
    });
    this.reservations.set(options.owner, {
      owner: options.owner,
      priority: options.priority,
      estimatedBytes,
      onEvict: options.onEvict,
      registration,
    });
    let released = false;
    return {
      scale: allowedScale,
      estimatedBytes,
      release: () => {
        if (released) return;
        released = true;
        this.release(options.owner);
      },
    };
  }

  release(owner: string) {
    const reservation = this.reservations.get(owner);
    if (!reservation) return;
    this.reservations.delete(owner);
    reservation.registration.release();
  }

  releaseAll() {
    for (const owner of [...this.reservations.keys()]) this.evict(owner);
  }

  get usedBytes() {
    let total = 0;
    for (const reservation of this.reservations.values()) total += reservation.estimatedBytes;
    return total;
  }

  private evict(owner: string) {
    const reservation = this.reservations.get(owner);
    if (!reservation) return;
    this.reservations.delete(owner);
    reservation.registration.release();
    reservation.onEvict();
  }
}

function estimateCanvasBytes(width: number, height: number, scale: number) {
  return Math.max(4, Math.ceil(width * scale) * Math.ceil(height * scale) * 4);
}
