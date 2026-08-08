import React from "react";
import ReactDOM from "react-dom/client";

import "./design/globals.css";
import { App } from "./App";
import { AgentIdentityBadgeWorkbench } from "@/components/agent/AgentIdentityBadge";

const preview = new URLSearchParams(window.location.search).get("preview");
const Root = preview === "agent-badge" ? AgentIdentityBadgeWorkbench : App;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);

if (import.meta.env.PROD && "serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    void navigator.serviceWorker.register("/sw.js");
  });
}
