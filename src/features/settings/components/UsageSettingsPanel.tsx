import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertCircle, RefreshCw, Trash2 } from "lucide-react";
import { clearUsageStats, loadUsageRecords, loadUsageSummary } from "../api/usage";
import {
  createEmptyUsageRecords,
  createEmptyUsageSummary,
  type UsageRecordsPage,
  type UsageStatsQuery,
  type UsageSummaryResponse,
} from "../../../types/usage";
import {
  formatCost,
  formatDate,
  formatDuration,
  formatHour,
  formatNumber,
  formatOptional,
  formatShortDate,
  formatSpeed,
} from "../utils/usageFormatters";
import "../styles/usage-settings.css";

const DAY_MS = 24 * 60 * 60 * 1_000;
const RANGE_OPTIONS = [
  { id: "today", label: "今天" },
  { id: "1", label: "1 天" },
  { id: "7", label: "7 天" },
  { id: "30", label: "30 天" },
] as const;

type RangeId = typeof RANGE_OPTIONS[number]["id"];
type Distribution = "provider" | "model" | "operation";

export function UsageSettingsPanel() {
  const [range, setRange] = useState<RangeId>("7");
  const [distribution, setDistribution] = useState<Distribution>("model");
  const [status, setStatus] = useState("");
  const [usageSource, setUsageSource] = useState("");
  const [operation, setOperation] = useState("");
  const [providerId, setProviderId] = useState("");
  // 使用 provider|model 复合键，避免不同中转站的同名模型混在一起。
  const [modelKey, setModelKey] = useState("");
  const [summary, setSummary] = useState<UsageSummaryResponse>(createEmptyUsageSummary);
  const [page, setPage] = useState<UsageRecordsPage>(createEmptyUsageRecords);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const refreshSequenceRef = useRef(0);

  const baseQuery = useMemo<UsageStatsQuery>(() => {
    const now = Date.now();
    const sinceMs = range === "today"
      ? new Date(new Date().getFullYear(), new Date().getMonth(), new Date().getDate()).getTime()
      : now - Number(range) * DAY_MS;
    const bucketCount = range === "today" ? 24 : Number(range);
    const [selectedProviderId, selectedModelId] = modelKey.split("|");
    return {
      sinceMs,
      untilMs: now + 1,
      bucketMs: range === "today" ? 60 * 60 * 1_000 : DAY_MS,
      bucketCount,
      status: status || undefined,
      usageSource: usageSource || undefined,
      operation: operation || undefined,
      providerId: providerId || selectedProviderId || undefined,
      modelId: selectedModelId || undefined,
    } as UsageStatsQuery;
  }, [modelKey, operation, providerId, range, status, usageSource]);

  const refresh = useCallback(async () => {
    const sequence = ++refreshSequenceRef.current;
    setLoading(true);
    setError(null);
    try {
      const [nextSummary, nextPage] = await Promise.all([
        loadUsageSummary(baseQuery),
        loadUsageRecords({ ...baseQuery, limit: 50 }),
      ]);
      if (sequence === refreshSequenceRef.current) {
        setSummary(nextSummary);
        setPage(nextPage);
      }
    } catch (reason) {
      if (sequence === refreshSequenceRef.current) {
        setError(reason instanceof Error ? reason.message : String(reason));
      }
    } finally {
      if (sequence === refreshSequenceRef.current) setLoading(false);
    }
  }, [baseQuery]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (providerId && !summary.filterOptions.providers.some((item) => item.id === providerId)) {
      setProviderId("");
      setModelKey("");
    } else if (modelKey && !summary.filterOptions.models.some((item) => item.id === modelKey)) {
      setModelKey("");
    }
    if (operation && !summary.filterOptions.operations.some((item) => item.label === operation)) {
      setOperation("");
    }
  }, [modelKey, operation, providerId, summary.filterOptions]);

  const loadMore = async () => {
    if (!page.hasMore || !page.nextCursor || loadingMore) return;
    const sequence = refreshSequenceRef.current;
    setLoadingMore(true);
    setError(null);
    try {
      const next = await loadUsageRecords({
        ...baseQuery,
        cursor: page.nextCursor,
        limit: 50,
      });
      if (sequence === refreshSequenceRef.current) {
        setPage({
          ...next,
          records: [...page.records, ...next.records],
          totalMatching: next.totalMatching,
          skippedRecords: Math.max(page.skippedRecords, next.skippedRecords),
        });
      }
    } catch (reason) {
      if (sequence === refreshSequenceRef.current) {
        setError(reason instanceof Error ? reason.message : String(reason));
      }
    } finally {
      setLoadingMore(false);
    }
  };

  const clear = async () => {
    if (!window.confirm("确定清空全部本地用量记录吗？此操作无法撤销。")) return;
    setLoading(true);
    setError(null);
    try {
      await clearUsageStats();
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setLoading(false);
    }
  };

  const knownUsageRate = summary.summary.totalRequests > 0
    ? summary.summary.knownUsageRequests / summary.summary.totalRequests
    : 0;
  const reportedUsageRate = summary.summary.totalRequests > 0
    ? (summary.summary.providerReportedRequests + summary.summary.gatewayNormalizedRequests)
      / summary.summary.totalRequests
    : 0;
  const successRate = summary.summary.totalRequests > 0
    ? summary.summary.successfulRequests / summary.summary.totalRequests
    : 0;
  const cacheRate = summary.summary.inputTokens > 0
    ? summary.summary.cacheReadTokens / summary.summary.inputTokens
    : 0;
  const maxTrendTokens = Math.max(1, ...summary.trend.map((point) => point.totalTokens));
  const distributionRows = distribution === "provider"
    ? summary.providerStats
    : distribution === "operation"
      ? summary.operationStats
      : summary.modelStats;
  const maxDistributionTokens = Math.max(1, ...distributionRows.map((item) => item.totalTokens));
  const summaryItems = [
    ["总 Token", formatNumber(summary.summary.totalTokens)],
    ["成本", formatCost(summary.summary.totalCostUsd)],
    ["请求次数", formatNumber(summary.summary.totalRequests)],
    ["成功率", `${(successRate * 100).toFixed(1)}%`],
    ["输入 / 输出", `${formatNumber(summary.summary.inputTokens)} / ${formatNumber(summary.summary.outputTokens)}`],
    ["缓存命中", `${(cacheRate * 100).toFixed(1)}%`],
    ["平均首字", formatDuration(summary.summary.averageTimeToFirstTokenMs)],
    ["平均速度", formatSpeed(summary.summary.averageOutputTokensPerSecond)],
    ["思考 Token", formatNumber(summary.summary.reasoningTokens)],
    ["缓存读取", formatNumber(summary.summary.cacheReadTokens)],
    ["缓存创建", formatNumber(summary.summary.cacheWriteTokens)],
    ["Usage 可用率", `${(knownUsageRate * 100).toFixed(1)}%`],
    ["官方/网关覆盖", `${(reportedUsageRate * 100).toFixed(1)}%`],
    ["部分/估算成本", formatNumber(summary.summary.partialCostRequests)],
    ["Usage 缺失", formatNumber(summary.summary.missingUsageRequests)],
    ["成本缺失", formatNumber(summary.summary.missingCostRequests)],
  ] as const;

  return (
    <section className="settings-content usage-settings-content">
      <div className="settings-content-heading">
        <div><h2>用量</h2><span>按模型调用记录 Token、成本和性能；请求正文仍只进入调试页</span></div>
        <div className="settings-heading-actions">
          <div className="settings-segmented" aria-label="统计时间范围">
            {RANGE_OPTIONS.map((option) => (
              <button className={range === option.id ? "settings-segmented-active" : ""} type="button" key={option.id} onClick={() => setRange(option.id)}>
                {option.label}
              </button>
            ))}
          </div>
          <button className="settings-button settings-button-secondary" type="button" disabled={loading} onClick={() => void refresh()}><RefreshCw size={15} /><span>刷新</span></button>
          <button className="settings-button settings-button-secondary usage-clear-button" type="button" disabled={loading || summary.totalLogs === 0} onClick={() => void clear()}><Trash2 size={15} /><span>清空</span></button>
        </div>
      </div>

      <div className="usage-settings-scroll">
        {error ? <div className="settings-feedback settings-feedback-error"><AlertCircle size={17} /><span>{error}</span></div> : null}
        {Math.max(summary.skippedRecords, page.skippedRecords) > 0 ? (
          <div className="settings-feedback"><AlertCircle size={17} /><span>已跳过 {Math.max(summary.skippedRecords, page.skippedRecords)} 条损坏的本地记录。</span></div>
        ) : null}

        <div className="usage-summary-grid" aria-busy={loading}>
          {summaryItems.map(([label, value]) => <div className="usage-summary-item" key={label}><span>{label}</span><strong>{value}</strong></div>)}
        </div>

        <section className="usage-section">
          <div className="usage-section-heading"><h3>Token 趋势</h3><span>同一 Agent Run 的每次真实模型调用分别计入</span></div>
          <div className="usage-trend" aria-label="Token 趋势图">
            {summary.trend.map((point) => (
              <div className="usage-trend-column" key={point.bucketIndex} title={`${formatDate(point.startedAtMs)}：${formatNumber(point.totalTokens)} tokens · ${formatCost(point.costUsd)}`}>
                <div className="usage-trend-track"><span style={{ height: `${Math.max(2, point.totalTokens / maxTrendTokens * 100)}%` }} /></div>
                <small>{range === "today" ? formatHour(point.startedAtMs) : formatShortDate(point.startedAtMs)}</small>
              </div>
            ))}
          </div>
        </section>

        <section className="usage-section">
          <div className="usage-section-heading">
            <h3>分布</h3>
            <div className="settings-segmented" aria-label="用量分布维度">
              {(["provider", "model", "operation"] as const).map((item) => (
                <button className={distribution === item ? "settings-segmented-active" : ""} type="button" key={item} onClick={() => setDistribution(item)}>
                  {item === "provider" ? "供应商" : item === "model" ? "模型" : "操作"}
                </button>
              ))}
            </div>
          </div>
          <div className="usage-model-list">
            {distributionRows.length === 0 ? <div className="usage-empty">当前范围内还没有用量记录</div> : distributionRows.map((item) => (
              <div className="usage-model-row" key={item.id}>
                <div><strong>{item.label}</strong><span>{item.providerName} · {item.requestCount} 次 · {formatCost(item.costUsd)}</span></div>
                <div className="usage-model-meter"><span style={{ width: `${Math.max(1, item.totalTokens / maxDistributionTokens * 100)}%` }} /></div>
                <b>{formatNumber(item.totalTokens)}</b>
              </div>
            ))}
          </div>
        </section>

        <section className="usage-section">
          <div className="usage-section-heading"><h3>请求明细</h3><span>已加载 {page.records.length} / {page.totalMatching} 条</span></div>
          <div className="usage-filters">
            <select className="settings-input" aria-label="供应商筛选" value={providerId} onChange={(event) => { setProviderId(event.target.value); setModelKey(""); }}>
              <option value="">全部供应商</option>
              {summary.filterOptions.providers.map((item) => <option value={item.id} key={item.id}>{item.label}</option>)}
            </select>
            <select className="settings-input" aria-label="模型筛选" value={modelKey} onChange={(event) => setModelKey(event.target.value)}>
              <option value="">全部模型</option>
              {summary.filterOptions.models
                .filter((item) => !providerId || item.providerId === providerId)
                .map((item) => <option value={item.id} key={item.id}>{item.label} · {item.providerName}</option>)}
            </select>
            <select className="settings-input" aria-label="状态筛选" value={status} onChange={(event) => setStatus(event.target.value)}>
              <option value="">全部状态</option><option value="success">成功</option><option value="error">失败</option><option value="stopped">已停止</option>
            </select>
            <select className="settings-input" aria-label="Usage 来源筛选" value={usageSource} onChange={(event) => setUsageSource(event.target.value)}>
              <option value="">全部 Usage 来源</option><option value="providerReported">官方实报</option><option value="gatewayNormalized">中转归一化</option><option value="estimated">本地估算</option><option value="missing">缺失</option>
            </select>
            <select className="settings-input" aria-label="操作筛选" value={operation} onChange={(event) => setOperation(event.target.value)}>
              <option value="">全部操作</option>
              {summary.filterOptions.operations.map((item) => <option value={item.label} key={item.id}>{item.label}</option>)}
            </select>
          </div>
          <div className="usage-table-wrap">
            <table className="usage-table">
              <thead><tr><th>时间</th><th>模型 / Run</th><th>状态</th><th>输入</th><th>缓存</th><th>输出</th><th>成本</th><th>首字 / 速度</th></tr></thead>
              <tbody>
                {page.records.map((record) => (
                  <tr key={record.id} title={record.pricingSnapshot ? `价格快照：${formatDate(record.pricingSnapshot.capturedAtMs)}` : "没有价格快照"}>
                    <td>{formatDate(record.createdAtMs)}</td>
                    <td><strong>{record.displayName}</strong><span>{record.runId ? `${record.providerName} · 第 ${(record.roundIndex ?? 0) + 1} 轮` : record.providerName}</span></td>
                    <td><i className={`usage-status usage-status-${record.status}`}>{statusLabel(record.status)}</i></td>
                    <td>{formatOptional(record.inputTokens)}</td>
                    <td>{formatOptional(record.cacheReadTokens)}</td>
                    <td>{formatOptional(record.outputTokens)}</td>
                    <td>{formatCost(record.costUsd)}</td>
                    <td>{formatDuration(record.timeToFirstTokenMs)} / {formatSpeed(record.outputTokensPerSecond)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {page.records.length === 0 ? <div className="usage-empty">发送一次真实模型请求后，这里会出现记录</div> : null}
          </div>
          {page.hasMore ? <button className="settings-button settings-button-secondary usage-load-more" type="button" disabled={loadingMore} onClick={() => void loadMore()}>{loadingMore ? "加载中" : "加载更多"}</button> : null}
        </section>
      </div>
    </section>
  );
}

function statusLabel(status: string) { return status === "success" ? "成功" : status === "stopped" ? "已停止" : "失败"; }
