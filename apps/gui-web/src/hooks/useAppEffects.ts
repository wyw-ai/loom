import { useEffect } from "react";
import type { Actor, ScopeRef } from "@/ipc/types";
import type { ChannelGroup, PanelSizes } from "@/lib/types";
import {
  initialViewportWidth,
  savePanelSizes,
} from "@/lib/format-utils";
import {
  loadChannelGroups,
} from "@/lib/channel-utils";

export interface AppEffectsDeps {
  panelSizes: PanelSizes;
  setViewportWidth: (width: number) => void;
  channelGroupsKey: string;
  setChannelGroups: (groups: ChannelGroup[] | ((prev: ChannelGroup[]) => ChannelGroup[])) => void;
  workspaceId: string | undefined;
  setActiveDirectActorId: (id: string | null | ((prev: string | null) => string | null)) => void;
  setDirectMessages: (messages: never[]) => void;
  setDirectDraft: (draft: string) => void;
  setDirectScopesByActorId: (scopes: Record<string, ScopeRef> | ((prev: Record<string, ScopeRef>) => Record<string, ScopeRef>)) => void;
  view: string;
  agentActorIdsKey: string;
  agentActors: Actor[];
  cleanupPanelResize: () => void;
}

export function useAppEffects(deps: AppEffectsDeps) {
  const d = deps;

  useEffect(() => {
    savePanelSizes(d.panelSizes);
  }, [d.panelSizes]);

  useEffect(() => {
    const updateViewport = () => d.setViewportWidth(initialViewportWidth());
    updateViewport();
    window.addEventListener("resize", updateViewport);
    return () => window.removeEventListener("resize", updateViewport);
  }, []);

  useEffect(() => {
    d.setChannelGroups(loadChannelGroups(d.channelGroupsKey));
  }, [d.channelGroupsKey]);

  useEffect(() => {
    d.setActiveDirectActorId(null);
    d.setDirectMessages([]);
    d.setDirectDraft("");
    d.setDirectScopesByActorId({});
  }, [d.workspaceId]);

  useEffect(() => {
    if (d.view !== "direct") return;
    d.setActiveDirectActorId((current: string | null) => current ?? d.agentActors[0]?.id ?? null);
  }, [d.agentActorIdsKey, d.view]);

  useEffect(() => {
    return () => {
      d.cleanupPanelResize();
      document.body.classList.remove("is-resizing-panels");
    };
  }, [d.cleanupPanelResize]);
}
