import { useEffect, useRef, useState } from "react";
import type { ChatMessage } from "../../../types/chat";
import { estimateTextTokens, type ContextUsageEstimate } from "../utils/contextUsage";
import {
  contextInputBudget,
  CONTEXT_SAFETY_RATIO,
  MIN_CONTEXT_SAFETY_TOKENS,
} from "../utils/contextCompression";
import "../styles/context-usage-indicator.css";

type Props = {
  usage: ContextUsageEstimate;
  contextWindowTokens: number | null;
  maxOutputTokens: number;
  messageCount: number;
  compressionCount?: number;
  disabled?: boolean;
  placement?: "up" | "down";
  messages?: ChatMessage[];
  systemPrompt?: string;
  availableSkillCount?: number;
};

export function ContextUsageIndicator({
  usage,
  contextWindowTokens,
  maxOutputTokens,
  messageCount,
  compressionCount = 0,
  disabled = false,
  placement = "down",
  messages = [],
  systemPrompt = "",
  availableSkillCount = 0,
}: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const inputBudget = contextWindowTokens && contextWindowTokens > 0
    ? contextInputBudget(contextWindowTokens, maxOutputTokens)
    : null;
  const ratio = inputBudget && inputBudget > 0
    ? usage.tokens / inputBudget
    : null;
  const ringPercent = ratio === null ? 0 : Math.min(100, Math.max(0, ratio * 100));
  const color = ratio !== null && ratio >= 0.9
    ? "var(--color-danger)"
    : ratio !== null && ratio >= 0.7
      ? "#b7791f"
      : "var(--color-accent)";
  const breakdown = open
    ? contextBreakdown(
        usage.tokens,
        messages,
        systemPrompt,
        availableSkillCount,
        contextWindowTokens,
        maxOutputTokens,
      )
    : null;

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  const title = contextWindowTokens
    ? `可用输入 ${formatTokens(usage.tokens)} / ${formatTokens(inputBudget ?? 0)} · ${Math.round(ringPercent)}%`
    : `上下文约 ${formatTokens(usage.tokens)} · 请在模型设置中填写上下文窗口`;

  return (
    <div className="context-usage-control" ref={rootRef}>
      <button
        className="context-usage-trigger"
        type="button"
        title={title}
        aria-label={title}
        aria-expanded={open}
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
      >
        <span
          className="context-usage-ring"
          style={{
            background: ratio === null
              ? "var(--color-border)"
              : `conic-gradient(${color} ${ringPercent * 3.6}deg, var(--color-border) 0deg)`,
          }}
        >
          <span />
        </span>
      </button>

      {open ? (
        <div className={`context-usage-popover context-usage-popover-${placement}`}>
          <div className="context-usage-heading">
            <strong>上下文用量</strong>
            <span>{ratio === null ? "窗口未知" : `${Math.round(ringPercent)}%`}</span>
          </div>
          <div className="context-usage-values">
            <strong>{usage.source === "estimated" ? "约 " : ""}{formatTokens(usage.tokens)}</strong>
            <span>/ {inputBudget ? formatTokens(inputBudget) : "未设置"}</span>
          </div>
          <div className="context-usage-bar" aria-hidden="true">
            <span style={{ width: `${ringPercent}%`, background: color }} />
          </div>
          <dl className="context-usage-details">
            <div><dt>统计来源</dt><dd>{usage.source === "providerAnchored" ? "供应商实报 + 增量估算" : "本地轻量估算"}</dd></div>
            <div><dt>对话消息</dt><dd>{messageCount} 条</dd></div>
            <div><dt>自动压缩</dt><dd>{compressionCount} 次</dd></div>
            {breakdown?.map(([label, tokens]) => (
              <div key={label}><dt>{label}</dt><dd>约 {formatTokens(tokens)}</dd></div>
            ))}
          </dl>
          {contextWindowTokens === null ? (
            <p>在“设置 → 模型服务”中填写该模型的上下文窗口后，圆环才会显示准确比例。</p>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function contextBreakdown(
  total: number,
  messages: ChatMessage[],
  systemPrompt: string,
  availableSkillCount: number,
  contextWindowTokens: number | null,
  maxOutputTokens: number,
): Array<[string, number]> {
  const system = estimateTextTokens(systemPrompt);
  const attachments = messages.reduce((sum, message) => sum + (message.attachments ?? []).reduce(
    (value, attachment) => value + (attachment.kind === "image" ? 1_200 : 80),
    0,
  ), 0);
  const activatedSkills = new Set(messages.flatMap((message) => (
    message.activatedSkills?.map((skill) => skill.id) ?? []
  ))).size * 800;
  const toolDefinitions = availableSkillCount > 0 || messages.some((message) => (message.attachments?.length ?? 0) > 0)
    ? 360
    : 0;
  const toolResults = messages.reduce(
    (sum, message) => sum + (message.toolTraces?.length ?? 0) * 120,
    0,
  );
  const known = system + attachments + activatedSkills + toolDefinitions + toolResults;
  const conversation = Math.max(0, total - known);
  const safety = contextWindowTokens
    ? Math.max(MIN_CONTEXT_SAFETY_TOKENS, Math.ceil(contextWindowTokens * CONTEXT_SAFETY_RATIO))
    : 0;
  return [
    ["System Prompt", system],
    ["对话消息", conversation],
    ["附件 / 图片", attachments],
    ["已激活 Skills", activatedSkills],
    ["工具定义", toolDefinitions],
    ["工具结果", toolResults],
    ["预留输出空间", maxOutputTokens],
    ["安全余量", safety],
  ];
}

function formatTokens(value: number) {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M tokens`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K tokens`;
  return `${value} tokens`;
}
