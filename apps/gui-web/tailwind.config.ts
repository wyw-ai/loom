import type { Config } from "tailwindcss";

// Keep the palette in sync with docs/gui-desktop-design.md §4.1. Tokens live
// as CSS variables (see src/design/tokens.css) so the same values serve
// inline styles, Tailwind, and any third-party component that wants them.
const config: Config = {
  darkMode: "class",
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        servers: "var(--bg-servers)",
        sidebar: "var(--bg-sidebar)",
        main: "var(--bg-main)",
        elevated: "var(--bg-elevated)",
        hover: "var(--bg-hover)",
        active: "var(--bg-active)",
        mention: "var(--bg-mention)",
        border: "var(--border)",
        primary: "var(--text-primary)",
        secondary: "var(--text-secondary)",
        muted: "var(--text-muted)",
        accent: "var(--accent)",
        "accent-hover": "var(--accent-hover)",
        "accent-contrast": "var(--accent-contrast)",
        success: "var(--success)",
        warning: "var(--warning)",
        danger: "var(--danger)",
        "role-human": "var(--role-human)",
        "role-agent": "var(--role-agent)",
        "role-service": "var(--role-service)",
      },
      fontFamily: {
        ui: [
          "Inter",
          "-apple-system",
          "BlinkMacSystemFont",
          "Segoe UI",
          "sans-serif",
        ],
        mono: [
          "JetBrains Mono",
          "SF Mono",
          "Menlo",
          "Consolas",
          "monospace",
        ],
      },
    },
  },
  plugins: [],
};

export default config;
