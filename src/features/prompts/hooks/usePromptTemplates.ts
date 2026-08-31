import { useCallback, useEffect, useState } from "react";
import type { PromptTemplate, PromptTemplateInput } from "../../../types/prompt";
import {
  deletePromptTemplate,
  listPromptTemplates,
  upsertPromptTemplate,
} from "../api/promptTemplates";

export function usePromptTemplates() {
  const [templates, setTemplates] = useState<PromptTemplate[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [createRequested, setCreateRequested] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const loaded = await listPromptTemplates();
      setTemplates(loaded);
      setError(null);
      return loaded;
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      return [];
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = useCallback(async (input: PromptTemplateInput) => {
    const saved = await upsertPromptTemplate(input);
    setTemplates((current) => [saved, ...current.filter((item) => item.id !== saved.id)]);
    setError(null);
    return saved;
  }, []);

  const remove = useCallback(async (promptId: string) => {
    const removed = await deletePromptTemplate(promptId);
    if (removed) setTemplates((current) => current.filter((item) => item.id !== promptId));
    setError(null);
    return removed;
  }, []);

  const requestCreate = useCallback(() => setCreateRequested(true), []);
  const consumeCreateRequest = useCallback(() => setCreateRequested(false), []);

  return {
    templates,
    loading,
    error,
    createRequested,
    refresh,
    save,
    remove,
    requestCreate,
    consumeCreateRequest,
  };
}
