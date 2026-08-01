import { Component, type ErrorInfo, type ReactNode } from "react";

type RenderFallbackProps = {
  children: ReactNode;
  fallback: ReactNode;
};

type RenderFallbackState = { failed: boolean };

/** 局部增强渲染失败时回退到源文本，避免整条消息或整个窗口白屏。 */
export class RenderFallback extends Component<RenderFallbackProps, RenderFallbackState> {
  state: RenderFallbackState = { failed: false };

  static getDerivedStateFromError(): RenderFallbackState {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Markdown 增强渲染失败", error.message, info.componentStack);
  }

  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}

