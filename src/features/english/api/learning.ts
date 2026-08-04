import { convertFileSrc, invoke, isTauri } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

export type EnglishRating = "again" | "hard" | "good" | "easy";
export type EnglishExerciseKind = "meaning_recall" | "spelling" | "dictation";
export type EnglishVerdict = "correct" | "acceptable" | "incorrect" | "skipped";
export type EnglishQueueMode = "mixed" | "review" | "new" | "dictation" | "spelling" | "mistakes" | "mastered";

export type EnglishPlanSettings = {
  newBatchSize: number;
  dailyNewTarget: number;
  reviewBatchSize: number;
  dailyReviewTarget: number;
  desiredRetention: number;
  preferredAccent: "british" | "american";
  autoPlay: boolean;
  playbackRate: number;
  masteredAudits: boolean;
  pauseNewWords: boolean;
  restDays: number[];
  audioCacheMaxMb: number;
  audioPrefetchDays: number;
  dictionaryPageSize: number;
  archivePageSize: number;
  historyPageSize: number;
};

export const defaultEnglishPlanSettings: EnglishPlanSettings = {
  newBatchSize: 10,
  dailyNewTarget: 20,
  reviewBatchSize: 20,
  dailyReviewTarget: 50,
  desiredRetention: 0.9,
  preferredAccent: "british",
  autoPlay: true,
  playbackRate: 1,
  masteredAudits: true,
  pauseNewWords: false,
  restDays: [],
  audioCacheMaxMb: 256,
  audioPrefetchDays: 3,
  dictionaryPageSize: 20,
  archivePageSize: 20,
  historyPageSize: 20,
};

export type EnglishPlanSummary = {
  id: string;
  bookId: string;
  bookName: string;
  status: "active" | "paused" | "completed";
  itemCount: number;
  settings: EnglishPlanSettings;
  startedAt: number;
  updatedAt: number;
};

export type EnglishLearningOverview = {
  activePlan: EnglishPlanSummary | null;
  dueCount: number;
  overdueCount: number;
  masteredDueCount: number;
  newAvailable: number;
  todayNewDone: number;
  todayReviewDone: number;
  learnedCount: number;
  masteredCount: number;
  archivedCount: number;
  weakSkill: string | null;
  estimatedCompletionAt: number | null;
  isRestDay: boolean;
};

export type EnglishAudioCacheStatus = {
  bytes: number;
  files: number;
  maxBytes: number;
  prefetchDays: number;
};

type EnglishCachedAudio = {
  path: string;
  cached: boolean;
};

export type EnglishLearningSnapshot = {
  dictionaryId: number;
  entryKey: string;
  sourceVersion: string;
  word: string;
  groupId: number;
  groupName: string;
  pronunciation: string;
  translation: string;
  example: string;
  exampleTranslation: string;
  britishAudio: string;
  americanAudio: string;
  mnemonic: string;
  rootAffixes: string;
};

export type EnglishRatingPreview = {
  rating: EnglishRating;
  dueAt: number;
  scheduledDays: number;
};

export type EnglishQueueItem = {
  progressId: string;
  itemId: string;
  state: "new" | "learning" | "review" | "relearning" | "mastered" | "archived";
  exerciseKind: EnglishExerciseKind;
  snapshot: EnglishLearningSnapshot;
  dueAt: number | null;
  ratingPreviews: EnglishRatingPreview[];
};

export type EnglishAttemptResult = {
  attemptId: string;
  duplicate: boolean;
  verdict: EnglishVerdict;
  suggestedRating: EnglishRating;
  finalRating: EnglishRating;
  nextDueAt: number;
  scheduledDays: number;
  state: string;
  overview: EnglishLearningOverview;
};

export type EnglishSkillSummary = {
  skill: string;
  attempts: number;
  correct: number;
  hintUses: number;
  averageResponseMs: number;
};

export type EnglishLearningStats = {
  attempts7d: number;
  correct7d: number;
  hintUses7d: number;
  averageResponseMs7d: number;
  dueBacklog: number;
  activeDays7d: number;
  currentStreakDays: number;
  skills: EnglishSkillSummary[];
};

export type EnglishAttemptHistoryItem = {
  id: string;
  word: string;
  exerciseKind: EnglishExerciseKind;
  rawAnswer: string;
  verdict: EnglishVerdict;
  suggestedRating: EnglishRating;
  finalRating: EnglishRating;
  hintLevel: number;
  hintCount: number;
  responseMs: number;
  reviewedAt: number;
  nextDueAt: number;
};

export type EnglishAttemptHistoryPage = {
  items: EnglishAttemptHistoryItem[];
  total: number;
};

export type EnglishArchivedItem = {
  progressId: string;
  word: string;
  translation: string;
  pronunciation: string;
  previousState: EnglishQueueItem["state"];
  archivedAt: number;
};

const emptyOverview: EnglishLearningOverview = {
  activePlan: null,
  dueCount: 0,
  overdueCount: 0,
  masteredDueCount: 0,
  newAvailable: 0,
  todayNewDone: 0,
  todayReviewDone: 0,
  learnedCount: 0,
  masteredCount: 0,
  archivedCount: 0,
  weakSkill: null,
  estimatedCompletionAt: null,
  isRestDay: false,
};

export function getEnglishLearningOverview() {
  if (!isTauri()) return Promise.resolve(emptyOverview);
  return invoke<EnglishLearningOverview>("english_learning_overview");
}

export function createEnglishLearningPlan(input: { name: string; groupIds: number[]; settings: EnglishPlanSettings }) {
  return invoke<EnglishPlanSummary>("english_learning_create_plan", { input });
}

export function updateEnglishLearningPlan(planId: string, settings: EnglishPlanSettings) {
  return invoke<EnglishPlanSummary>("english_learning_update_plan", { input: { planId, settings } });
}

export function addEnglishWordToPlan(wordId: number) {
  return invoke<EnglishLearningOverview>("english_learning_add_word", { wordId });
}

export function pauseEnglishLearningPlan(planId: string, paused: boolean) {
  return invoke<EnglishPlanSummary | null>("english_learning_pause_plan", { planId, paused });
}

export function getEnglishLearningBatch(mode: EnglishQueueMode) {
  if (!isTauri()) return Promise.resolve<EnglishQueueItem[]>([]);
  return invoke<EnglishQueueItem[]>("english_learning_next_batch", { input: { mode } });
}

export function submitEnglishAttempt(input: {
  attemptId: string;
  progressId: string;
  exerciseKind: EnglishExerciseKind;
  rawAnswer: string;
  hintLevel: number;
  hintCount: number;
  responseMs: number;
  finalRating: EnglishRating;
}) {
  return invoke<EnglishAttemptResult>("english_learning_submit_attempt", { input });
}

export function markEnglishItemMastered(progressId: string) {
  return invoke<EnglishLearningOverview>("english_learning_mark_mastered", { progressId });
}

export function archiveEnglishItem(progressId: string) {
  return invoke<EnglishLearningOverview>("english_learning_archive_item", { progressId });
}

export function restoreEnglishItem(progressId: string) {
  return invoke<EnglishLearningOverview>("english_learning_restore_item", { progressId });
}

export function listArchivedEnglishItems(limit = 20, offset = 0) {
  if (!isTauri()) return Promise.resolve<EnglishArchivedItem[]>([]);
  return invoke<EnglishArchivedItem[]>("english_learning_list_archived", { limit, offset });
}

export function getEnglishLearningStats() {
  if (!isTauri()) return Promise.resolve<EnglishLearningStats>({
    attempts7d: 0,
    correct7d: 0,
    hintUses7d: 0,
    averageResponseMs7d: 0,
    dueBacklog: 0,
    activeDays7d: 0,
    currentStreakDays: 0,
    skills: [],
  });
  return invoke<EnglishLearningStats>("english_learning_stats");
}

export function listEnglishAttemptHistory(limit = 20, offset = 0) {
  if (!isTauri()) return Promise.resolve<EnglishAttemptHistoryPage>({ items: [], total: 0 });
  return invoke<EnglishAttemptHistoryPage>("english_learning_list_history", { limit, offset });
}

export async function exportEnglishWordBook(bookName: string) {
  if (!isTauri()) throw new Error("词书导出需要在 Tauri 应用中运行。");
  const safeName = bookName.trim().replace(/[<>:"/\\|?*\u0000-\u001f]/g, "-").slice(0, 80) || "mnemora-word-book";
  const path = await save({
    title: "导出英语词书",
    defaultPath: `${safeName}.mnemora-words.json`,
    filters: [{ name: "Mnemora 英语词书", extensions: ["json"] }],
  });
  if (!path) return false;
  await invoke("english_learning_export_book", { path });
  return true;
}

export async function importEnglishWordBook() {
  if (!isTauri()) throw new Error("词书导入需要在 Tauri 应用中运行。");
  const path = await open({
    title: "导入英语词书",
    multiple: false,
    directory: false,
    filters: [{ name: "Mnemora 英语词书", extensions: ["json"] }],
  });
  if (typeof path !== "string") return null;
  return invoke<EnglishPlanSummary>("english_learning_import_book", { path });
}

export async function resolveEnglishAudio(url: string) {
  if (!url || !isTauri()) return url;
  const result = await invoke<EnglishCachedAudio>("english_learning_cache_audio", { url });
  return result.cached ? convertFileSrc(result.path) : result.path;
}

export function getEnglishAudioCacheStatus() {
  if (!isTauri()) return Promise.resolve<EnglishAudioCacheStatus>({ bytes: 0, files: 0, maxBytes: 0, prefetchDays: 0 });
  return invoke<EnglishAudioCacheStatus>("english_learning_audio_cache_status");
}

export function clearEnglishAudioCache() {
  return invoke<EnglishAudioCacheStatus>("english_learning_clear_audio_cache");
}

export function prefetchEnglishAudio() {
  return invoke<EnglishAudioCacheStatus>("english_learning_prefetch_audio");
}
