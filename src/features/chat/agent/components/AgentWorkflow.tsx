import { useMemo, useState } from "react";
import {
  AlertCircle,
  BrainCircuit,
  Check,
  CheckCircle2,
  ChevronDown,
  Circle,
  CircleStop,
  Clock3,
  LoaderCircle,
  ShieldAlert,
  Sparkles,
  Wrench,
} from "lucide-react";
import type { ChatMessage, ToolTrace } from "../../../../types/chat";
import type { AgentRunStatus, WorkflowStep } from "../../../../types/workflow";
import { useI18n } from "../../../../i18n/I18nProvider";
import { resolveToolApproval } from "../../api/chat";
import { projectAgentWorkflow } from "../projections/workflowProjection";
import "../styles/agent-workflow.css";

type AgentWorkflowProps = {
  message: ChatMessage;
  reasoning: string;
  streaming: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

export function AgentWorkflow({
  message,
  reasoning,
  streaming,
  open,
  onOpenChange,
}: AgentWorkflowProps) {
  const { language, t } = useI18n();
  const [resolvingApprovalId, setResolvingApprovalId] = useState<string | null>(null);
  const projection = useMemo(() => projectAgentWorkflow(message, {
    reasoning,
    streaming,
    language,
  }), [language, message, reasoning, streaming]);
  const { summary } = projection;
  const summaryParts = [
    t("chat.workflowSteps", { count: summary.stepCount }),
    summary.toolCallCount > 0 ? t("chat.workflowTools", { count: summary.toolCallCount }) : null,
    summary.skillCount > 0 ? t("chat.workflowSkills", { count: summary.skillCount }) : null,
    summary.durationMs !== undefined ? formatDuration(summary.durationMs) : null,
  ].filter((part): part is string => Boolean(part));

  const resolveApproval = async (approvalId: string, approved: boolean) => {
    if (resolvingApprovalId) return;
    setResolvingApprovalId(approvalId);
    try {
      await resolveToolApproval(approvalId, approved);
    } finally {
      setResolvingApprovalId(null);
    }
  };

  return (
    <section
      className={`agent-workflow agent-workflow-${projection.status}${open ? " is-open" : ""}`}
      aria-label={t("chat.workflow")}
    >
      <button
        className="agent-workflow-summary"
        type="button"
        aria-expanded={open}
        onClick={() => onOpenChange(!open)}
      >
        <span className="agent-workflow-status-icon" aria-hidden="true">
          <WorkflowStatusIcon status={projection.status} />
        </span>
        <span className="agent-workflow-summary-copy">
          <strong>{workflowStatusLabel(projection.status, language)}</strong>
          <small>{summaryParts.join(" · ")}</small>
        </span>
        <ChevronDown className="agent-workflow-chevron" size={15} aria-hidden="true" />
      </button>

      {open ? (
        <div className="agent-workflow-timeline" role="list">
          {projection.steps.map((step) => (
            <WorkflowStepRow
              key={step.id}
              step={step}
              language={language}
              resolvingApprovalId={resolvingApprovalId}
              onResolveApproval={resolveApproval}
            />
          ))}
        </div>
      ) : null}
    </section>
  );
}

function WorkflowStepRow({
  step,
  language,
  resolvingApprovalId,
  onResolveApproval,
}: {
  step: WorkflowStep;
  language: "zh" | "en";
  resolvingApprovalId: string | null;
  onResolveApproval: (approvalId: string, approved: boolean) => Promise<void>;
}) {
  const { t } = useI18n();
  const tool = step.tool;
  return (
    <div
      className={`agent-workflow-step agent-workflow-step-${step.kind} agent-workflow-step-${step.status}`}
      role="listitem"
    >
      <span className="agent-workflow-node" aria-hidden="true">
        <StepIcon step={step} />
      </span>
      <div className="agent-workflow-step-body">
        <div className="agent-workflow-step-heading">
          <strong>{step.kind === "tool" ? toolNameLabel(step.title, language) : step.title}</strong>
          <span>{stepStatusLabel(step, language)}</span>
          {tool?.durationMs !== undefined ? <small>{formatDuration(tool.durationMs)}</small> : null}
        </div>

        {step.kind === "skill" && step.skill ? (
          <p className="agent-workflow-skill-meta">
            <span>{step.detail}</span>
            <span>{skillActivationLabel(step.skill.activation, language)}</span>
          </p>
        ) : null}

        {step.kind === "reasoning" && step.reasoning ? (
          <details className="agent-workflow-detail">
            <summary>{t("chat.workflowRawReasoning")}</summary>
            <pre>{step.reasoning}</pre>
          </details>
        ) : null}

        {tool ? (
          <ToolStepDetail tool={tool} language={language} />
        ) : null}

        {tool?.status === "awaitingApproval" && tool.approvalId ? (
          <div className="agent-workflow-approval">
            <ShieldAlert size={14} aria-hidden="true" />
            <span>{t("chat.workflowApprovalPrompt")}</span>
            <button
              type="button"
              disabled={resolvingApprovalId !== null}
              onClick={() => void onResolveApproval(tool.approvalId!, false)}
            >
              {t("chat.reject")}
            </button>
            <button
              className="agent-workflow-approve"
              type="button"
              disabled={resolvingApprovalId !== null}
              onClick={() => void onResolveApproval(tool.approvalId!, true)}
            >
              {t("chat.allowOnce")}
            </button>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function ToolStepDetail({ tool, language }: { tool: ToolTrace; language: "zh" | "en" }) {
  const hasOutput = Boolean(tool.preview);
  return (
    <details className="agent-workflow-detail">
      <summary>{language === "en" ? "Call details" : "调用详情"}</summary>
      <div className="agent-workflow-tool-detail">
        <section>
          <span>{language === "en" ? "Input" : "输入"}</span>
          <code>{tool.argumentSummary}</code>
        </section>
        {hasOutput ? (
          <section>
            <span>{language === "en" ? "Result" : "结果"}</span>
            <p>{tool.preview}</p>
          </section>
        ) : null}
      </div>
    </details>
  );
}

function WorkflowStatusIcon({ status }: { status: AgentRunStatus }) {
  if (status === "completed") return <CheckCircle2 size={16} />;
  if (status === "waitingApproval" || status === "waitingUser") return <ShieldAlert size={16} />;
  if (status === "failed" || status === "budgetExhausted") return <AlertCircle size={16} />;
  if (status === "stopped" || status === "paused") return <CircleStop size={16} />;
  return <LoaderCircle className="agent-workflow-spin" size={16} />;
}

function StepIcon({ step }: { step: WorkflowStep }) {
  if (step.kind === "reasoning") return <BrainCircuit size={14} />;
  if (step.kind === "skill") return <Sparkles size={14} />;
  if (step.kind === "tool") return <Wrench size={14} />;
  if (step.kind === "final") return step.status === "completed" ? <Check size={14} /> : <Clock3 size={14} />;
  return step.status === "running" ? <LoaderCircle className="agent-workflow-spin" size={14} /> : <Circle size={14} />;
}

function workflowStatusLabel(status: AgentRunStatus, language: "zh" | "en") {
  const english: Record<AgentRunStatus, string> = {
    preparing: "Workflow preparing",
    running: "Workflow running",
    waitingApproval: "Approval required",
    waitingUser: "Waiting for input",
    paused: "Workflow paused",
    checkpointing: "Saving checkpoint",
    finalizing: "Finalizing answer",
    completed: "Workflow completed",
    failed: "Workflow failed",
    stopped: "Workflow stopped",
    budgetExhausted: "Budget exhausted",
  };
  const chinese: Record<AgentRunStatus, string> = {
    preparing: "工作流准备中",
    running: "工作流执行中",
    waitingApproval: "工作流需要确认",
    waitingUser: "工作流等待输入",
    paused: "工作流已暂停",
    checkpointing: "正在保存检查点",
    finalizing: "正在整理回答",
    completed: "工作流已完成",
    failed: "工作流失败",
    stopped: "工作流已停止",
    budgetExhausted: "工作流预算已用尽",
  };
  return language === "en" ? english[status] : chinese[status];
}

function stepStatusLabel(step: WorkflowStep, language: "zh" | "en") {
  if (step.tool) return toolStatusLabel(step.tool.status, language);
  const labels = language === "en"
    ? { pending: "Pending", running: "Running", completed: "Completed", failed: "Failed", rejected: "Rejected", stopped: "Stopped" }
    : { pending: "等待中", running: "进行中", completed: "已完成", failed: "失败", rejected: "已拒绝", stopped: "已停止" };
  return labels[step.status];
}

function toolNameLabel(name: string, language: "zh" | "en") {
  if (name === "search_tools") return language === "en" ? "Search tool catalog" : "搜索工具目录";
  if (name === "search_skills") return language === "en" ? "Search skill catalog" : "搜索技能目录";
  if (name === "skill") return language === "en" ? "Load skill" : "加载技能";
  if (name === "read_attachment_text") return language === "en" ? "Read text attachment" : "读取文本附件";
  if (name === "read_pdf_pages") return language === "en" ? "Read PDF pages" : "读取 PDF 页面";
  if (name === "read_docx_blocks") return language === "en" ? "Read DOCX blocks" : "读取 DOCX 内容";
  if (name === "read_xlsx_rows") return language === "en" ? "Read spreadsheet rows" : "读取表格行";
  if (name === "memory_read") return language === "en" ? "Read memory" : "读取记忆";
  if (name === "memory_search") return language === "en" ? "Search memory" : "搜索记忆";
  if (name === "memory_modify") return language === "en" ? "Update memory" : "更新记忆";
  return name;
}

function toolStatusLabel(status: ToolTrace["status"], language: "zh" | "en") {
  if (status === "awaitingApproval") return language === "en" ? "Awaiting approval" : "等待确认";
  if (status === "running") return language === "en" ? "Running" : "执行中";
  if (status === "completed") return language === "en" ? "Completed" : "已完成";
  if (status === "rejected") return language === "en" ? "Rejected" : "已拒绝";
  return language === "en" ? "Failed" : "失败";
}

function skillActivationLabel(activation: "manual" | "slash" | "model", language: "zh" | "en") {
  if (activation === "slash") return language === "en" ? "Slash command" : "Slash 激活";
  if (activation === "model") return language === "en" ? "Model selected" : "模型按需加载";
  return language === "en" ? "Manually selected" : "手动启用";
}

function formatDuration(value: number) {
  return value < 1_000 ? `${Math.round(value)} ms` : `${(value / 1_000).toFixed(1)} s`;
}
