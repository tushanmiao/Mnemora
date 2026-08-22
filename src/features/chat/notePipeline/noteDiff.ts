import { diffLines, type Change } from "diff";

export type NoteDiffHunk = {
  id: number;
  oldText: string;
  newText: string;
  oldLines: number;
  newLines: number;
};

type DiffPart = Change & { hunkId?: number };

export function buildNoteDiff(oldContent: string, newContent: string) {
  const parts = diffLines(oldContent, newContent) as DiffPart[];
  const hunks: NoteDiffHunk[] = [];
  let hunk: { oldText: string; newText: string } | null = null;
  const flush = () => {
    if (!hunk) return;
    const id = hunks.length;
    hunks.push({
      id,
      oldText: hunk.oldText,
      newText: hunk.newText,
      oldLines: countLines(hunk.oldText),
      newLines: countLines(hunk.newText),
    });
    hunk = null;
  };
  for (const part of parts) {
    if (part.added || part.removed) {
      hunk ??= { oldText: "", newText: "" };
      if (part.removed) hunk.oldText += part.value;
      if (part.added) hunk.newText += part.value;
    } else {
      flush();
    }
  }
  flush();
  return { parts, hunks };
}

export function applySelectedNoteHunks(
  oldContent: string,
  newContent: string,
  selectedHunkIds: ReadonlySet<number>,
) {
  const parts = diffLines(oldContent, newContent) as DiffPart[];
  let hunkId = -1;
  let inChange = false;
  let output = "";
  for (const part of parts) {
    if (!part.added && !part.removed) {
      inChange = false;
      output += part.value;
      continue;
    }
    if (!inChange) {
      hunkId += 1;
      inChange = true;
    }
    if (selectedHunkIds.has(hunkId)) {
      if (part.added) output += part.value;
    } else if (part.removed) {
      output += part.value;
    }
  }
  return output;
}

function countLines(value: string) {
  if (!value) return 0;
  return value.split(/\r?\n/).length - (value.endsWith("\n") ? 1 : 0);
}
