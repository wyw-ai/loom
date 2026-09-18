import React from "react";
import ReactDOM from "react-dom/client";

import "./design/globals.css";
import { App } from "./App";
import { AgentIdentityBadgeWorkbench } from "@/components/agent/AgentIdentityBadge";
import { I18nProvider } from "@/lib/i18n";

const preview = new URLSearchParams(window.location.search).get("preview");
const Root = preview === "agent-badge" ? AgentIdentityBadgeWorkbench : App;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <I18nProvider>
      <Root />
    </I18nProvider>
  </React.StrictMode>,
);

const hasTauri =
  typeof window !== "undefined" &&
  Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

if (hasTauri) {
  // The desktop shell serves assets over Tauri's custom protocol; a service
  // worker (installed by an earlier run or a browser session on the same
  // origin) intercepts those requests, fails the network fetch, and serves
  // the cached index.html for every asset — a guaranteed white screen.
  // Never register in Tauri, and actively heal installs that already have
  // one registered.
  if ("serviceWorker" in navigator) {
    void navigator.serviceWorker.getRegistrations().then((registrations) => {
      for (const registration of registrations) void registration.unregister();
    });
    if (typeof caches !== "undefined") {
      void caches.keys().then((keys) => {
        for (const key of keys) void caches.delete(key);
      });
    }
  }
} else if (import.meta.env.PROD && "serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    void navigator.serviceWorker.register("/sw.js");
  });
}
