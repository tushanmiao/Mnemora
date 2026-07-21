import {
  Children,
  isValidElement,
  memo,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, Copy, Eye, LoaderCircle, X } from "lucide-react";
import ReactMarkdown, { type Components } from "react-markdown";
import rehypeRaw from "rehype-raw";
import rehypeSanitize from "rehype-sanitize";
import remarkGfm from "remark-gfm";
import type { PluggableList } from "unified";
import { SAFE_CHAT_HTML_SCHEMA, safeMarkdownUrlTransform } from "../utils/htmlSecurity";
import {
  renderableStreamingBlock,
  splitStreamingMarkdownBlocks,
  type StreamingMarkdownBlock,
} from "../utils/streamingMarkdown";
import "../styles/markdown-message.css";

type MarkdownMessageProps = {
  content: string;
  streaming?: boolean;
};

type CopyState = "idle" | "copied" | "error";
type PreviewState = "idle" | "loading" | "error";

async function openMarkdownLink(event: ReactMouseEvent<HTMLAnchorElement>) {
  if (!isTauri()) return;
  event.preventDefault();
  const href = event.currentTarget.getAttribute("href");
  if (!href) return;

  try {
    await openUrl(href);
  } catch (error) {
    console.error("打开 Markdown 链接失败", error);
  }
}

function extractText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(extractText).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return extractText(node.props.children);
  return "";
}

function extractLanguage(children: ReactNode) {
  const codeElement = Children.toArray(children).find((child) => (
    isValidElement<{ className?: string }>(child)
  ));
  if (!isValidElement<{ className?: string }>(codeElement)) return null;
  return codeElement.props.className?.match(/language-([^\s]+)/)?.[1] ?? null;
}

function MarkdownCodeBlock({ children, ...props }: ComponentPropsWithoutRef<"pre">) {
  const [copyState, setCopyState] = useState<CopyState>("idle");
  const [previewState, setPreviewState] = useState<PreviewState>("idle");
  const resetTimerRef = useRef<number | null>(null);
  const code = extractText(children).replace(/\n$/, "");
  const language = extractLanguage(children);
  const normalizedLanguage = language?.toLowerCase();
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

  const copyLabel = copyState === "copied"
    ? "已复制"
    : copyState === "error"
      ? "复制失败"
      : "复制代码";

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

  return (
    <div className="markdown-code-block">
      <div className="markdown-code-toolbar">
        <span>{language ?? "代码"}</span>
        <div className="markdown-code-actions">
          {canPreview ? (
            <button
              className={`markdown-copy-button markdown-preview-button-${previewState}`}
              type="button"
              title={previewState === "error" ? "HTML 预览打开失败，点击重试" : "预览 HTML"}
              aria-label={previewState === "error" ? "重试 HTML 预览" : "预览 HTML"}
              disabled={previewState === "loading"}
              onClick={() => void handlePreview()}
            >
              {previewState === "loading" ? (
                <LoaderCircle className="message-spin" size={15} />
              ) : null}
              {previewState === "error" ? <X size={15} /> : null}
              {previewState === "idle" ? <Eye size={15} /> : null}
            </button>
          ) : null}
          <button
            className={`markdown-copy-button markdown-copy-button-${copyState}`}
            type="button"
            title={copyLabel}
            aria-label={copyLabel}
            onClick={handleCopy}
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

const markdownComponents: Components = {
  a({ node, ...props }) {
    void node;
    return (
      <a
        {...props}
        target="_blank"
        rel="noreferrer noopener"
        onClick={openMarkdownLink}
      />
    );
  },
  pre({ node, ...props }) {
    void node;
    return <MarkdownCodeBlock {...props} />;
  },
  table({ node, ...props }) {
    void node;
    return (
      <div className="markdown-table-scroll">
        <table {...props} />
      </div>
    );
  },
};

const streamingTailComponents: Components = {
  ...markdownComponents,
  pre({ node, ...props }) {
    void node;
    return <pre className="markdown-streaming-code" {...props} />;
  },
};

const remarkPlugins = [remarkGfm];
const rehypePlugins: PluggableList = [
  rehypeRaw,
  [rehypeSanitize, SAFE_CHAT_HTML_SCHEMA],
];

type MarkdownBlockProps = {
  block: StreamingMarkdownBlock;
  isStreamingTail: boolean;
};

const MarkdownBlock = memo(function MarkdownBlock({
  block,
  isStreamingTail,
}: MarkdownBlockProps) {
  const content = renderableStreamingBlock(block);
  return (
    <ReactMarkdown
      remarkPlugins={remarkPlugins}
      rehypePlugins={rehypePlugins}
      components={isStreamingTail ? streamingTailComponents : markdownComponents}
      disallowedElements={["img"]}
      urlTransform={safeMarkdownUrlTransform}
    >
      {content}
    </ReactMarkdown>
  );
});

export const MarkdownMessage = memo(function MarkdownMessage({
  content,
  streaming = false,
}: MarkdownMessageProps) {
  const blocks = useMemo(
    () => (streaming
      ? splitStreamingMarkdownBlocks(content)
      : [{ content, htmlComplete: true }]),
    [content, streaming],
  );

  return (
    <div className="markdown-content" data-streaming={streaming || undefined}>
      {streaming ? blocks.map((block, index) => (
        <MarkdownBlock
          block={block}
          isStreamingTail={index === blocks.length - 1}
          key={index}
        />
      )) : (
        <MarkdownBlock block={blocks[0]} isStreamingTail={false} />
      )}
    </div>
  );
});
