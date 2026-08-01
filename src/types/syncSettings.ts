export type SyncTarget = "obsidian" | "notion";

export type SyncSettings = {
  version: number;
  enabled: boolean;
  target: SyncTarget;
  autoSync: boolean;
  includeAnnotations: boolean;
  includeMetadata: boolean;
  obsidian: {
    vaultPath: string;
    directory: string;
  };
  notion: {
    parentPageId: string;
    hasToken: boolean;
  };
};

export type SyncItemResult = {
  noteId: string;
  title: string;
  status: "succeeded" | "skipped" | "failed";
  message: string;
};

export type SyncResult = {
  target: SyncTarget;
  attempted: number;
  succeeded: number;
  skipped: number;
  failed: number;
  items: SyncItemResult[];
};

export const DEFAULT_SYNC_SETTINGS: SyncSettings = {
  version: 1,
  enabled: false,
  target: "obsidian",
  autoSync: false,
  includeAnnotations: true,
  includeMetadata: true,
  obsidian: {
    vaultPath: "",
    directory: "Mnemora",
  },
  notion: {
    parentPageId: "",
    hasToken: false,
  },
};
