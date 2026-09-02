import { useMemo, useState } from "react";
import { CircleHelp } from "lucide-react";
import { useI18n } from "../../../../i18n/I18nProvider";
import { resolveToolQuestion } from "../../api/chat";
import type { ToolQuestion, ToolQuestionAnswer } from "../../../../types/chat";

/** 「其他」选项不是模型给的候选项，用一个不可能与 label 冲突的哨兵值标记。 */
const OTHER_VALUE = "__other__";

type Selection = { values: string[]; other: string };

/**
 * 渲染 ask_user_question 的选项表单。
 *
 * 表单只负责收集与提交：是否该出现由 `interrupt.kind` 决定，超时与取消由后端的
 * 等待方处理。提交成功后不清空本地状态——轨迹会切出 awaitingApproval，整个组件
 * 随之卸载。
 */
export function ToolQuestionForm({
  approvalId,
  questions,
  disabled,
}: {
  approvalId: string;
  questions: ToolQuestion[];
  disabled: boolean;
}) {
  const { t } = useI18n();
  const [selections, setSelections] = useState<Record<string, Selection>>({});
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const answers = useMemo<ToolQuestionAnswer[] | null>(() => {
    const collected: ToolQuestionAnswer[] = [];
    for (const question of questions) {
      const selection = selections[question.header];
      if (!selection) return null;
      const chosen = selection.values.filter((value) => value !== OTHER_VALUE);
      // 选了「其他」但没填字，等于还没回答：不能提交一个空答案冒充已回答。
      if (selection.values.includes(OTHER_VALUE)) {
        const trimmed = selection.other.trim();
        if (!trimmed) return null;
        chosen.push(trimmed);
      }
      if (chosen.length === 0) return null;
      collected.push({ header: question.header, values: chosen });
    }
    return collected;
  }, [questions, selections]);

  const toggle = (question: ToolQuestion, value: string) => {
    setError(null);
    setSelections((current) => {
      const existing = current[question.header] ?? { values: [], other: "" };
      const next = question.multiSelect
        ? existing.values.includes(value)
          ? existing.values.filter((item) => item !== value)
          : [...existing.values, value]
        : [value];
      return { ...current, [question.header]: { ...existing, values: next } };
    });
  };

  const submit = async () => {
    if (!answers) {
      setError(t("chat.workflowQuestionRequired"));
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await resolveToolQuestion(approvalId, answers);
    } catch (submitError) {
      setError(submitError instanceof Error ? submitError.message : String(submitError));
    } finally {
      setSubmitting(false);
    }
  };

  const busy = disabled || submitting;

  return (
    <div className="agent-workflow-question">
      <p className="agent-workflow-question-prompt">
        <CircleHelp size={14} aria-hidden="true" />
        <span>{t("chat.workflowQuestionPrompt")}</span>
      </p>

      {questions.map((question) => {
        const selection = selections[question.header] ?? { values: [], other: "" };
        const inputType = question.multiSelect ? "checkbox" : "radio";
        return (
          <fieldset key={question.header} className="agent-workflow-question-group" disabled={busy}>
            <legend>{question.question}</legend>
            {question.options.map((option) => (
              <label key={option.label} className="agent-workflow-question-option">
                <input
                  type={inputType}
                  name={`${approvalId}-${question.header}`}
                  checked={selection.values.includes(option.label)}
                  onChange={() => toggle(question, option.label)}
                />
                <span>
                  <strong>{option.label}</strong>
                  {option.description ? <small>{option.description}</small> : null}
                </span>
              </label>
            ))}
            <label className="agent-workflow-question-option">
              <input
                type={inputType}
                name={`${approvalId}-${question.header}`}
                checked={selection.values.includes(OTHER_VALUE)}
                onChange={() => toggle(question, OTHER_VALUE)}
              />
              <span>{t("chat.workflowQuestionOther")}</span>
            </label>
            {selection.values.includes(OTHER_VALUE) ? (
              <input
                className="agent-workflow-question-other"
                type="text"
                value={selection.other}
                placeholder={t("chat.workflowQuestionOtherPlaceholder")}
                onChange={(event) => {
                  setError(null);
                  setSelections((current) => ({
                    ...current,
                    [question.header]: { ...selection, other: event.target.value },
                  }));
                }}
              />
            ) : null}
          </fieldset>
        );
      })}

      {error ? (
        <p className="agent-workflow-question-error" role="alert">{error}</p>
      ) : null}

      <button
        className="agent-workflow-approve"
        type="button"
        disabled={busy}
        onClick={() => void submit()}
      >
        {t("chat.workflowQuestionSubmit")}
      </button>
    </div>
  );
}
