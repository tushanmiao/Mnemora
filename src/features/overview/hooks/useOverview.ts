import { useCallback, useEffect, useState } from "react";
import type { OverviewSnapshot } from "../types";
import { loadOverviewSnapshot } from "../api/overview";

export function useOverview() {
  const [snapshot, setSnapshot] = useState<OverviewSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [refreshVersion, setRefreshVersion] = useState(0);

  const refresh = useCallback(() => setRefreshVersion((value) => value + 1), []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError("");
    void loadOverviewSnapshot()
      .then((next) => {
        if (!cancelled) setSnapshot(next);
      })
      .catch((reason) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => { cancelled = true; };
  }, [refreshVersion]);

  return { snapshot, loading, error, refresh };
}
