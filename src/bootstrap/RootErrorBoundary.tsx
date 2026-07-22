import React, { type ErrorInfo, type ReactNode } from "react";
import { StartupFailure } from "./StartupFailure";
import {
  createStartupDiagnostic,
  recordStartupDiagnostic,
  type StartupDiagnostic,
} from "./startupDiagnostics";

type Props = {
  children: ReactNode;
  title?: string;
};

type State = {
  diagnostic: StartupDiagnostic | null;
};

export class RootErrorBoundary extends React.Component<Props, State> {
  state: State = { diagnostic: null };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { diagnostic: createStartupDiagnostic(error) };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const diagnostic = createStartupDiagnostic(error, {
      context: this.props.title ?? "application-root",
      componentStack: info.componentStack ?? undefined,
    });
    this.setState({ diagnostic });
    recordStartupDiagnostic(diagnostic);
    console.error("Mnemora React 渲染异常", diagnostic);
  }

  render() {
    if (this.state.diagnostic) {
      return <StartupFailure diagnostic={this.state.diagnostic} title={this.props.title} />;
    }
    return this.props.children;
  }
}
