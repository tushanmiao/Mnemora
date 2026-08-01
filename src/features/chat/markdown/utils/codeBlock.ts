import { Children, isValidElement, type ReactNode } from "react";

export function extractCodeText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(extractCodeText).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return extractCodeText(node.props.children);
  return "";
}

export function extractCodeLanguage(children: ReactNode): string | null {
  const codeElement = Children.toArray(children).find((child) => (
    isValidElement<{ className?: string }>(child)
  ));
  if (!isValidElement<{ className?: string }>(codeElement)) return null;
  return codeElement.props.className?.match(/language-([^\s]+)/)?.[1] ?? null;
}

export function normalizeCodeLanguage(language: string | null | undefined) {
  const normalized = language?.toLowerCase().trim();
  if (!normalized) return null;
  const aliases: Record<string, string> = {
    js: "javascript",
    ts: "typescript",
    py: "python",
    rb: "ruby",
    sh: "shell",
    zsh: "shell",
    yml: "yaml",
    md: "markdown",
    rs: "rust",
    cs: "csharp",
    golang: "go",
    psql: "pgsql",
    plaintext: "text",
    text: "text",
  };
  return aliases[normalized] ?? normalized;
}

export function isMermaidLanguage(language: string | null | undefined) {
  return normalizeCodeLanguage(language) === "mermaid";
}

