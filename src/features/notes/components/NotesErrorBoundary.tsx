import React, { type ErrorInfo, type ReactNode } from "react";
import { RefreshCw } from "lucide-react";

type Props = { children: ReactNode };
type State = { error: Error | null };

/** 笔记按需模块失败时只显示局部恢复入口，不影响 Chat 和 Work。 */
export class NotesErrorBoundary extends React.Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("笔记工作区加载失败", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="workspace-loading notes-load-error" role="alert">
        <strong>笔记页面加载失败</strong>
        <span>{this.state.error.message || "按需模块暂时不可用"}</span>
        <button type="button" onClick={() => window.location.reload()}>
          <RefreshCw size={14} />重新加载笔记
        </button>
      </div>
    );
  }
}
