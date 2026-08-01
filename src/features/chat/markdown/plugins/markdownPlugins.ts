import rehypeRaw from "rehype-raw";
import rehypeSanitize from "rehype-sanitize";
import remarkGfm from "remark-gfm";
import type { PluggableList } from "unified";
import { SAFE_CHAT_HTML_SCHEMA } from "../../utils/htmlSecurity";
import { remarkLearningCallouts } from "./remarkLearningCallouts";
import type { LiteratureReference } from "../../../../types/chat";
import { rehypeScopeDocument } from "./rehypeScopeDocument";
import { remarkLiteratureCitations } from "./remarkLiteratureCitations";

export function createMarkdownRemarkPlugins(references: readonly LiteratureReference[]): PluggableList {
  return [remarkGfm, remarkLearningCallouts, remarkLiteratureCitations(references)];
}

export function createMarkdownRehypePlugins(messageId: string): PluggableList {
  return [
    rehypeRaw,
    rehypeScopeDocument(messageId),
    [rehypeSanitize, SAFE_CHAT_HTML_SCHEMA],
  ];
}
