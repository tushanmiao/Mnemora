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

const HOUR_MS = 60 * 60 * 1_000;
const DAY_MS = 24 * HOUR_MS;
/**
 * 桶的粒度按「读起来清楚」来定，而不是等于天数。
 * 旧实现用 bucketCount = 天数，因此「1 天」只有一个桶——整张趋势图
 * 是一根必然 100% 高的柱子，看不出任何信息。
 */
const RANGE_OPTIONS = [
  { id: "today", label: "今天", bucketMs: HOUR_MS, buckets: 24 },
  { id: "24h", label: "24 小时", bucketMs: HOUR_MS, buckets: 24 },
  { id: "7", label: "7 天", bucketMs: 6 * HOUR_MS, buckets: 28 },
  { id: "30", label: "30 天", bucketMs: DAY_MS, buckets: 30 },
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
    const option = RANGE_OPTIONS.find((item) => item.id === range) ?? RANGE_OPTIONS[1];
    const sinceMs = range === "today"
      ? new Date(new Date().getFullYear(), new Date().getMonth(), new Date().getDate()).getTime()
      : now - option.bucketMs * option.buckets;
    const [selectedProviderId, selectedModelId] = modelKey.split("|");
    return {
      sinceMs,
      untilMs: now + 1,
      bucketMs: option.bucketMs,
      bucketCount: option.buckets,
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

  const stats = summary.summary;
  // Token 构成是「组成关系」，用一根堆叠条比四个孤立数字更容易读懂。
  const cachedInput = Math.min(stats.cacheReadTokens, stats.inputTokens);
  const freshInput = Math.max(0, stats.inputTokens - cachedInput);
  const visibleOutput = Math.max(0, stats.outputTokens - stats.reasoningTokens);
  const compositionTotal = Math.max(1, freshInput + cachedInput + visibleOutput + stats.reasoningTokens);
  // 配色编码的是两层归属：输入两段同为蓝相、输出两段同为橙相，次级项压一档明度。
  // 旧写法借用 --workspace-* 语义色，回答↔思考只有 ΔE 7.5、缓存读取对表面 1.16:1，
  // 那才是「四段区分不明显」的根因。取值与实测数据见 tokens.css。
  const composition = [
    { key: "fresh", label: "新输入", value: freshInput, color: "var(--chart-series-1)" },
    { key: "cached", label: "缓存读取", value: cachedInput, color: "var(--chart-series-1-sub)" },
    { key: "answer", label: "回答", value: visibleOutput, color: "var(--chart-series-2)" },
    { key: "reasoning", label: "思考", value: stats.reasoningTokens, color: "var(--chart-series-2-sub)" },
  ].filter((part) => part.value > 0);

  const failedRequests = stats.failedRequests + stats.stoppedRequests;
  // 数据质量是「这些数字有多可信」的元信息，平时应当安静；只有出问题才需要抬头。
  const qualityIssues = Math.max(stats.missingUsageRequests, stats.missingCostRequests, stats.partialCostRequests);
  const averageCostPerRequest = stats.totalRequests > 0 && typeof stats.totalCostUsd === "number"
    ? stats.totalCostUsd / stats.totalRequests
    : null;
  const rangeLabel = RANGE_OPTIONS.find((item) => item.id === range)?.label ?? "";


  return (
    <section className="settings-content usage-settings-content">
      <div className="settings-content-heading usage-settings-heading">
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

        <div className="usage-hero" aria-busy={loading}>
          <div className="usage-hero-item usage-hero-primary">
            <span>成本</span>
            <strong>{formatCost(stats.totalCostUsd)}</strong>
            <em>{rangeLabel}内</em>
          </div>
          <div className="usage-hero-item">
            <span>总 Token</span>
            <strong>{formatNumber(stats.totalTokens)}</strong>
            <em>{formatNumber(stats.totalRequests)} 次请求</em>
          </div>
          <div className="usage-hero-item">
            <span>成功率</span>
            <strong>{(successRate * 100).toFixed(1)}%</strong>
            <em>{failedRequests > 0 ? `${failedRequests} 次未完成` : "全部完成"}</em>
          </div>
          <div className="usage-hero-item">
            <span>缓存命中</span>
            <strong>{(cacheRate * 100).toFixed(1)}%</strong>
            <em>省下 {formatNumber(stats.cacheReadTokens)} 输入 Token</em>
          </div>
        </div>

        <section className="usage-section usage-section-flush">
          <div className="usage-section-heading"><h3>Token 构成</h3><span>缓存读取计入输入，思考 Token 计入输出</span></div>
          <div className="usage-composition">
            <div className="usage-composition-bar" role="img" aria-label="Token 构成">
              {composition.map((part) => (
                <span
                  key={part.key}
                  style={{ width: `${(part.value / compositionTotal) * 100}%`, background: part.color }}
                  title={`${part.label}：${formatNumber(part.value)}`}
                />
              ))}
            </div>
            <ul className="usage-composition-legend">
              {composition.map((part) => (
                <li key={part.key}>
                  <i style={{ background: part.color }} />
                  <span>{part.label}</span>
                  <b>{formatNumber(part.value)}</b>
                  <em>{((part.value / compositionTotal) * 100).toFixed(0)}%</em>
                </li>
              ))}
              {composition.length === 0 ? <li className="usage-empty-inline">还没有 Token 记录</li> : null}
            </ul>
          </div>
          <div className="usage-inline-stats">
            <div><span>平均首字</span><b>{formatDuration(stats.averageTimeToFirstTokenMs)}</b></div>
            <div><span>平均速度</span><b>{formatSpeed(stats.averageOutputTokensPerSecond)}</b></div>
            <div><span>缓存创建</span><b>{formatNumber(stats.cacheWriteTokens)}</b></div>
            <div><span>平均单次成本</span><b>{formatCost(averageCostPerRequest)}</b></div>
          </div>
        </section>

        <details className={`usage-quality${qualityIssues > 0 ? " usage-quality-warn" : ""}`}>
          <summary>
            <span className="usage-quality-dot" aria-hidden="true" />
            <span className="usage-quality-text">
              {qualityIssues > 0
                ? `${formatNumber(qualityIssues)} 次请求缺少完整的用量或成本数据`
                : `全部 ${formatNumber(stats.totalRequests)} 次请求都有可用的用量数据`}
            </span>
            <span className="usage-quality-rate">{(knownUsageRate * 100).toFixed(1)}%</span>
          </summary>
          <div className="usage-quality-detail">
            <div><span>Usage 可用率</span><b>{(knownUsageRate * 100).toFixed(1)}%</b></div>
            <div><span>官方 / 网关覆盖</span><b>{(reportedUsageRate * 100).toFixed(1)}%</b></div>
            <div><span>本地估算</span><b>{formatNumber(stats.estimatedUsageRequests)}</b></div>
            <div><span>部分 / 估算成本</span><b>{formatNumber(stats.partialCostRequests)}</b></div>
            <div><span>Usage 缺失</span><b>{formatNumber(stats.missingUsageRequests)}</b></div>
            <div><span>成本缺失</span><b>{formatNumber(stats.missingCostRequests)}</b></div>
          </div>
        </details>

        <section className="usage-section">
          <div className="usage-section-heading">
            <h3>Token 趋势</h3>
            <span>同一 Agent Run 的每次真实模型调用分别计入</span>
          </div>
          <div className="usage-chart">
            <div className="usage-chart-axis" aria-hidden="true">
              <span>{formatAxisTick(maxTrendTokens, maxTrendTokens)}</span>
              <span>{formatAxisTick(maxTrendTokens / 2, maxTrendTokens)}</span>
              <span>0</span>
            </div>
            <div className="usage-chart-plot" aria-label="Token 趋势图">
              {summary.trend.length === 0 ? <div className="usage-empty">当前范围内还没有用量记录</div> : summary.trend.map((point, index) => {
                const inputShare = point.totalTokens > 0 ? point.inputTokens / point.totalTokens : 0;
                const height = (point.totalTokens / maxTrendTokens) * 100;
                const tick = range === "today" || range === "24h" ? formatHour(point.startedAtMs) : formatShortDate(point.startedAtMs);
                // 桶多的时候只标少数几个刻度，避免轴线糊成一片。
                const tickStep = Math.ceil(summary.trend.length / 6);
                return (
                  <div className="usage-chart-column" key={point.bucketIndex}>
                    <div
                      className="usage-chart-bar"
                      title={`${formatDate(point.startedAtMs)}\n${formatNumber(point.totalTokens)} tokens · ${point.requests} 次 · ${formatCost(point.costUsd)}`}
                    >
                      {point.totalTokens > 0 ? (
                        <span className="usage-chart-stack" style={{ height: `${Math.max(1.5, height)}%` }}>
                          <i style={{ height: `${(1 - inputShare) * 100}%` }} />
                          <u style={{ height: `${inputShare * 100}%` }} />
                        </span>
                      ) : null}
                    </div>
                    <small>{index % tickStep === 0 ? tick : ""}</small>
                  </div>
                );
              })}
            </div>
          </div>
          <div className="usage-chart-legend">
            <span><i className="usage-swatch-input" />输入</span>
            <span><i className="usage-swatch-output" />输出</span>
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
            {distributionRows.length === 0 ? <div className="usage-empty">当前范围内还没有用量记录</div> : distributionRows.map((item, index) => (
              <div className="usage-model-row" key={item.id}>
                <div><strong>{item.label}</strong><span>{item.providerName} · {item.requestCount} 次 · {formatCost(item.costUsd)}</span></div>
                <div className="usage-model-meter"><span style={{ width: `${Math.max(1, item.totalTokens / maxDistributionTokens * 100)}%` }} data-rank={index % 8} /></div>
                <b>{formatNumber(item.totalTokens)}</b>
              </div>
            ))}
          </div>
        </section>

        <section className="usage-section usage-records-section">
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

/** 三个刻度必须用同一种记数法，混用「15.8万」和「79,136」根本没法比较。 */
function formatAxisTick(value: number, max: number) {
  return new Intl.NumberFormat("zh-CN", {
    notation: max >= 100_000 ? "compact" : "standard",
    maximumFractionDigits: 1,
  }).format(value);
}

function statusLabel(status: string) { return status === "success" ? "成功" : status === "stopped" ? "已停止" : "失败"; }
