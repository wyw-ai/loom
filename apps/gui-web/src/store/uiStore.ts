import { create } from "zustand";
import type { Message } from "@/ipc/types";
import type { ActorWorkspaceSection, ChannelPanelTab, PanelResizeKind, PanelSizes, View } from "@/lib/types";
import { initialViewportWidth, loadPanelSizes } from "@/lib/format-utils";

export interface UIStore {
  // State
  view: View;
  settingsAgentId: string | null;
  /** Which section of the Actors/Hosts workspace view is active. Drives both
   * the SettingsView content and the sidebar highlight (Actors vs Managed
   * Hosts are two nav entries over the same view). */
  settingsSection: ActorWorkspaceSection;
  channelPanelTab: ChannelPanelTab | null;
  searchPanelOpen: boolean;
  selectedRunId: string | null;
  panelSizes: PanelSizes;
  viewportWidth: number;
  resizingPanel: PanelResizeKind | null;
  replyTo: Message | null;

  // Setters
  setView: (view: View | ((prev: View) => View)) => void;
  setSettingsAgentId: (id: string | null | ((prev: string | null) => string | null)) => void;
  setSettingsSection: (section: ActorWorkspaceSection | ((prev: ActorWorkspaceSection) => ActorWorkspaceSection)) => void;
  setChannelPanelTab: (tab: ChannelPanelTab | null | ((prev: ChannelPanelTab | null) => ChannelPanelTab | null)) => void;
  setSearchPanelOpen: (open: boolean | ((prev: boolean) => boolean)) => void;
  setSelectedRunId: (id: string | null | ((prev: string | null) => string | null)) => void;
  setPanelSizes: (sizes: PanelSizes | ((prev: PanelSizes) => PanelSizes)) => void;
  setViewportWidth: (width: number | ((prev: number) => number)) => void;
  setResizingPanel: (kind: PanelResizeKind | null | ((prev: PanelResizeKind | null) => PanelResizeKind | null)) => void;
  setReplyTo: (message: Message | null | ((prev: Message | null) => Message | null)) => void;
}

export const useUIStore = create<UIStore>((set) => ({
  view: "chat",
  settingsAgentId: null,
  settingsSection: "agents",
  channelPanelTab: null,
  searchPanelOpen: false,
  selectedRunId: null,
  panelSizes: loadPanelSizes(),
  viewportWidth: initialViewportWidth(),
  resizingPanel: null,
  replyTo: null,

  setView: (view) => set((state) => ({ view: typeof view === 'function' ? view(state.view) : view })),
  setSettingsAgentId: (settingsAgentId) => set((state) => ({ settingsAgentId: typeof settingsAgentId === 'function' ? settingsAgentId(state.settingsAgentId) : settingsAgentId })),
  setSettingsSection: (settingsSection) => set((state) => ({ settingsSection: typeof settingsSection === 'function' ? settingsSection(state.settingsSection) : settingsSection })),
  setChannelPanelTab: (channelPanelTab) => set((state) => ({ channelPanelTab: typeof channelPanelTab === 'function' ? channelPanelTab(state.channelPanelTab) : channelPanelTab })),
  setSearchPanelOpen: (searchPanelOpen) => set((state) => ({ searchPanelOpen: typeof searchPanelOpen === 'function' ? searchPanelOpen(state.searchPanelOpen) : searchPanelOpen })),
  setSelectedRunId: (selectedRunId) => set((state) => ({ selectedRunId: typeof selectedRunId === 'function' ? selectedRunId(state.selectedRunId) : selectedRunId })),
  setPanelSizes: (panelSizes) => set((state) => ({ panelSizes: typeof panelSizes === 'function' ? panelSizes(state.panelSizes) : panelSizes })),
  setViewportWidth: (viewportWidth) => set((state) => ({ viewportWidth: typeof viewportWidth === 'function' ? viewportWidth(state.viewportWidth) : viewportWidth })),
  setResizingPanel: (resizingPanel) => set((state) => ({ resizingPanel: typeof resizingPanel === 'function' ? resizingPanel(state.resizingPanel) : resizingPanel })),
  setReplyTo: (replyTo) => set((state) => ({ replyTo: typeof replyTo === 'function' ? replyTo(state.replyTo) : replyTo })),
}));
