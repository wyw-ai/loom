import { useState, useCallback, type ReactNode } from "react";
import { Copy, Check } from "lucide-react";

function extractText(node: ReactNode): string {
  if (typeof node === "string") return node;
  if (typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(extractText).join("");
  if (node && typeof node === "object" && "props" in node) {
    return extractText((node as { props: { children?: ReactNode } }).props.children);
  }
  return "";
}

export function CodeBlockWithCopy({
  language,
  codeClassName,
  children,
}: {
  language: string;
  codeClassName?: string;
  children: ReactNode;
}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(() => {
    const text = extractText(children);
    const fallback = () => {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand("copy");
      } catch {
        // noop
      }
      document.body.removeChild(ta);
    };

    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(text).catch(fallback);
    } else {
      fallback();
    }

    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [children]);

  return (
    <div className="code-block-wrapper">
      <div className="code-block-header">
        <span className="code-lang-label">{language || "text"}</span>
        <button
          type="button"
          onClick={handleCopy}
          className={`code-copy-btn${copied ? " copied" : ""}`}
          aria-label={copied ? "Copied!" : `Copy ${language || "code"}`}
        >
          {copied ? <Check size={14} /> : <Copy size={14} />}
        </button>
        <span role="status" aria-live="polite" className="sr-only">
          {copied ? "Copied!" : ""}
        </span>
      </div>
      <pre>
        <code className={codeClassName ?? (language ? `language-${language}` : undefined)}>{children}</code>
      </pre>
    </div>
  );
}
