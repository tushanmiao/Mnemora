import React, { type ErrorInfo, type ReactNode } from "react";
import { MessageCircle, RefreshCw } from "lucide-react";

type WorkspaceViewBoundaryProps = {
  children: ReactNode;
  viewLabel: string;
  failureLabel: string;
  retryLabel: string;
  returnChatLabel: string;
  onRetry: () => void;
  onReturnToChat: () => void;
};

type WorkspaceViewBoundaryState = { error: Error | null };

/** 单个工作区失败时保留活动栏和其它视图，不再让局部异常演变成整页白屏。 */
export class WorkspaceViewBoundary extends React.Component<
  WorkspaceViewBoundaryProps,
  WorkspaceViewBoundaryState
> {
  state: WorkspaceViewBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): WorkspaceViewBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(`${this.props.viewLabel}视图加载失败`, error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="workspace-loading workspace-view-error" role="alert">
        <strong>{this.props.failureLabel}</strong>
        <span>{this.state.error.message}</span>
        <div className="workspace-view-error-actions">
          <button type="button" onClick={this.props.onRetry}>
            <RefreshCw size={14} />
            {this.props.retryLabel}
          </button>
          <button type="button" onClick={this.props.onReturnToChat}>
            <MessageCircle size={14} />
            {this.props.returnChatLabel}
          </button>
        </div>
      </div>
    );
  }
}
