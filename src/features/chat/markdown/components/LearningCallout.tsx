import { useState, type HTMLAttributes, type ReactNode } from "react";
import { AlertTriangle, BookMarked, CircleHelp, FlaskConical, Lightbulb, NotebookText, Scale, Star } from "lucide-react";
import { extractCodeText } from "../utils/codeBlock";
import "../styles/enhanced-markdown.css";

const calloutDefinitions = {
  note: { label: "说明", icon: NotebookText },
  tip: { label: "提示", icon: Lightbulb },
  important: { label: "重点", icon: Star },
  warning: { label: "注意", icon: AlertTriangle },
  definition: { label: "定义", icon: BookMarked },
  example: { label: "示例", icon: FlaskConical },
  evidence: { label: "证据", icon: Scale },
  question: { label: "思考", icon: CircleHelp },
} as const;

type LearningCalloutProps = HTMLAttributes<HTMLElement> & {
  children?: ReactNode;
  "data-callout"?: string;
};

export function LearningCallout({ children, "data-callout": rawType, ...props }: LearningCalloutProps) {
  const type = rawType && rawType in calloutDefinitions ? rawType as keyof typeof calloutDefinitions : "note";
  const definition = calloutDefinitions[type];
  const Icon = definition.icon;
  const collapsible = extractCodeText(children).length > 700;
  const [open, setOpen] = useState(true);
  return (
    <aside {...props} className={`markdown-callout markdown-callout-${type}`} data-callout={type}>
      <header className="markdown-callout-heading">
        <Icon size={15} />
        <strong>{definition.label}</strong>
        {collapsible ? <button type="button" onClick={() => setOpen((value) => !value)}>{open ? "收起" : "展开"}</button> : null}
      </header>
      {open ? <div className="markdown-callout-content">{children}</div> : null}
    </aside>
  );
}

