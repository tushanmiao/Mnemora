/** Rust 的 Option 数值在 JSON 中会序列化为 null，前端也要明确处理它。 */
export type NullableNumber = number | null | undefined;

function isUsableNumber(value: NullableNumber): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

export function formatNumber(value: NullableNumber) {
  if (!isUsableNumber(value)) return "-";

  return new Intl.NumberFormat("zh-CN", {
    notation: value >= 100_000 ? "compact" : "standard",
    maximumFractionDigits: 1,
  }).format(value);
}

export function formatOptional(value: NullableNumber) {
  return formatNumber(value);
}

export function formatDuration(value: NullableNumber) {
  if (!isUsableNumber(value) || value < 0) return "-";
  return value < 1_000
    ? `${Math.round(value)} ms`
    : `${(value / 1_000).toFixed(1)} s`;
}

export function formatSpeed(value: NullableNumber) {
  if (!isUsableNumber(value) || value < 0) return "-";
  return `${value.toFixed(1)} tok/s`;
}

export function formatCost(value: NullableNumber) {
  if (!isUsableNumber(value) || value < 0) return "-";
  return `$${value.toFixed(value < 0.01 ? 5 : 3)}`;
}

export function formatDate(value: NullableNumber) {
  if (!isUsableNumber(value)) return "-";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(value);
}

export function formatShortDate(value: NullableNumber) {
  if (!isUsableNumber(value)) return "-";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "numeric",
    day: "numeric",
  }).format(value);
}

export function formatHour(value: NullableNumber) {
  if (!isUsableNumber(value)) return "-";
  return new Intl.DateTimeFormat("zh-CN", { hour: "2-digit" }).format(value);
}
