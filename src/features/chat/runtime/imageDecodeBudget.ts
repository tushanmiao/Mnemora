import { registerResource } from "../../../runtime/resources/ResourceRegistry";

export const CHAT_PREVIEW_DECODE_BUDGET_BYTES = 24 * 1024 * 1024;
export const CHAT_VIEWER_DECODE_LIMIT_BYTES = 32 * 1024 * 1024;
const FALLBACK_PREVIEW_DIMENSION = 640;

type ImageReservation = {
  owner: string;
  estimatedBytes: number;
  onEvict: () => void;
  registration: ReturnType<typeof registerResource>;
};

export type DecodedImageLease = {
  estimatedBytes: number;
  release: () => void;
};

export class DecodedImageBudget {
  private reservations = new Map<string, ImageReservation>();

  constructor(private readonly maxBytes: number) {}

  reserve(options: {
    owner: string;
    estimatedBytes: number;
    onEvict: () => void;
  }): DecodedImageLease | null {
    this.release(options.owner);
    const estimatedBytes = Math.max(0, Math.floor(options.estimatedBytes));
    if (estimatedBytes <= 0 || estimatedBytes > this.maxBytes - this.usedBytes) return null;
    const registration = registerResource({
      owner: options.owner,
      kind: "image",
      estimatedBytes,
      backgroundReleasable: true,
      release: () => this.evict(options.owner),
    });
    this.reservations.set(options.owner, {
      owner: options.owner,
      estimatedBytes,
      onEvict: options.onEvict,
      registration,
    });
    let released = false;
    return {
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

export const chatPreviewDecodeBudget = new DecodedImageBudget(
  CHAT_PREVIEW_DECODE_BUDGET_BYTES,
);

export function estimateDecodedImageBytes(width?: number | null, height?: number | null) {
  if (!Number.isFinite(width) || !Number.isFinite(height)) return 0;
  const normalizedWidth = Math.max(0, Math.floor(width ?? 0));
  const normalizedHeight = Math.max(0, Math.floor(height ?? 0));
  return normalizedWidth * normalizedHeight * 4;
}

export function estimatePreviewDecodedBytes(
  width?: number | null,
  height?: number | null,
) {
  if (!Number.isFinite(width) || !Number.isFinite(height) || !width || !height) {
    return FALLBACK_PREVIEW_DIMENSION * FALLBACK_PREVIEW_DIMENSION * 4;
  }
  const scale = Math.min(1, FALLBACK_PREVIEW_DIMENSION / Math.max(width, height));
  return estimateDecodedImageBytes(width * scale, height * scale);
}
