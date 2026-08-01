export type UpdateCheckSource = "githubApi" | "githubWeb";

export type UpdateCheckResult = {
  currentVersion: string;
  latestVersion: string;
  tag: string;
  available: boolean;
  releaseUrl: string;
  releaseNotes: string;
  publishedAt: string;
  source: UpdateCheckSource;
};

export type SignedUpdateInfo = {
  currentVersion: string;
  version: string;
  date: string;
  body: string;
};

export type UpdateDownloadProgress = {
  downloadedBytes: number;
  totalBytes: number | null;
  finished: boolean;
};
