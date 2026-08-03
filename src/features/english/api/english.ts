import { Channel, invoke, isTauri } from "@tauri-apps/api/core";

export type EnglishDictionaryStatus = {
  installed: boolean;
  sourceName: string;
  sourceUrl: string;
  wordCount: number;
  downloadedAt: number | null;
  dataSizeBytes: number;
};

export type EnglishDownloadProgress = {
  phase: "download" | "backup" | "builtin" | "decode" | "index" | "complete";
  downloadedBytes: number;
  totalBytes: number | null;
  indexedWords: number;
  totalWords: number;
  progress: number | null;
  finished: boolean;
};

export type EnglishGroupSummary = { id: number; name: string; count: number };
export type EnglishWordSummary = {
  id: number;
  word: string;
  groupId: number;
  groupName: string;
  pronunciation: string;
  occurrence: number | null;
};
export type EnglishDerivedWord = {
  word: string;
  definition: string;
  partOfSpeech: string;
  wordFormation: string;
};
export type EnglishExamExample = {
  sentence: string;
  source: string;
  section: string;
  sourceKind: string;
};
export type EnglishWordEntry = EnglishWordSummary & {
  translation: string;
  example: string;
  exampleTranslation: string;
  britishAudio: string;
  americanAudio: string;
  mnemonic: string;
  rootAffixes: string;
  englishDefinition: string;
  derivedWords: EnglishDerivedWord[];
  examExamples: EnglishExamExample[];
};
export type EnglishSearchResult = {
  items: EnglishWordSummary[];
  total: number;
  groups: EnglishGroupSummary[];
};

const unavailable = <T,>(value: T) => Promise.resolve(value);

export function getEnglishDictionaryStatus() {
  if (!isTauri()) return unavailable<EnglishDictionaryStatus>({ installed: false, sourceName: "雅思词典", sourceUrl: "https://isdc.pages.dev/", wordCount: 0, downloadedAt: null, dataSizeBytes: 0 });
  return invoke<EnglishDictionaryStatus>("english_dictionary_status");
}

export function downloadEnglishDictionary(onProgress: (progress: EnglishDownloadProgress) => void) {
  if (!isTauri()) return Promise.reject(new Error("词库下载需要在 Tauri 桌面应用中执行。"));
  const channel = new Channel<EnglishDownloadProgress>();
  channel.onmessage = onProgress;
  return invoke<EnglishDictionaryStatus>("english_dictionary_download", { onProgress: channel });
}

export function searchEnglishDictionary(query: string, groupId: number | null, limit = 40) {
  if (!isTauri()) return unavailable<EnglishSearchResult>({ items: [], total: 0, groups: [] });
  return invoke<EnglishSearchResult>("english_dictionary_search", { query, groupId, limit });
}

export function getEnglishWord(wordId: number) {
  if (!isTauri()) return Promise.reject(new Error("词条查询需要在 Tauri 桌面应用中执行。"));
  return invoke<EnglishWordEntry>("english_dictionary_get", { wordId });
}

export function deleteEnglishDictionary() {
  if (!isTauri()) return Promise.reject(new Error("词库管理需要在 Tauri 桌面应用中执行。"));
  return invoke<void>("english_dictionary_delete");
}

export function releaseEnglishDictionary() {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("english_dictionary_release");
}
