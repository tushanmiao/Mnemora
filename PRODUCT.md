# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Mnemora primarily serves individual users who use Chinese as their main interface language and need one desktop workspace for sustained AI conversations, document reading, knowledge organization, and English learning.

The product is mouse-first. Basic keyboard behavior remains available for text entry, focus, submission, and dialog dismissal, but complete keyboard-only operation is not a product requirement.

## Product Purpose

Mnemora is a local-first personal knowledge and learning workspace. It brings AI conversations, notes, literature and PDF reading, and English learning into one coherent desktop environment so users can move from asking and reading to organizing and practicing without losing context.

Success means users can resume work quickly, understand where information came from, complete focused reading or learning sessions, and retain their data independently of any single remote service.

## Positioning

Mnemora treats Chat, notes, literature, and English learning as parallel parts of one personal workspace rather than making every task subordinate to a chatbot. Its distinguishing mechanism is the continuity between source material, conversation context, durable notes, and active recall, while keeping core data and workflows local.

## Operating Context

- Windows desktop application built with a web interface inside Tauri.
- Used in long, focused sessions that mix conversation, writing, document reading, and English practice.
- Handles local notes, imported literature and PDFs, model-provider configuration, conversation history, and local English learning records.
- Must remain usable in a 720 x 520 window, with the full multi-panel workspace optimized for approximately 1200 px and wider windows.
- Supports Chinese and English interface text and both light and dark viewing conditions.

## Capabilities and Constraints

- Five top-level workspaces remain first-class: Overview, Chat, Notes, Literature, and English. Settings remains a global destination.
- The shared desktop structure is a global activity bar, an optional workspace sidebar, a primary content area, and an optional context or AI panel.
- Core workflows are local-first. Network use is explicit for model requests, downloads, remote audio, updates, and similar remote resources.
- Views and heavy resources are loaded on demand and released when no longer needed. UI work must preserve the existing low-resource architecture.
- Large collections must use pagination, incremental loading, or virtualization; an interface must not render an unbounded history or dictionary.
- Plan05 may change information hierarchy, navigation placement, component structure, feedback, and small usability details, but does not add major business capabilities.
- The redesign is delivered in independently testable stages while ultimately covering the complete application.

## Brand Commitments

- Product name: Mnemora.
- The current application icon remains unchanged and is outside Plan05.
- The product voice is direct, calm, and work-focused. It should not use punitive streaks, exaggerated celebration, or marketing-style feature narration inside the application.

## Evidence on Hand

- The existing React and Tauri implementation is the source of truth for current features, data contracts, and technical constraints.
- The local `md/plan` and `md/Summary` documents record current workflows and roadmap decisions.
- Plan04 defines the English learning principles, including local-first storage, active recall, new-word versus review behavior, and bounded history views.
- Existing application icons are available under `src-tauri/icons/`; they are not redesign references for Plan05.
- No testimonials, customer logos, usage benchmarks, or external brand system are available and none should be fabricated.

## Product Principles

1. Continue the user's work instead of making them reconstruct context.
2. Keep conversation, sources, notes, and learning connected but independently usable.
3. Make dense operational surfaces efficient and long-form content comfortable to read.
4. Preserve local ownership, bounded resource use, and explainable state.
5. Prefer purposeful feedback and clear recovery over decoration or gamification.

## Accessibility & Inclusion

- Light, dark, paper, and high-contrast themes must maintain readable contrast, targeting WCAG AA for text and essential controls.
- Meaning and status must not be communicated by color alone.
- Icon controls require accessible names and tooltips; focus states remain visible for standard keyboard interaction.
- Chinese and English text must fit without clipping, overlap, or layout instability.
- Reduced-motion preferences must be respected.
