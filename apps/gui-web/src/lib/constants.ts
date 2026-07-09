import type { PanelSizes } from "./types";

export const supportedReactionEmojis = ["👍", "👀", "✅", "🥳", "💔"];
export const avatarCount = 25;
export const agentAvatarIndexes = [1, 5, 10, 15, 20, 25] as const;
export const agentMessageBadgePopoverScale = 0.5;
export const agentMessageBadgeCompactWidth = 256;
export const agentMessageBadgeDetailWidth = 668;
export const agentMessageBadgeCompactHeight = 490;
export const agentMessageBadgeDetailHeight = 1352;
export const avatarLibraryUrls = Array.from(
  { length: avatarCount },
  (_, index) => `/avatars/avatar-${String(index + 1).padStart(2, "0")}.png`,
);
export const localServerCommand = "loom-server --bind 0.0.0.0:7878";
export const localServerUrl = "ws://127.0.0.1:7878/rpc";
export const machineStatusPollIntervalMs = 5_000;
export const reasoningEffortChoices = ["", "minimal", "low", "medium", "high", "xhigh"] as const;
export const defaultNewPromptFilePath = "prompts/new.md";
export const defaultSystemPromptTemplate = [
  "{bootstrap_memory}",
  "{profile_prompt_files}",
].join("\n\n");
export const defaultUserPromptTemplate = [
  "{turn_memory}",
  "{runtime_context}",
  "{assignment_context}",
  "{user_message}",
].join("\n\n");
export const promptPresetParts: Record<string, string[]> = {
  loom_system: ["bootstrap_memory", "profile_prompt_files"],
  loom_turn: ["turn_memory", "runtime_context", "assignment_context", "user_message"],
  loom_full: [
    "bootstrap_memory",
    "profile_prompt_files",
    "turn_memory",
    "runtime_context",
    "assignment_context",
    "user_message",
  ],
};
export const promptVariableOptions = [
  { key: "bootstrap_memory", label: "Long memory" },
  { key: "profile_prompt_files", label: "Profile prompt files" },
  { key: "turn_memory", label: "Turn memory" },
  { key: "runtime_context", label: "Runtime" },
  { key: "assignment_context", label: "Assignment" },
  { key: "user_message", label: "Message" },
] as const;
export const defaultAgentPromptAssembly: Record<string, unknown> = {
  outputs: {
    system: {
      template: defaultSystemPromptTemplate,
    },
    user: {
      template: defaultUserPromptTemplate,
    },
    full: {
      include: ["prompt.system", "prompt.user"],
    },
  },
};
export const defaultProviderManifestText = JSON.stringify(
  {
    schemaVersion: 1,
    id: "my_provider",
    displayName: "My Provider",
    detect: {
      candidates: ["my-agent"],
    },
    modes: {
      print: {
        transport: "command",
        command: "{bin}",
        args: [
          "run",
          {
            when: "model",
            args: ["--model", "{model}"],
          },
          "{prompt.full}",
        ],
        stdout: {
          format: "text",
        },
      },
    },
    models: {
      default: "default",
      choices: [{ id: "default", label: "Default" }],
    },
  },
  null,
  2,
);
export const ungroupedChannelGroupId = "__ungrouped";
export const channelContextMenuWidthPx = 44 * 4;
export const channelContextMenuItemHeightPx = 36;
export const channelContextMenuItemCount = 2;
export const channelContextMenuPaddingPx = 4;
export const channelContextMenuBorderPx = 1;
export const channelContextMenuViewportPaddingPx = 8;
export const channelContextMenuHeightPx =
  channelContextMenuPaddingPx * 2 +
  channelContextMenuItemHeightPx * channelContextMenuItemCount +
  channelContextMenuBorderPx * 2;
export const panelLayoutStorageKey = "loom:panel-layout:v1";
export const detailPanelBreakpoint = 1280;
export const railWidth = 72;
export const resizeHandleWidth = 8;
export const sidebarMinWidth = 216;
export const sidebarMaxWidth = 420;
export const detailMinWidth = 280;
export const detailMaxWidth = 560;
export const mainMinWidth = 360;
export const defaultPanelSizes: PanelSizes = {
  sidebar: 286,
  detail: 340,
};
