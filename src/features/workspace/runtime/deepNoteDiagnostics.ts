import type {
  DeepNoteRunDetail,
  NotePipelineActivity,
  NotePipelineEventRecord,
  NotePipelinePhase,
} from "../../chat/api/notePipeline";

export type DeepNoteWorkflowStepId =
  | "preflight"
  | "context"
  | "planning"
  | "review"
  | "execution"
  | "saving";

export type DeepNoteWorkflowStatus =
  | "pending"
  | "active"
  | "completed"
  | "paused"
  | "failed"
  | "stopped";

export interface DeepNoteWorkflowStep {
  id: DeepNoteWorkflowStepId;
  label: string;
  description: string;
  status: DeepNoteWorkflowStatus;
}

export interface DeepNoteRuntimeDiagnosis {
  tone: "active" | "warning" | "danger" | "muted" | "success";
  title: string;
  detail: string;
  elapsedSeconds: number | null;
  timeoutSeconds: number | null;
}

const EXECUTION_PHASES = new Set<NotePipelinePhase>([
  "compiling",
  "queued",
  "drafting",
  "validating",
  "replanning",
]);

const SAVING_PHASES = new Set<NotePipelinePhase>(["assembling", "persisting"]);

function terminalStatus(phase: NotePipelinePhase): DeepNoteWorkflowStatus | null {
  if (phase === "error") return "failed";
  if (phase === "cancelled") return "stopped";
  if (phase === "cancelling") return "stopped";
  return null;
}

export function buildDeepNoteWorkflow(
  detail: DeepNoteRunDetail | null,
  phase: NotePipelinePhase,
): DeepNoteWorkflowStep[] {
  const hasSnapshot = Boolean(detail?.inputSnapshot && detail.preflight);
  const coverageComplete = Boolean(detail?.contextBudget.coverageComplete);
  const hasOutline = Boolean(detail?.planVersion?.plan);
  const planConfirmed = Boolean(detail?.planVersion?.confirmedAt);
  const executionComplete = SAVING_PHASES.has(phase) || phase === "done";
  const savingComplete = phase === "done";

  let current: DeepNoteWorkflowStepId = "preflight";
  if (hasSnapshot) current = "context";
  if (coverageComplete) current = "planning";
  if (hasOutline) current = "review";
  if (planConfirmed) current = "execution";
  if (executionComplete) current = "saving";

  const stopped = terminalStatus(phase);
  const currentStatus: DeepNoteWorkflowStatus = stopped
    ?? (phase === "paused" ? "paused" : "active");
  const completed = (id: DeepNoteWorkflowStepId) => {
    if (id === "preflight") return hasSnapshot;
    if (id === "context") return coverageComplete;
    if (id === "planning") return hasOutline;
    if (id === "review") return planConfirmed;
    if (id === "execution") return executionComplete;
    return savingComplete;
  };
  const status = (id: DeepNoteWorkflowStepId): DeepNoteWorkflowStatus => {
    if (completed(id)) return "completed";
    if (id === current) return currentStatus;
    return "pending";
  };

  const context = detail?.contextBudget;
  const totalSections = detail?.run.selectedSectionIds.length
    || detail?.planVersion?.plan.sections.length
    || 0;
  const processedSections = (detail?.run.completedSectionIds.length ?? 0)
    + (detail?.run.failedSectionIds.length ?? 0);

  return [
    {
      id: "preflight",
      label: "输入与能力预检",
      description: hasSnapshot
        ? `${detail?.inputSnapshot?.messageIds.length ?? 0} 条消息，${detail?.inputSnapshot?.attachmentIds.length ?? 0} 个附件`
        : "正在冻结输入并检查模型能力",
      status: status("preflight"),
    },
    {
      id: "context",
      label: "准备规划输入",
      description: coverageComplete
        ? `${context?.processedMessageCount ?? 0}/${context?.totalMessageCount ?? 0} 条消息已纳入规划输入`
        : context && context.chunkCount > 0
          ? `正在处理来源分块 ${context.processedChunkCount}/${context.chunkCount}`
          : "正在计算上下文预算与分块策略",
      status: status("context"),
    },
    {
      id: "planning",
      label: "生成知识结构与提纲",
      description: hasOutline
        ? `已生成 ${detail?.planVersion?.plan.sections.length ?? 0} 个章节`
        : coverageComplete
          ? "规划输入已就绪，正在等待模型返回结构化提纲"
          : "将在规划输入准备完成后开始",
      status: status("planning"),
    },
    {
      id: "review",
      label: "确认语义计划",
      description: planConfirmed
        ? "章节范围已确认，执行计划已锁定"
        : hasOutline
          ? "等待确认章节范围或调整提纲"
          : "提纲生成后由你确认",
      status: status("review"),
    },
    {
      id: "execution",
      label: "生成并验证章节",
      description: totalSections > 0
        ? `${processedSections}/${totalSections} 个章节已处理`
        : EXECUTION_PHASES.has(phase)
          ? "正在编译并执行章节任务"
          : "确认计划后开始章节生成",
      status: status("execution"),
    },
    {
      id: "saving",
      label: "组装并保存笔记",
      description: savingComplete
        ? "笔记、来源与运行记录已保存"
        : SAVING_PHASES.has(phase)
          ? "正在合并章节并写入笔记库"
          : "全部章节处理后执行",
      status: status("saving"),
    },
  ];
}

export function diagnoseDeepNoteRuntime(
  phase: NotePipelinePhase,
  activity: NotePipelineActivity | null | undefined,
  updatedAt: number | undefined,
  now: number,
): DeepNoteRuntimeDiagnosis {
  if (phase === "done") {
    return { tone: "success", title: "运行完成", detail: "笔记已经写入笔记库。", elapsedSeconds: null, timeoutSeconds: null };
  }
  if (phase === "error") {
    return { tone: "danger", title: "运行失败", detail: "查看下方运行记录可定位最后一个成功步骤和失败原因。", elapsedSeconds: null, timeoutSeconds: null };
  }
  if (phase === "cancelled") {
    return { tone: "warning", title: "运行已停止", detail: "已经完成的检查点会保留。", elapsedSeconds: null, timeoutSeconds: null };
  }
  if (phase === "cancelling") {
    return {
      tone: "warning",
      title: "停止请求已发送",
      detail: "正在等待后台任务释放资源；超过安全期限后会自动强制终止，并保留诊断记录。",
      elapsedSeconds: updatedAt ? Math.max(0, Math.floor((now - updatedAt) / 1_000)) : 0,
      timeoutSeconds: null,
    };
  }
  if (phase === "paused") {
    return { tone: "warning", title: "运行已暂停", detail: "继续后将从已保存的检查点恢复。", elapsedSeconds: null, timeoutSeconds: null };
  }
  if (phase === "awaitingOutline") {
    return { tone: "muted", title: "等待你的确认", detail: "模型调用已经结束，确认提纲后才会生成章节。", elapsedSeconds: null, timeoutSeconds: null };
  }
  if (activity) {
    const elapsedSeconds = Math.max(0, Math.floor((now - activity.startedAt) / 1_000));
    const timeoutSeconds = Math.max(0, Math.ceil((activity.timeoutMs - (now - activity.startedAt)) / 1_000));
    if (activity.kind === "retryWait") {
      return {
        tone: "warning",
        title: `第 ${activity.attempt} 次请求等待重试`,
        detail: activity.lastError ?? "上一次模型请求失败，正在按重试策略等待。",
        elapsedSeconds,
        timeoutSeconds,
      };
    }
    return {
      tone: "active",
      title: `模型请求进行中 · 第 ${activity.attempt}/${activity.maxRetries + 1} 次`,
      detail: `当前请求仍在 ${Math.ceil(activity.timeoutMs / 1_000)} 秒的超时窗口内；超时后会记录失败并按策略重试。`,
      elapsedSeconds,
      timeoutSeconds,
    };
  }
  const quietSeconds = updatedAt ? Math.max(0, Math.floor((now - updatedAt) / 1_000)) : 0;
  if (quietSeconds >= 30) {
    return {
      tone: "warning",
      title: `${quietSeconds} 秒没有收到新事件`,
      detail: "当前可能停在本地处理、数据库写入或事件同步；运行记录中的最后一项就是排查起点。",
      elapsedSeconds: quietSeconds,
      timeoutSeconds: null,
    };
  }
  return {
    tone: "active",
    title: "本地步骤进行中",
    detail: "正在准备输入、编译计划或保存检查点；收到下一条事件后会自动更新。",
    elapsedSeconds: quietSeconds,
    timeoutSeconds: null,
  };
}

function payload(record: NotePipelineEventRecord): Record<string, unknown> {
  try {
    const parsed = JSON.parse(record.payloadJson) as unknown;
    return parsed && typeof parsed === "object" ? parsed as Record<string, unknown> : {};
  } catch {
    return {};
  }
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function number(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function modelStage(value: unknown): string {
  switch (text(value)) {
    case "deepNoteChunk": return "来源分块提取";
    case "deepNoteChunkRepair": return "来源分块 JSON 修复";
    case "deepNoteOutlineDirect": return "直接生成提纲";
    case "deepNoteOutline": return "知识账本汇总提纲";
    case "deepNoteOutlineFallback": return "精简账本提纲";
    default: return "模型调用";
  }
}

function modelCallMetrics(data: Record<string, unknown>, includeResponse: boolean): string {
  const metrics = [`耗时 ${formatDuration(number(data.durationMs) ?? 0)}`];
  const inputChars = number(data.inputChars);
  const responseChars = number(data.responseChars);
  const maxOutputTokens = number(data.maxOutputTokens);
  if (inputChars !== null) metrics.push(`输入 ${inputChars} 字符`);
  if (includeResponse && responseChars !== null) metrics.push(`返回 ${responseChars} 字符`);
  if (maxOutputTokens !== null) metrics.push(`输出上限 ${maxOutputTokens} Token`);
  return metrics.join(" · ");
}

function modelErrorGuidance(data: Record<string, unknown>): string | null {
  const kind = text(data.errorKind)?.toLowerCase();
  const status = number(data.statusCode);
  if (kind?.includes("concurrency") || status === 429 && text(data.message)?.toLowerCase().includes("concurr")) {
    return "账户并发额度已用尽：等待冷却后重试，或切换备用模型。";
  }
  if (kind?.includes("providerunavailable") || status === 503 || status === 403 && /balance|quota|insufficient/i.test(text(data.message) ?? "")) {
    return "当前模型没有可用渠道：不要连续重试同一模型，建议切换备用模型。";
  }
  if (kind?.includes("upstreamtimeout") || status === 504 || status === 524) {
    return "中转站或模型服务处理超时：已保留检查点，可缩小请求或切换模型。";
  }
  if (kind?.includes("clienttimeout")) {
    return "客户端等待窗口已到：可以从当前检查点继续，不需要重新解析全部会话。";
  }
  return null;
}

export function describeNotePipelineEvent(record: NotePipelineEventRecord): { label: string; detail: string } {
  const data = payload(record);
  const activity = data.activity && typeof data.activity === "object"
    ? data.activity as Record<string, unknown>
    : {};
  switch (record.eventType) {
    case "preflightCompleted":
      return { label: "输入预检完成", detail: "模型能力、消息与附件检查通过" };
    case "phaseProgress":
      return { label: "阶段更新", detail: text(data.message) ?? text(data.phase) ?? "运行状态已更新" };
    case "contextChunkCompleted":
      return {
        label: "来源分块完成",
        detail: `分块 ${number(data.chunkIndex) ?? "-"}/${number(data.chunkCount) ?? "-"}，已覆盖 ${number(data.processedMessageCount) ?? 0} 条消息`,
      };
    case "contextCoverageCompleted":
      return {
        label: "规划输入准备完成",
        detail: `${number(data.processedMessageCount) ?? 0}/${number(data.totalMessageCount) ?? 0} 条消息已纳入；${data.mode === "chunked" ? "分块账本" : "直接规划"}`,
      };
    case "modelCallStarted":
      return {
        label: "模型请求开始",
        detail: `${text(data.message) ?? modelStage(activity.operation)} · 超时 ${Math.ceil((number(activity.timeoutMs) ?? 0) / 1_000)} 秒`,
      };
    case "modelRetryScheduled":
      return { label: "模型请求准备重试", detail: text(activity.lastError) ?? text(data.message) ?? "等待重试" };
    case "modelCallCompleted":
      return {
        label: `${modelStage(data.operation)}完成`,
        detail: modelCallMetrics(data, true),
      };
    case "modelCallFailed":
      return {
        label: `${modelStage(data.operation)}失败`,
        detail: [
          text(data.message) ?? text(data.errorKind) ?? "未知模型错误",
          modelErrorGuidance(data),
          number(data.statusCode) !== null ? `HTTP ${number(data.statusCode)}` : null,
          number(data.inputChars) !== null ? `输入 ${number(data.inputChars)} 字符` : null,
          number(data.maxOutputTokens) !== null ? `输出上限 ${number(data.maxOutputTokens)} Token` : null,
        ].filter(Boolean).join(" · "),
      };
    case "outlineReady":
      return { label: "知识结构生成完成", detail: `提纲包含 ${number(data.sectionCount) ?? 0} 个章节` };
    case "planConfirmed":
      return { label: "语义计划已确认", detail: `计划版本 v${number(data.version) ?? 1}` };
    case "sectionUsingChunkLedger":
      return { label: "章节使用知识账本", detail: `引用 ${number(data.sourceMessageCount) ?? 0} 条来源消息` };
    case "sectionCompleted":
      return { label: text(data.heading) ?? "章节完成", detail: `尝试 ${number(data.attemptCount) ?? 0} 次，修订 ${number(data.revisionCount) ?? 0} 次` };
    case "sectionFailed":
      return { label: `${text(data.heading) ?? "章节"}生成失败`, detail: text(data.message) ?? "未通过验证" };
    case "runPaused":
      return { label: "任务暂停", detail: "当前请求已中断，检查点已保留" };
    case "runCancellationRequested":
      return { label: "已请求停止任务", detail: "正在等待后台任务协作退出" };
    case "runCancelled":
      return {
        label: data.forced ? "任务已强制停止" : "任务已停止",
        detail: [
          text(data.reason) ?? "检查点已保留",
          text(data.diagnosticPath) ? `诊断日志：${text(data.diagnosticPath)}` : null,
        ].filter(Boolean).join(" · "),
      };
    case "runPanicked":
      return {
        label: "后台任务发生 panic",
        detail: [text(data.message) ?? "任务异常终止", text(data.diagnosticPath)].filter(Boolean).join(" · "),
      };
    case "runResumed":
      return { label: "任务继续", detail: "从最近检查点恢复" };
    case "runContinued":
      return {
        label: "已从停止点继续",
        detail: `恢复执行版本 v${number(data.executionVersion) ?? "-"}，保留已有检查点`,
      };
    case "runRetryRequested":
      return {
        label: "已重试失败步骤",
        detail: `恢复执行版本 v${number(data.executionVersion) ?? "-"}，失败章节和节点已重置`,
      };
    case "runRestarted":
      return {
        label: "已重新生成",
        detail: text(data.newRunId) ? `新任务 ${text(data.newRunId)}` : "已使用当前会话创建新任务",
      };
    case "runCompleted":
      return { label: "深度笔记完成", detail: `${number(data.completedSectionCount) ?? 0} 个章节已完成` };
    case "runFailed":
      return { label: "任务失败", detail: text(data.message) ?? "查看上一条事件定位失败步骤" };
    default:
      return { label: record.eventType, detail: text(data.message) ?? (record.nodeId ? `节点 ${record.nodeId}` : "运行事件") };
  }
}

export function formatDuration(milliseconds: number): string {
  if (milliseconds < 1_000) return `${Math.max(0, Math.round(milliseconds))} 毫秒`;
  const seconds = Math.round(milliseconds / 1_000);
  if (seconds < 60) return `${seconds} 秒`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes} 分 ${seconds % 60} 秒`;
}
