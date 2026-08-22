import { useMemo, useState } from "react";
import {
  AlertCircle,
  BrainCircuit,
  CheckCircle2,
  ChevronDown,
  CircleStop,
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
  const hasToolFailures = projection.toolOutcomes.failed > 0;
  const toolSummary = summary.toolCallCount > 0
    ? hasToolFailures
      ? language === "en"
        ? `Tools: ${projection.toolOutcomes.succeeded} succeeded · ${projection.toolOutcomes.failed} failed`
        : `工具：${projection.toolOutcomes.succeeded} 成功 · ${projection.toolOutcomes.failed} 失败`
      : t("chat.workflowTools", { count: summary.toolCallCount })
    : null;
  const summaryParts = [
    projection.steps.length > 0 ? t("chat.workflowSteps", { count: summary.stepCount }) : null,
    toolSummary,
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
      className={`agent-workflow agent-workflow-${projection.status}${hasToolFailures ? " has-issues" : ""}${open ? " is-open" : ""}`}
      aria-label={t("chat.workflow")}
    >
      <button
        className="agent-workflow-summary"
        type="button"
        aria-expanded={open}
        onClick={() => onOpenChange(!open)}
      >
        <span className="agent-workflow-status-icon" aria-hidden="true">
          <WorkflowStatusIcon status={projection.status} hasIssues={hasToolFailures} />
        </span>
        <span className="agent-workflow-summary-copy">
          <strong>{activityStatusLabel(projection, language)}</strong>
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
          <div className="agent-workflow-reasoning">
            <pre>{step.reasoning}</pre>
            {step.reasoningLabel === "summary" ? (
              <small className="agent-workflow-reasoning-note">
                {language === "en"
                  ? "The provider exposed a reasoning summary, not its hidden reasoning tokens."
                  : "该协议返回的是供应商提供的思考摘要，不是模型内部隐藏的完整推理 Token。"}
              </small>
            ) : null}
          </div>
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

function WorkflowStatusIcon({ status, hasIssues = false }: { status: AgentRunStatus; hasIssues?: boolean }) {
  if (status === "completed" && hasIssues) return <AlertCircle size={16} />;
  if (status === "completed") return <CheckCircle2 size={16} />;
  if (status === "waitingApproval" || status === "waitingUser") return <ShieldAlert size={16} />;
  if (status === "failed" || status === "budgetExhausted") return <AlertCircle size={16} />;
  if (status === "stopped" || status === "paused") return <CircleStop size={16} />;
  return <LoaderCircle className="agent-workflow-spin" size={16} />;
}

function StepIcon({ step }: { step: WorkflowStep }) {
  if (step.kind === "reasoning") return <BrainCircuit size={14} />;
  if (step.kind === "skill") return <Sparkles size={14} />;
  return <Wrench size={14} />;
}

function activityStatusLabel(projection: ReturnType<typeof projectAgentWorkflow>, language: "zh" | "en") {
  const { status, steps } = projection;
  const latestActiveStep = [...steps].reverse().find((step) => step.status === "running" || step.status === "pending");
  if (status === "running" || status === "preparing" || status === "checkpointing") {
    if (latestActiveStep?.kind === "reasoning") return language === "en" ? "Thinking" : "正在思考";
    if (latestActiveStep?.kind === "skill") return language === "en" ? `Using skill: ${latestActiveStep.title}` : `正在使用技能：${latestActiveStep.title}`;
    if (latestActiveStep?.kind === "tool") {
      const name = toolNameLabel(latestActiveStep.title, language);
      return language === "en" ? `Calling: ${name}` : `正在调用：${name}`;
    }
  }
  if (status === "completed") {
    if (projection.toolOutcomes.failed > 0) {
      return language === "en"
        ? `Answer completed, but ${projection.toolOutcomes.failed} tool call${projection.toolOutcomes.failed === 1 ? "" : "s"} failed`
        : `回答已完成，但 ${projection.toolOutcomes.failed} 个工具调用失败`;
    }
    const kinds = new Set(steps.map((step) => step.kind));
    if (kinds.size === 1 && kinds.has("reasoning")) return language === "en" ? "Thinking completed" : "思考已完成";
    if (kinds.size === 1 && kinds.has("skill")) return language === "en" ? "Skills completed" : "技能使用已完成";
    if (kinds.size === 1 && kinds.has("tool")) return language === "en" ? "Tool calls completed" : "工具调用已完成";
    return language === "en" ? "Thinking and calls completed" : "思考与调用已完成";
  }
  const statusValue = status;
  const english: Record<AgentRunStatus, string> = {
    preparing: "Processing",
    running: "Thinking and using tools",
    waitingApproval: "Approval required",
    waitingUser: "Waiting for input",
    paused: "Processing paused",
    checkpointing: "Saving checkpoint",
    finalizing: "Preparing the answer",
    completed: "Thinking completed",
    failed: "Processing failed",
    stopped: "Processing stopped",
    budgetExhausted: "Processing limit reached",
  };
  const chinese: Record<AgentRunStatus, string> = {
    preparing: "处理中",
    running: "正在思考与调用",
    waitingApproval: "需要确认",
    waitingUser: "等待输入",
    paused: "处理已暂停",
    checkpointing: "正在保存检查点",
    finalizing: "正在整理回答",
    completed: "思考已完成",
    failed: "处理失败",
    stopped: "处理已停止",
    budgetExhausted: "处理次数已用尽",
  };
  return language === "en" ? english[statusValue] : chinese[statusValue];
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
  if (name === "inspect_tool") return language === "en" ? "Inspect tool contract" : "查看工具契约";
  if (name === "search_skills") return language === "en" ? "Search skill catalog" : "搜索技能目录";
  if (name === "inspect_skill") return language === "en" ? "Inspect skill" : "查看技能说明";
  if (name === "activate_skill" || name === "skill") return language === "en" ? "Load skill" : "加载技能";
  if (name === "read_skill_resource") return language === "en" ? "Read skill resource" : "读取技能资源";
  if (name === "read_attachment_text") return language === "en" ? "Read text attachment" : "读取文本附件";
  if (name === "read_pdf_pages") return language === "en" ? "Read PDF pages" : "读取 PDF 页面";
  if (name === "read_docx_blocks") return language === "en" ? "Read DOCX blocks" : "读取 DOCX 内容";
  if (name === "read_xlsx_rows") return language === "en" ? "Read spreadsheet rows" : "读取表格行";
  if (name === "memory_read") return language === "en" ? "Read memory" : "读取记忆";
  if (name === "memory_search") return language === "en" ? "Search memory" : "搜索记忆";
  if (name === "memory_modify") return language === "en" ? "Update memory" : "更新记忆";
  if (name === "workspace_list") return language === "en" ? "List workspace" : "列出工作区";
  if (name === "workspace_glob") return language === "en" ? "Find workspace files" : "查找工作区文件";
  if (name === "workspace_search") return language === "en" ? "Search workspace" : "搜索工作区";
  if (name === "workspace_read") return language === "en" ? "Read workspace file" : "读取工作区文件";
  if (name === "knowledge_list") return language === "en" ? "List knowledge base" : "列出知识库";
  if (name === "knowledge_search") return language === "en" ? "Search knowledge base" : "搜索知识库";
  if (name === "knowledge_read") return language === "en" ? "Read knowledge source" : "读取知识来源";
  if (name === "web_search") return language === "en" ? "Search the web" : "搜索网页";
  if (name === "web_fetch") return language === "en" ? "Read web page" : "读取网页";
  if (name === "present_artifact") return language === "en" ? "Prepare artifact" : "整理交付内容";
  if (name === "note_list") return language === "en" ? "List notes" : "列出笔记";
  if (name === "note_read") return language === "en" ? "Read note" : "读取笔记";
  if (name === "note_create") return language === "en" ? "Create note" : "创建笔记";
  if (name === "note_update") return language === "en" ? "Update note" : "更新笔记";
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
  if (value < 1) return "<1 ms";
  return value < 1_000 ? `${Math.round(value)} ms` : `${(value / 1_000).toFixed(1)} s`;
}
