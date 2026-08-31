import { invoke, isTauri } from "@tauri-apps/api/core";

export type RemotePackageKind = "skill" | "plugin" | "pet";

export type RemoteCandidate = {
  fullName: string;
  owner: string;
  description: string;
  htmlUrl: string;
  stars: number;
  pushedAt: string;
  archived: boolean;
  license: string | null;
  defaultBranch: string;
  topics: string[];
};

export type RemoteSearchResult = {
  candidates: RemoteCandidate[];
  totalCount: number;
  truncated: boolean;
};

/** 下载解析完成、安装之前呈现给用户的清单。 */
export type RemotePackagePreview = {
  stagingToken: string;
  kind: RemotePackageKind;
  fullName: string;
  commitSha: string;
  sourceUrl: string;
  packagePath: string;
  id: string;
  name: string;
  version: string;
  description: string;
  publisher: string;
  skillCount: number;
  mcpServerIds: string[];
  networkDomains: string[];
  secrets: string[];
  replacesExisting: boolean;
  warnings: string[];
  totalBytes: number;
  fileCount: number;
};

export type GitHubPackageSource = {
  fullName: string;
  gitRef?: string;
  packagePath?: string;
};

/**
 * 接受 owner/repo、仓库 URL，以及 GitHub tree/blob 目录地址。
 * tree/blob 的 ref 使用第一个路径段；带斜杠的分支请直接使用固定 tag/commit，
 * 避免 GitHub URL 本身无法区分 ref 与仓库内路径的歧义。
 */
export function parseGitHubPackageSource(value: string): GitHubPackageSource | null {
  const trimmed = value.trim();
  if (/^[\w.-]+\/[\w.-]+$/.test(trimmed)) {
    return { fullName: trimmed.replace(/\.git$/i, "") };
  }

  const candidate = /^github\.com\//i.test(trimmed) ? `https://${trimmed}` : trimmed;
  let url: URL;
  try {
    url = new URL(candidate);
  } catch {
    return null;
  }
  if (url.protocol !== "https:" || !["github.com", "www.github.com"].includes(url.hostname.toLocaleLowerCase("en-US"))) {
    return null;
  }
  let segments: string[];
  try {
    segments = url.pathname.split("/").filter(Boolean).map(decodeURIComponent);
  } catch {
    return null;
  }
  if (segments.length < 2 || !segments.slice(0, 2).every((segment) => /^[\w.-]+$/.test(segment))) {
    return null;
  }
  const source: GitHubPackageSource = { fullName: `${segments[0]}/${segments[1].replace(/\.git$/i, "")}` };
  if (segments.length === 2) return source;
  if (!["tree", "blob"].includes(segments[2]) || segments.length < 4) return null;
  source.gitRef = segments[3];
  const path = segments.slice(4).join("/");
  if (path) source.packagePath = path;
  return source;
}

const NEEDS_TAURI = "远端资源包安装需要在 Mnemora 桌面应用中执行。";

export function searchRemotePackages(
  kind: RemotePackageKind,
  query: string,
): Promise<RemoteSearchResult> {
  if (!isTauri()) return Promise.reject(new Error(NEEDS_TAURI));
  return invoke<RemoteSearchResult>("packages_search_remote", { request: { kind, query } });
}

/**
 * 下载并解析，但**不安装**。
 * fullName 必须是 owner/repo；后端只接受这个形式，不接受任意 URL。
 */
export function fetchRemotePackage(
  kind: RemotePackageKind,
  fullName: string,
  gitRef?: string,
  packagePath?: string,
  selector?: string,
): Promise<RemotePackagePreview> {
  if (!isTauri()) return Promise.reject(new Error(NEEDS_TAURI));
  return invoke<RemotePackagePreview>("packages_fetch_remote", {
    request: {
      kind,
      fullName,
      gitRef: gitRef ?? null,
      packagePath: packagePath ?? null,
      selector: selector ?? null,
    },
  });
}

/** 确认安装。token 一次性消费，过期或重放都会被后端拒绝。 */
export function installRemotePackage(
  stagingToken: string,
  replaceExisting: boolean,
): Promise<string> {
  if (!isTauri()) return Promise.reject(new Error(NEEDS_TAURI));
  return invoke<string>("packages_install_remote", {
    request: { stagingToken, replaceExisting },
  });
}

export function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KB", "MB"];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  const amount = value / (1024 ** index);
  return `${amount >= 100 || index === 0 ? amount.toFixed(0) : amount.toFixed(1)} ${units[index]}`;
}

/** 相对时间；用于判断仓库是否长期没人维护。 */
export function formatRelativeDate(value: string) {
  const time = new Date(value).getTime();
  if (!Number.isFinite(time)) return "未知";
  const days = Math.floor((Date.now() - time) / 86_400_000);
  if (days < 1) return "今天";
  if (days < 30) return `${days} 天前`;
  if (days < 365) return `${Math.floor(days / 30)} 个月前`;
  return `${Math.floor(days / 365)} 年前`;
}
