import {
  lazy,
  memo,
  Suspense,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, Copy, Eye, LoaderCircle, X } from "lucide-react";
import ReactMarkdown, { type Components } from "react-markdown";
import type { LiteratureReference } from "../../../types/chat";
import { safeMarkdownContentUrlTransform, safeMarkdownUrlTransform } from "../utils/htmlSecurity";
import {
  renderableStreamingBlock,
  splitStreamingMarkdownBlocks,
  type StreamingMarkdownBlock,
} from "../utils/streamingMarkdown";
import { extractCodeLanguage, extractCodeText, isMermaidLanguage, normalizeCodeLanguage } from "../markdown/utils/codeBlock";
import { createMarkdownRehypePlugins, createMarkdownRemarkPlugins } from "../markdown/plugins/markdownPlugins";
import { extractMarkdownOutline } from "../markdown/utils/outline";
import { MARKDOWN_RENDER_LIMITS } from "../markdown/utils/renderLimits";
import { MermaidBlock } from "../markdown/components/MermaidBlock";
import { HighlightedCodeBlock } from "../markdown/components/HighlightedCodeBlock";
import { SafeMarkdownImage } from "../markdown/components/SafeMarkdownImage";
import { LearningCallout } from "../markdown/components/LearningCallout";
import { MessageOutline } from "../markdown/components/MessageOutline";
import { RenderFallback } from "../markdown/components/RenderFallback";
import "../styles/markdown-message.css";
import "../markdown/styles/enhanced-markdown.css";

type MarkdownMessageProps = {
  content: string;
  streaming?: boolean;
  messageId?: string;
  literatureReferences?: readonly LiteratureReference[];
  onLiteratureReferenceOpen?: (reference: LiteratureReference) => void;
};

type CopyState = "idle" | "copied" | "error";
type PreviewState = "idle" | "loading" | "error";
type MarkdownPreProps = ComponentPropsWithoutRef<"pre"> & {
  "data-mermaid-disabled"?: "true";
};

const MathMarkdownContent = lazy(() => import("./MathMarkdownContent"));

async function openMarkdownLink(event: ReactMouseEvent<HTMLAnchorElement>) {
  const href = event.currentTarget.getAttribute("href") ?? "";
  if (href.startsWith("#")) {
    event.preventDefault();
    document.getElementById(href.slice(1))?.scrollIntoView({ behavior: "smooth", block: "center" });
    return;
  }
  if (href.startsWith("mnemora-citation:")) {
    event.preventDefault();
    return;
  }
  if (!isTauri()) return;
  event.preventDefault();
  if (!href) return;
  try {
    await openUrl(href);
  } catch (error) {
    console.error("打开 Markdown 链接失败", error);
  }
}

function MarkdownCodeBlock({ children, ...props }: MarkdownPreProps) {
  const [copyState, setCopyState] = useState<CopyState>("idle");
  const [previewState, setPreviewState] = useState<PreviewState>("idle");
  const resetTimerRef = useRef<number | null>(null);
  const code = extractCodeText(children).replace(/\n$/, "");
  const language = extractCodeLanguage(children);
  const normalizedLanguage = normalizeCodeLanguage(language);
  const canPreview = normalizedLanguage === "html" || normalizedLanguage === "htm";

  useEffect(() => () => {
    if (resetTimerRef.current !== null) window.clearTimeout(resetTimerRef.current);
  }, []);

  const handleCopy = async () => {
    try {
      if (!navigator.clipboard) throw new Error("当前环境不支持剪贴板 API");
      await navigator.clipboard.writeText(code);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
    if (resetTimerRef.current !== null) window.clearTimeout(resetTimerRef.current);
    resetTimerRef.current = window.setTimeout(() => setCopyState("idle"), 1600);
  };

  const handlePreview = async () => {
    if (!canPreview || !code || previewState === "loading") return;
    setPreviewState("loading");
    try {
      const { openHtmlPreview } = await import("../../html-preview/api");
      await openHtmlPreview(code);
      setPreviewState("idle");
    } catch (error) {
      console.error("打开 HTML 预览失败", error);
      setPreviewState("error");
    }
  };

  if (isMermaidLanguage(language) && props["data-mermaid-disabled"] !== "true") return <MermaidBlock code={code} />;
  if (normalizedLanguage && normalizedLanguage !== "html" && normalizedLanguage !== "htm") {
    return <HighlightedCodeBlock code={code} language={language} />;
  }

  return (
    <div className="markdown-code-block">
      <div className="markdown-code-toolbar">
        <span>{language ?? "代码"}</span>
        <div className="markdown-code-actions">
          {canPreview ? (
            <button
              className={`markdown-copy-button markdown-preview-button-${previewState}`}
              type="button"
              title={previewState === "error" ? "HTML 预览失败，点击重试" : "预览 HTML"}
              aria-label={previewState === "error" ? "重试 HTML 预览" : "预览 HTML"}
              disabled={previewState === "loading"}
              onClick={() => void handlePreview()}
            >
              {previewState === "loading" ? <LoaderCircle className="message-spin" size={15} /> : null}
              {previewState === "error" ? <X size={15} /> : null}
              {previewState === "idle" ? <Eye size={15} /> : null}
            </button>
          ) : null}
          <button
            className={`markdown-copy-button markdown-copy-button-${copyState}`}
            type="button"
            title={copyState === "copied" ? "已复制" : copyState === "error" ? "复制失败" : "复制代码"}
            aria-label={copyState === "copied" ? "已复制" : copyState === "error" ? "复制失败" : "复制代码"}
            onClick={() => void handleCopy()}
          >
            {copyState === "copied" ? <Check size={15} /> : null}
            {copyState === "error" ? <X size={15} /> : null}
            {copyState === "idle" ? <Copy size={15} /> : null}
          </button>
        </div>
      </div>
      <pre {...props}>{children}</pre>
    </div>
  );
}

function createMarkdownComponents(
  literatureReferences: readonly LiteratureReference[],
  onLiteratureReferenceOpen?: (reference: LiteratureReference) => void,
  allowedMermaidOffsets: ReadonlySet<number> = new Set(),
): Components {
  return {
    a({ node, ...props }) {
      void node;
      const href = props.href ?? "";
      const citationId = href.startsWith("mnemora-citation:")
        ? decodeURIComponent(href.slice("mnemora-citation:".length))
        : null;
      const citation = citationId
        ? literatureReferences.find((reference) => reference.id === citationId)
        : null;
      return (
        <a
          {...props}
          className={citation ? "markdown-literature-citation" : props.className}
          target={href.startsWith("#") || citation ? undefined : "_blank"}
          rel={href.startsWith("#") || citation ? undefined : "noreferrer noopener"}
          onClick={(event) => {
            if (citation) {
              event.preventDefault();
              onLiteratureReferenceOpen?.(citation);
              return;
            }
            void openMarkdownLink(event);
          }}
        />
      );
    },
    aside({ node, ...props }) {
      void node;
      return <LearningCallout {...props} />;
    },
    img({ node, ...props }) {
      void node;
      return <SafeMarkdownImage {...props} />;
    },
    pre({ node, ...props }) {
      const language = extractCodeLanguage(props.children);
      const isMermaid = isMermaidLanguage(language);
      const offset = node?.position?.start.offset;
      const allowMermaid = !isMermaid || (typeof offset === "number" && allowedMermaidOffsets.has(offset));
      return <MarkdownCodeBlock {...props} data-mermaid-disabled={allowMermaid ? undefined : "true"} />;
    },
    table({ node, ...props }) {
      void node;
      return <div className="markdown-table-scroll"><table {...props} /></div>;
    },
  };
}

function findAllowedMermaidOffsets(content: string, budget: number) {
  const offsets = new Set<number>();
  if (budget <= 0) return offsets;
  const fencePattern = /^ {0,3}```+\s*mermaid(?:\s+[^\n]*)?\s*$/gim;
  for (const match of content.matchAll(fencePattern)) {
    if (offsets.size >= budget) break;
    if (typeof match.index === "number") offsets.add(match.index);
  }
  return offsets;
}

function countMermaidBlocks(content: string) {
  return findAllowedMermaidOffsets(content, Number.MAX_SAFE_INTEGER).size;
}

const streamingComponents: Components = {
  a({ node, ...props }) {
    void node;
    return <a {...props} />;
  },
  pre({ node, ...props }) {
    void node;
    return <pre className="markdown-streaming-code" {...props} />;
  },
};

function containsMath(content: string) {
  return /(?:^|[^\\])\$\$[\s\S]+?\$\$|(?:^|[^\\])\$(?!\s)[^\n$]+?\$(?!\$)/m.test(content);
}

type MarkdownBlockProps = {
  block: StreamingMarkdownBlock;
  isStreamingTail: boolean;
  messageId: string;
  literatureReferences: readonly LiteratureReference[];
  mermaidBudget: number;
  onLiteratureReferenceOpen?: (reference: LiteratureReference) => void;
};

const MarkdownBlock = memo(function MarkdownBlock({
  block,
  isStreamingTail,
  messageId,
  literatureReferences,
  mermaidBudget,
  onLiteratureReferenceOpen,
}: MarkdownBlockProps) {
  const content = renderableStreamingBlock(block);
  const allowedMermaidOffsets = useMemo(
    () => findAllowedMermaidOffsets(content, mermaidBudget),
    [content, mermaidBudget],
  );
  const components = useMemo(
    () => createMarkdownComponents(literatureReferences, onLiteratureReferenceOpen, allowedMermaidOffsets),
    [allowedMermaidOffsets, literatureReferences, onLiteratureReferenceOpen],
  );
  const mathEnabled = containsMath(content);
  const remarkPlugins = useMemo(
    () => (isStreamingTail ? [] : createMarkdownRemarkPlugins(literatureReferences)),
    [isStreamingTail, literatureReferences],
  );
  const rehypePlugins = useMemo(
    () => (isStreamingTail ? [] : createMarkdownRehypePlugins(messageId)),
    [isStreamingTail, messageId],
  );
  if (mathEnabled && !isStreamingTail) {
    return (
      <Suspense fallback={<span className="markdown-math-loading">{content}</span>}>
        <MathMarkdownContent
          content={content}
          components={components}
          messageId={messageId}
          literatureReferences={literatureReferences}
        />
      </Suspense>
    );
  }
  return (
    <ReactMarkdown
      remarkPlugins={remarkPlugins}
      rehypePlugins={rehypePlugins}
      components={isStreamingTail ? streamingComponents : components}
      urlTransform={isStreamingTail ? safeMarkdownUrlTransform : safeMarkdownContentUrlTransform}
    >
      {content}
    </ReactMarkdown>
  );
});

export const MarkdownMessage = memo(function MarkdownMessage({
  content,
  streaming = false,
  messageId = "message",
  literatureReferences = [],
  onLiteratureReferenceOpen,
}: MarkdownMessageProps) {
  const blocks = useMemo(
    () => (streaming ? splitStreamingMarkdownBlocks(content) : [{ content, htmlComplete: true, settled: true }]),
    [content, streaming],
  );
  const outline = useMemo(
    () => (streaming ? [] : extractMarkdownOutline(content, messageId)),
    [content, messageId, streaming],
  );
  const mermaidBudgets = useMemo(() => {
    let used = 0;
    return blocks.map((block) => {
      const budget = Math.max(0, MARKDOWN_RENDER_LIMITS.maxMermaidBlocksPerMessage - used);
      used += countMermaidBlocks(renderableStreamingBlock(block));
      return budget;
    });
  }, [blocks]);

  return (
    <div className="markdown-content" data-streaming={streaming || undefined}>
      <MessageOutline items={outline} />
      {streaming ? blocks.map((block, index) => (
        <RenderFallback
          key={`${messageId}-${index}`}
          fallback={<pre className="markdown-streaming-code">{renderableStreamingBlock(block)}</pre>}
        >
          <MarkdownBlock
            block={block}
            isStreamingTail={index === blocks.length - 1 && block.settled !== true}
            messageId={messageId}
            literatureReferences={literatureReferences}
            mermaidBudget={mermaidBudgets[index] ?? 0}
            onLiteratureReferenceOpen={onLiteratureReferenceOpen}
          />
        </RenderFallback>
      )) : (
        <RenderFallback fallback={<pre className="markdown-streaming-code">{content}</pre>}>
          <MarkdownBlock block={blocks[0]} isStreamingTail={false} messageId={messageId} literatureReferences={literatureReferences} mermaidBudget={mermaidBudgets[0] ?? 0} onLiteratureReferenceOpen={onLiteratureReferenceOpen} />
        </RenderFallback>
      )}
    </div>
  );
});
