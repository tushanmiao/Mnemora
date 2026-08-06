/**
 * 内置模型数据库的前端匹配工具（与 Rust 侧 `src-tauri/src/ai/model/` 共享同一份
 * `modelDatabase.json`，匹配策略保持一致）。
 *
 * 匹配优先级：精确 → 去 `provider/` 前缀精确 → 分隔符归一化精确 → 前缀（最长 key）
 * → 包含（最长 key）。匹配不到返回 `null`，调用方保持宽松默认。
 */

import modelDatabase from "./modelDatabase.json";
import type { ModelPricing } from "../types/modelSettings";

type DatabaseEntry = {
  displayName?: string;
  contextWindow?: number;
  maxOutput?: number;
  capabilities?: {
    vision?: boolean;
    functionCalling?: boolean;
    reasoning?: boolean;
    streaming?: boolean;
    webSearch?: boolean;
    imageGeneration?: boolean;
    embedding?: boolean;
  };
  pricing?: {
    input?: number;
    output?: number;
    cachedInput?: number;
  };
};

/** 数据库记录的模型能力集合（展示用；undefined 表示未收录）。 */
export interface ModelCapabilityDefaults {
  vision?: boolean;
  functionCalling?: boolean;
  reasoning?: boolean;
  webSearch?: boolean;
  imageGeneration?: boolean;
  embedding?: boolean;
}

/** 匹配结果：Mnemora 关心的模型默认元数据。 */
export interface ModelDefaults {
  displayName?: string;
  contextWindowTokens?: number;
  /** 是否支持图片输入；undefined 表示数据库未收录。 */
  supportsVision?: boolean;
  /** 是否支持结构化 Tool Calling；undefined 表示数据库未收录。 */
  supportsFunctionCalling?: boolean;
  /** 是否支持独立 reasoning；undefined 表示数据库未收录。 */
  supportsReasoning?: boolean;
  /** 数据库记录的完整能力集合（设置页徽章展示用）。 */
  capabilities?: ModelCapabilityDefaults;
  pricing?: ModelPricing;
}

const raw = modelDatabase as Record<string, unknown>;
const entries = Object.entries(raw).filter(([key]) => key !== "_meta") as Array<
  [string, DatabaseEntry]
>;
const byKey = new Map(entries);

/** 版本分隔符归一化：数据库键用点号（`gpt-5.5`），中转站可能返回 `gpt-5-5`。 */
function normalizeSeparators(value: string): string {
  return value.replace(/\./g, "-");
}

const normalizedEntries = entries.map(([key, entry]) => ({
  norm: normalizeSeparators(key),
  entry,
}));
const normalizedExact = new Map<string, DatabaseEntry>();
for (const { norm, entry } of normalizedEntries) {
  if (!normalizedExact.has(norm)) normalizedExact.set(norm, entry);
}

function findEntry(apiModel: string): DatabaseEntry | null {
  const name = apiModel.trim().toLowerCase();
  if (!name) return null;
  const cached = matchCache.get(name);
  if (cached !== undefined) return cached;
  const entry = findEntryUncached(name);
  // 有界缓存：键空间是用户配置过的模型名（几十个量级），防御性设个上限。
  if (matchCache.size >= MATCH_CACHE_LIMIT) matchCache.clear();
  matchCache.set(name, entry);
  return entry;
}

const MATCH_CACHE_LIMIT = 512;
const matchCache = new Map<string, DatabaseEntry | null>();

function findEntryUncached(name: string): DatabaseEntry | null {
  const stripped = name.includes("/") ? name.split("/").pop()! : name;

  const exact = byKey.get(name) ?? byKey.get(stripped);
  if (exact) return exact;

  const normName = normalizeSeparators(name);
  const normStripped = normalizeSeparators(stripped);
  const exactNorm =
    normalizedExact.get(normName) ?? normalizedExact.get(normStripped);
  if (exactNorm) return exactNorm;

  const candidates =
    normName === normStripped ? [normStripped] : [normName, normStripped];

  const prefixMatches = normalizedEntries
    .filter(({ norm }) =>
      candidates.some(
        (candidate) =>
          candidate.startsWith(norm) && norm.length < candidate.length,
      ),
    )
    .sort((a, b) => b.norm.length - a.norm.length);
  if (prefixMatches.length > 0) return prefixMatches[0].entry;

  const containsMatches = normalizedEntries
    .filter(({ norm }) =>
      candidates.some(
        (candidate) => norm !== candidate && candidate.includes(norm),
      ),
    )
    .sort((a, b) => b.norm.length - a.norm.length);
  if (containsMatches.length > 0) return containsMatches[0].entry;

  return null;
}

/** 查询模型的内置默认元数据；未收录返回 null。 */
export function matchModelDefaults(apiModel: string): ModelDefaults | null {
  const entry = findEntry(apiModel);
  if (!entry) return null;

  const pricing: ModelPricing | undefined =
    entry.pricing &&
    (entry.pricing.input !== undefined || entry.pricing.output !== undefined)
      ? {
          inputPerMillion: entry.pricing.input,
          outputPerMillion: entry.pricing.output,
          cacheReadPerMillion: entry.pricing.cachedInput,
          currency: "USD",
        }
      : undefined;

  return {
    displayName: entry.displayName,
    contextWindowTokens:
      entry.contextWindow && entry.contextWindow > 0
        ? entry.contextWindow
        : undefined,
    supportsVision: entry.capabilities?.vision,
    supportsFunctionCalling: entry.capabilities?.functionCalling,
    supportsReasoning: entry.capabilities?.reasoning,
    capabilities: entry.capabilities
      ? {
          vision: entry.capabilities.vision,
          functionCalling: entry.capabilities.functionCalling,
          reasoning: entry.capabilities.reasoning,
          webSearch: entry.capabilities.webSearch,
          imageGeneration: entry.capabilities.imageGeneration,
          embedding: entry.capabilities.embedding,
        }
      : undefined,
    pricing,
  };
}

/**
 * 名称家族启发式（参考 cherry-studio 的 vision 白/黑名单设计）：数据库未命中时
 * 按模型名家族兜底判定。规则与 Rust 侧 `ai/model` 保持一致——只收录高置信家族，
 * 拿不准时返回 undefined 交给上层放行。
 *
 * 顺序敏感：视觉标记先于 DeepSeek 判定，保证 `deepseek-vl` 这类多模态变体不被
 * 家族黑名单误杀。
 */
export function heuristicSupportsVision(
  apiModel: string,
): boolean | undefined {
  const lower = apiModel.trim().toLowerCase();
  const name = lower.includes("/") ? lower.split("/").pop()! : lower;
  if (!name) return undefined;

  // 非对话用途的模型家族：embedding / rerank / 语音，不接受图片。
  const nonChatMarkers = ["embed", "rerank", "tts", "whisper"];
  if (nonChatMarkers.some((marker) => name.includes(marker))) return false;

  // 高置信视觉家族（名称即宣告多模态）。
  const visionMarkers = [
    "vision",
    "-vl",
    "vl-",
    "llava",
    "pixtral",
    "internvl",
    "qvq",
    "moondream",
    "minicpm-v",
    "gpt-4o",
  ];
  if (visionMarkers.some((marker) => name.includes(marker))) return true;

  // DeepSeek 对话家族（非 VL 变体）至今不支持图片输入。
  if (name.includes("deepseek")) return false;

  return undefined;
}

/**
 * 解析模型的有效视觉能力：用户覆盖 → 内置数据库 → 名称家族启发式。
 * 返回 undefined 表示未知（界面与发送逻辑应保持放行）。
 */
export function resolveSupportsVision(
  apiModel: string,
  override?: boolean,
): boolean | undefined {
  if (override !== undefined) return override;
  return (
    matchModelDefaults(apiModel)?.supportsVision ??
    heuristicSupportsVision(apiModel)
  );
}

/**
 * 解析模型的有效 Tool Calling 能力。未知模型采用保守 false；用户可在模型设置中
 * 显式开启中转商改名或数据库尚未收录的模型。
 */
export function resolveSupportsFunctionCalling(
  apiModel: string,
  override?: boolean,
): boolean {
  if (override !== undefined) return override;
  return matchModelDefaults(apiModel)?.supportsFunctionCalling === true;
}

/** 解析 reasoning 能力；未知保持 undefined，只影响 thinking 参数和界面提示。 */
export function resolveSupportsReasoning(
  apiModel: string,
  override?: boolean,
): boolean | undefined {
  if (override !== undefined) return override;
  return matchModelDefaults(apiModel)?.supportsReasoning;
}
