import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertCircle, RefreshCw, Trash2 } from "lucide-react";
import { clearUsageStats, loadUsageStats } from "../api/usage";
import { createEmptyUsageStats, type UsageStatsResponse } from "../../../types/usage";
import "../styles/usage-settings.css";

const DAY_MS = 24 * 60 * 60 * 1_000;
const RANGE_OPTIONS = [
  { days: 1, label: "今天" },
  { days: 7, label: "7 天" },
  { days: 30, label: "30 天" },
] as const;

export function UsageSettingsPanel() {
  const [days, setDays] = useState(7);
  const [stats, setStats] = useState<UsageStatsResponse>(createEmptyUsageStats);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const now = Date.now();
      const sinceMs = now - days * DAY_MS;
      setStats(await loadUsageStats({
        sinceMs,
        bucketMs: DAY_MS,
        bucketCount: days,
        limit: 100,
      }));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setLoading(false);
    }
  }, [days]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const coverage = stats.summary.totalRequests > 0
    ? stats.summary.providerReportedRequests / stats.summary.totalRequests
    : 0;
  const maxTrendTokens = Math.max(1, ...stats.trend.map((point) => point.totalTokens));
  const maxModelTokens = Math.max(1, ...stats.modelStats.map((item) => item.totalTokens));
  const summaryItems = useMemo(() => [
    ["总 Token", formatNumber(stats.summary.totalTokens)],
    ["请求次数", formatNumber(stats.summary.totalRequests)],
    ["输入 Token", formatNumber(stats.summary.inputTokens)],
    ["输出 Token", formatNumber(stats.summary.outputTokens)],
    ["Usage 覆盖率", `${(coverage * 100).toFixed(1)}%`],
    ["平均耗时", formatDuration(stats.summary.averageDurationMs)],
    ["缓存读取", formatNumber(stats.summary.cacheReadTokens)],
    ["推理 Token", formatNumber(stats.summary.reasoningTokens)],
  ] as const, [coverage, stats.summary]);

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

  return (
    <section className="settings-content usage-settings-content">
      <div className="settings-content-heading">
        <div>
          <h2>用量</h2>
          <span>本地统计模型调用和 Token 使用情况</span>
        </div>
        <div className="settings-heading-actions">
          <div className="settings-segmented" aria-label="统计时间范围">
            {RANGE_OPTIONS.map((option) => (
              <button
                className={days === option.days ? "settings-segmented-active" : ""}
                type="button"
                key={option.days}
                onClick={() => setDays(option.days)}
              >
                {option.label}
              </button>
            ))}
          </div>
          <button className="settings-button settings-button-secondary" type="button" disabled={loading} onClick={() => void refresh()}>
            <RefreshCw size={15} /><span>刷新</span>
          </button>
          <button className="settings-button settings-button-secondary usage-clear-button" type="button" disabled={loading || stats.totalLogs === 0} onClick={() => void clear()}>
            <Trash2 size={15} /><span>清空</span>
          </button>
        </div>
      </div>

      <div className="usage-settings-scroll">
        {error ? <div className="settings-feedback settings-feedback-error"><AlertCircle size={17} /><span>{error}</span></div> : null}

        <div className="usage-summary-grid" aria-busy={loading}>
          {summaryItems.map(([label, value]) => (
            <div className="usage-summary-item" key={label}>
              <span>{label}</span>
              <strong>{value}</strong>
            </div>
          ))}
        </div>

        <section className="usage-section">
          <div className="usage-section-heading">
            <h3>Token 趋势</h3>
            <span>成本暂不统计，需等模型配置加入价格后再计算</span>
          </div>
          <div className="usage-trend" aria-label="Token 趋势图">
            {stats.trend.map((point) => (
              <div className="usage-trend-column" key={point.bucketIndex} title={`${formatDate(point.startedAtMs)}：${formatNumber(point.totalTokens)} tokens`}>
                <div className="usage-trend-track">
                  <span style={{ height: `${Math.max(2, point.totalTokens / maxTrendTokens * 100)}%` }} />
                </div>
                <small>{formatShortDate(point.startedAtMs)}</small>
              </div>
            ))}
          </div>
        </section>

        <section className="usage-section">
          <div className="usage-section-heading"><h3>模型分布</h3><span>按 Token 从高到低</span></div>
          <div className="usage-model-list">
            {stats.modelStats.length === 0 ? <div className="usage-empty">当前时间范围内还没有用量记录</div> : stats.modelStats.map((item) => (
              <div className="usage-model-row" key={item.id}>
                <div><strong>{item.label}</strong><span>{item.providerName} · {item.requestCount} 次请求</span></div>
                <div className="usage-model-meter"><span style={{ width: `${Math.max(1, item.totalTokens / maxModelTokens * 100)}%` }} /></div>
                <b>{formatNumber(item.totalTokens)}</b>
              </div>
            ))}
          </div>
        </section>

        <section className="usage-section">
          <div className="usage-section-heading"><h3>请求明细</h3><span>显示最近 {stats.logs.length} / {stats.totalLogs} 条</span></div>
          <div className="usage-table-wrap">
            <table className="usage-table">
              <thead><tr><th>时间</th><th>模型</th><th>状态</th><th>输入</th><th>输出</th><th>总计</th><th>耗时</th></tr></thead>
              <tbody>
                {stats.logs.map((record) => (
                  <tr key={record.id}>
                    <td>{formatDate(record.createdAtMs)}</td>
                    <td><strong>{record.displayName}</strong><span>{record.providerName}</span></td>
                    <td><i className={`usage-status usage-status-${record.status}`}>{statusLabel(record.status)}</i></td>
                    <td>{formatOptional(record.inputTokens)}</td>
                    <td>{formatOptional(record.outputTokens)}</td>
                    <td>{record.usageSource === "missing" ? "未返回" : formatOptional(record.totalTokens)}</td>
                    <td>{formatDuration(record.durationMs)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {stats.logs.length === 0 ? <div className="usage-empty">发送一次真实模型请求后，这里会出现记录</div> : null}
          </div>
        </section>
      </div>
    </section>
  );
}

function formatNumber(value: number) {
  return new Intl.NumberFormat("zh-CN", { notation: value >= 100_000 ? "compact" : "standard", maximumFractionDigits: 1 }).format(value);
}

function formatOptional(value?: number) {
  return value === undefined ? "-" : formatNumber(value);
}

function formatDuration(value?: number) {
  if (value === undefined) return "-";
  return value < 1_000 ? `${Math.round(value)} ms` : `${(value / 1_000).toFixed(1)} s`;
}

function formatDate(value: number) {
  return new Intl.DateTimeFormat("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }).format(value);
}

function formatShortDate(value: number) {
  return new Intl.DateTimeFormat("zh-CN", { month: "numeric", day: "numeric" }).format(value);
}

function statusLabel(status: string) {
  if (status === "success") return "成功";
  if (status === "stopped") return "已停止";
  return "失败";
}
