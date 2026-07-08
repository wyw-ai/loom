import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronUp, ChevronDown } from "lucide-react";

type ScrollJumpButtonsProps = {
  showJumpToTop: boolean;
  showJumpToBottom: boolean;
  onJumpToTop: () => void;
  onJumpToBottom: () => void;
};

export function ScrollJumpButtons({
  showJumpToTop,
  showJumpToBottom,
  onJumpToTop,
  onJumpToBottom,
}: ScrollJumpButtonsProps) {
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);

  if (!mounted || (!showJumpToTop && !showJumpToBottom)) return null;

  return createPortal(
    <div
      style={{
        position: "fixed",
        bottom: "80px",
        right: "32px",
        zIndex: 9999,
        display: "flex",
        flexDirection: "column",
        gap: "8px",
        opacity: 0.8,
        transition: "opacity 200ms ease",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.opacity = "1";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.opacity = "0.8";
      }}
    >
      {showJumpToTop && (
        <button
          type="button"
          onClick={onJumpToTop}
          aria-label="Jump to top"
          style={{
            width: "36px",
            height: "36px",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            borderRadius: "9999px",
            border: "1px solid #dfe3ec",
            background: "#ffffff",
            color: "#667085",
            boxShadow: "0 2px 8px rgba(0,0,0,0.12)",
            cursor: "pointer",
            transition: "background 150ms ease, color 150ms ease",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "#f3f4f6";
            e.currentTarget.style.color = "#1f2937";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "#ffffff";
            e.currentTarget.style.color = "#667085";
          }}
        >
          <ChevronUp size={20} />
        </button>
      )}
      {showJumpToBottom && (
        <button
          type="button"
          onClick={onJumpToBottom}
          aria-label="Jump to bottom"
          style={{
            width: "36px",
            height: "36px",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            borderRadius: "9999px",
            border: "1px solid #dfe3ec",
            background: "#ffffff",
            color: "#667085",
            boxShadow: "0 2px 8px rgba(0,0,0,0.12)",
            cursor: "pointer",
            transition: "background 150ms ease, color 150ms ease",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "#f3f4f6";
            e.currentTarget.style.color = "#1f2937";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "#ffffff";
            e.currentTarget.style.color = "#667085";
          }}
        >
          <ChevronDown size={20} />
        </button>
      )}
    </div>,
    document.body,
  );
}
