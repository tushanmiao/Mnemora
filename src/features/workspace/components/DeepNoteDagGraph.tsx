import { useId, useMemo, useState } from "react";
import { AlertTriangle, ChevronDown, GitBranch, MousePointer2 } from "lucide-react";
import type { DeepNoteDagNode, DeepNoteDagNodeStatus } from "../../chat/api/notePipeline";

const NODE_WIDTH = 176;
const NODE_HEIGHT = 62;
const LAYER_GAP = 72;
const ROW_GAP = 16;
const GRAPH_PADDING = 22;

export interface DeepNoteDagLayoutNode {
  node: DeepNoteDagNode;
  layer: number;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface DeepNoteDagLayoutEdge {
  sourceId: string;
  targetId: string;
  path: string;
}

export interface DeepNoteDagLayout {
  width: number;
  height: number;
  nodes: DeepNoteDagLayoutNode[];
  edges: DeepNoteDagLayoutEdge[];
}

export function layoutDeepNoteDag(nodes: DeepNoteDagNode[]): DeepNoteDagLayout {
  const order = new Map(nodes.map((node, index) => [node.nodeId, index]));
  const byId = new Map(nodes.map((node) => [node.nodeId, node]));
  const indegree = new Map<string, number>();
  const dependents = new Map<string, string[]>();
  const depth = new Map<string, number>();

  for (const node of nodes) {
    const existingDependencies = node.dependsOn.filter((dependency) => byId.has(dependency));
    indegree.set(node.nodeId, existingDependencies.length);
    depth.set(node.nodeId, 0);
    for (const dependency of existingDependencies) {
      const targets = dependents.get(dependency) ?? [];
      targets.push(node.nodeId);
      dependents.set(dependency, targets);
    }
  }

  const ready = nodes
    .filter((node) => indegree.get(node.nodeId) === 0)
    .map((node) => node.nodeId);
  const visited = new Set<string>();
  while (ready.length > 0) {
    ready.sort((a, b) => (order.get(a) ?? 0) - (order.get(b) ?? 0));
    const current = ready.shift();
    if (!current) break;
    visited.add(current);
    for (const target of dependents.get(current) ?? []) {
      depth.set(target, Math.max(depth.get(target) ?? 0, (depth.get(current) ?? 0) + 1));
      const nextIndegree = Math.max(0, (indegree.get(target) ?? 0) - 1);
      indegree.set(target, nextIndegree);
      if (nextIndegree === 0) ready.push(target);
    }
  }

  const resolvedMaxDepth = Math.max(0, ...[...visited].map((nodeId) => depth.get(nodeId) ?? 0));
  for (const node of nodes) {
    if (!visited.has(node.nodeId)) depth.set(node.nodeId, resolvedMaxDepth + 1);
  }

  const layers = new Map<number, DeepNoteDagNode[]>();
  for (const node of nodes) {
    const layer = depth.get(node.nodeId) ?? 0;
    layers.set(layer, [...(layers.get(layer) ?? []), node]);
  }
  const layerEntries = [...layers.entries()].sort(([a], [b]) => a - b);
  const maxRows = Math.max(1, ...layerEntries.map(([, layerNodes]) => layerNodes.length));
  const graphHeight = GRAPH_PADDING * 2 + maxRows * NODE_HEIGHT + Math.max(0, maxRows - 1) * ROW_GAP;
  const positioned: DeepNoteDagLayoutNode[] = [];
  for (const [layer, layerNodes] of layerEntries) {
    const layerHeight = layerNodes.length * NODE_HEIGHT + Math.max(0, layerNodes.length - 1) * ROW_GAP;
    const startY = GRAPH_PADDING + Math.max(0, (graphHeight - GRAPH_PADDING * 2 - layerHeight) / 2);
    layerNodes.forEach((node, row) => {
      positioned.push({
        node,
        layer,
        x: GRAPH_PADDING + layer * (NODE_WIDTH + LAYER_GAP),
        y: startY + row * (NODE_HEIGHT + ROW_GAP),
        width: NODE_WIDTH,
        height: NODE_HEIGHT,
      });
    });
  }

  const positionedById = new Map(positioned.map((item) => [item.node.nodeId, item]));
  const edges: DeepNoteDagLayoutEdge[] = [];
  for (const target of positioned) {
    for (const dependency of target.node.dependsOn) {
      const source = positionedById.get(dependency);
      if (!source) continue;
      if (source.layer === target.layer) {
        const sourceBelowTarget = source.y > target.y;
        const sourceX = source.x + source.width / 2;
        const sourceY = sourceBelowTarget ? source.y : source.y + source.height;
        const targetX = target.x + target.width / 2;
        const targetY = sourceBelowTarget ? target.y + target.height : target.y;
        const curve = Math.max(22, Math.abs(targetY - sourceY) / 2);
        edges.push({
          sourceId: dependency,
          targetId: target.node.nodeId,
          path: `M ${sourceX} ${sourceY} C ${sourceX + 24} ${sourceY + (sourceBelowTarget ? -curve : curve)}, ${targetX + 24} ${targetY + (sourceBelowTarget ? curve : -curve)}, ${targetX} ${targetY}`,
        });
      } else {
        const sourceX = source.x + source.width;
        const sourceY = source.y + source.height / 2;
        const targetX = target.x;
        const targetY = target.y + target.height / 2;
        const control = Math.max(28, (targetX - sourceX) / 2);
        edges.push({
          sourceId: dependency,
          targetId: target.node.nodeId,
          path: `M ${sourceX} ${sourceY} C ${sourceX + control} ${sourceY}, ${targetX - control} ${targetY}, ${targetX} ${targetY}`,
        });
      }
    }
  }

  const maxLayer = Math.max(0, ...layerEntries.map(([layer]) => layer));
  return {
    width: GRAPH_PADDING * 2 + (maxLayer + 1) * NODE_WIDTH + maxLayer * LAYER_GAP,
    height: graphHeight,
    nodes: positioned,
    edges,
  };
}

const STATUS_LABELS: Record<DeepNoteDagNodeStatus, string> = {
  pending: "等待",
  ready: "就绪",
  leased: "已租约",
  inProgress: "执行中",
  needsReview: "待检查",
  needsRevision: "待修订",
  completed: "已完成",
  failed: "失败",
  blocked: "已阻塞",
  skipped: "已跳过",
  interrupted: "已中断",
  superseded: "已替代",
};

const NODE_TYPE_LABELS: Record<string, string> = {
  analyzeInput: "分析输入",
  reconSource: "核对来源",
  extractEvidence: "提取证据",
  buildLedger: "构建知识账本",
  draftSection: "生成章节",
  validateSection: "验证章节",
  reviewSection: "复核章节",
  reviseSection: "修订章节",
  validateGlobal: "跨章节验证",
  applyPatch: "应用修订",
  assembleNote: "组装笔记",
  persistNote: "保存笔记",
};

function nodeLabel(node: DeepNoteDagNode, headings: Map<string, string>): string {
  const typeLabel = NODE_TYPE_LABELS[node.nodeType] ?? node.nodeType;
  if (!node.sectionId) return typeLabel;
  return `${typeLabel} · ${headings.get(node.sectionId) ?? node.sectionId}`;
}

export function DeepNoteDagGraph({
  nodes,
  sectionHeadings,
}: {
  nodes: DeepNoteDagNode[];
  sectionHeadings: Map<string, string>;
}) {
  const [open, setOpen] = useState(true);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const markerId = `deep-note-dag-arrow-${useId().replace(/:/g, "")}`;
  const layout = useMemo(() => layoutDeepNoteDag(nodes), [nodes]);
  const selected = nodes.find((node) => node.nodeId === selectedId) ?? null;
  const completed = nodes.filter((node) => node.status === "completed").length;
  const active = nodes.filter((node) => node.status === "inProgress" || node.status === "leased").length;
  const failed = nodes.filter((node) => node.status === "failed" || node.status === "blocked").length;

  if (nodes.length === 0) return null;

  return (
    <section className="deep-note-dag-panel" aria-label="执行 DAG">
      <div className="deep-note-dag-header">
        <button type="button" onClick={() => setOpen((value) => !value)} aria-expanded={open}>
          <ChevronDown size={15} data-open={open} />
          <GitBranch size={15} />
          <strong>执行 DAG</strong>
          <span>{completed}/{nodes.length} 完成{active > 0 ? ` · ${active} 执行中` : ""}{failed > 0 ? ` · ${failed} 异常` : ""}</span>
        </button>
        <small><MousePointer2 size={13} />选择节点查看检查点</small>
      </div>
      {open ? (
        <>
          <div className="deep-note-dag-legend" aria-label="节点状态图例">
            <span data-status="completed"><i />已完成</span>
            <span data-status="inProgress"><i />执行中 / 就绪</span>
            <span data-status="needsReview"><i />待检查 / 修订</span>
            <span data-status="failed"><i />失败 / 阻塞</span>
            <span data-status="pending"><i />等待</span>
          </div>
          <div className="deep-note-dag-scroll" tabIndex={0} aria-label="可横向滚动的 DAG 流程图">
            <svg width={layout.width} height={layout.height} viewBox={`0 0 ${layout.width} ${layout.height}`}>
              <defs>
                <marker id={markerId} viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                  <path d="M 0 0 L 10 5 L 0 10 z" />
                </marker>
              </defs>
              <g className="deep-note-dag-edges" aria-hidden="true">
                {layout.edges.map((edge) => (
                  <path key={`${edge.sourceId}->${edge.targetId}`} d={edge.path} markerEnd={`url(#${markerId})`} />
                ))}
              </g>
              {layout.nodes.map((item) => {
                const label = nodeLabel(item.node, sectionHeadings);
                return (
                  <foreignObject key={item.node.nodeId} x={item.x} y={item.y} width={item.width} height={item.height}>
                    <button
                      type="button"
                      className="deep-note-dag-node"
                      data-status={item.node.status}
                      data-selected={selectedId === item.node.nodeId}
                      aria-pressed={selectedId === item.node.nodeId}
                      aria-label={`${label}，${STATUS_LABELS[item.node.status]}`}
                      onClick={() => setSelectedId((current) => current === item.node.nodeId ? null : item.node.nodeId)}
                    >
                      <span className="deep-note-dag-node-title">{label}</span>
                      <span className="deep-note-dag-node-meta"><i />{STATUS_LABELS[item.node.status]} · {item.node.dependsOn.length} 个依赖</span>
                    </button>
                  </foreignObject>
                );
              })}
            </svg>
          </div>
          {selected ? (
            <div className="deep-note-dag-detail" data-status={selected.status}>
              <div>
                <strong>{nodeLabel(selected, sectionHeadings)}</strong>
                <code>{selected.nodeId}</code>
              </div>
              <dl>
                <div><dt>状态</dt><dd>{STATUS_LABELS[selected.status]}</dd></div>
                <div><dt>尝试</dt><dd>{selected.attemptCount} 次</dd></div>
                <div><dt>前置节点</dt><dd>{selected.dependsOn.length > 0 ? selected.dependsOn.join("、") : "无"}</dd></div>
                <div><dt>产物检查点</dt><dd>{selected.outputRef ?? "尚未生成"}</dd></div>
                <div><dt>证据</dt><dd>{selected.evidenceIds.length} 项</dd></div>
              </dl>
              {selected.errorMessage ? <p><AlertTriangle size={14} />{selected.errorMessage}</p> : null}
            </div>
          ) : null}
        </>
      ) : null}
    </section>
  );
}
