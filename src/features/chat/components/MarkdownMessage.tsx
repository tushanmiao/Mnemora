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
import { Check, Copy, X } from "lucide-react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { splitStreamingMarkdownBlocks } from "../utils/streamingMarkdown";
import "../styles/markdown-message.css";

type MarkdownMessageProps = {
  content: string;
  streaming?: boolean;
};

type CopyState = "idle" | "copied" | "error";

async function openMarkdownLink(event: ReactMouseEvent<HTMLAnchorElement>) {
  if (!isTauri()) return;
  event.preventDefault();

  try {
    await openUrl(event.currentTarget.href);
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
  const resetTimerRef = useRef<number | null>(null);
  const code = extractText(children).replace(/\n$/, "");
  const language = extractLanguage(children);

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

  return (
    <div className="markdown-code-block">
      <div className="markdown-code-toolbar">
        <span>{language ?? "代码"}</span>
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

type MarkdownBlockProps = {
  content: string;
  isStreamingTail: boolean;
};

const MarkdownBlock = memo(function MarkdownBlock({
  content,
  isStreamingTail,
}: MarkdownBlockProps) {
  return (
    <ReactMarkdown
      remarkPlugins={remarkPlugins}
      components={isStreamingTail ? streamingTailComponents : markdownComponents}
      skipHtml
      disallowedElements={["img"]}
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
    () => (streaming ? splitStreamingMarkdownBlocks(content) : [content]),
    [content, streaming],
  );

  return (
    <div className="markdown-content" data-streaming={streaming || undefined}>
      {streaming ? blocks.map((block, index) => (
        <MarkdownBlock
          content={block}
          isStreamingTail={index === blocks.length - 1}
          key={index}
        />
      )) : (
        <MarkdownBlock content={content} isStreamingTail={false} />
      )}
    </div>
  );
});
