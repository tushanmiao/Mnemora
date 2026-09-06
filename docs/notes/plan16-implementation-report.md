# Plan 16 implementation record

Updated 2026-09-06. The Markdown editor work is implemented in the shared note session and editor host used by ordinary and literature notes. DeepNote output uses the same note editor when it is represented as a Library note.

Implemented and verified:

- CodeMirror live/source/read modes with retained history, selection, scroll position, read-only filtering, native textarea recovery, bounded rendering, and shared sessions.
- Source-preserving formatting commands, headings, list/task editing, links, custom highlight/underline/superscript/subscript syntax, safe search and replace, outline virtualization, and duplicate-heading anchors.
- Local table editing with cell-range writes, IME buffering, rectangular TSV copy/paste, row/column operations, alignment, structural undo boundaries, and budget fallback.
- Code, math, Mermaid, HTML/YAML/image blocks with debounced preview, local source editing, image replacement, and resize remeasurement.
- Versioned saves use operation IDs, generations, content hashes, disk checks, durable drafts, external-change conflict handling, recovery, copy-version, image staging validation, export bundle manifests, and safe attachment opening. Old committed file backups and unreferenced staged images are pruned after retention windows.
- Note citations carry committed `noteVersion`, SHA-256 identity, optional canonical-LF UTF-8 byte ranges, and backend range validation. The model context includes the identity and range when present.
- Note settings migrate safely from older schemas and expose independent typography, focus, typewriter, autosave, wrapping, and rendering preferences for all note entry points.
- English labels, front matter in read/export HTML, relative attachment URL validation, and safe external/citation link handling are covered.

Evidence from this worktree:

- `npm test`: 66 files, 337 tests passed.
- `npm run build`: passed; 4,631 modules transformed.
- `cargo test --lib`: 553 passed, 1 existing ignored.
- `cargo check --lib`: passed; only existing knowledge dead-code warnings remain.
- `npm run css:verify-tokens` and `npm run settings:verify-css`: passed.
- `npm run notes:regression`: passed for table composition/TSV, local block save/undo, anchors, read-only search, English UI, and independent roots.
- `npm run notes:e2e -- --skip-screenshots`: passed for mode round-trip, read-only tasks, save, undo, and 720px overflow.
- `npm run notes:behavior`: passed with p95 input frame time 21.1ms at 51KiB and 50.9ms at 512KiB in Edge 152.
- `node C:/Users/404/.codex/skills/impeccable/scripts/detect.mjs --json`: `[]`.
- Release and license notes: `docs/release/plan16-notes.md`.

Known limits that must remain visible in release notes: the browser tests use synthetic composition events and do not prove every Windows WebView2 IME/DPI driver combination; the 128MiB history soft-cap accounting and citation discovery for legacy external chat records remain conservative; unambiguous source selections receive precise ranges while rendered selections intentionally keep only a versioned excerpt when no safe source mapping exists.
