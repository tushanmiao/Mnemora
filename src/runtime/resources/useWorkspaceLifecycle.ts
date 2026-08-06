import { useEffect, useState } from "react";
import {
  getWorkspaceLifecycleState,
  subscribeWorkspaceLifecycle,
  type WorkspaceLifecycleState,
} from "./WorkspaceLifecycle";

export function useWorkspaceLifecycle(): WorkspaceLifecycleState {
  const [state, setState] = useState(getWorkspaceLifecycleState);
  useEffect(() => subscribeWorkspaceLifecycle(setState), []);
  return state;
}
