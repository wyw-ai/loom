import { errorText, machineCanRunCommands } from "@/lib/format-utils";
import {
  useEffect,
  useRef,
  useState,
} from "react";
import { HostDetailSection } from "@/components/shared/UIComponents";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { defaultNewPromptFilePath, defaultSystemPromptTemplate, defaultUserPromptTemplate, promptPresetParts, promptVariableOptions } from "@/lib/constants";
import { cn } from "@/lib/utils";
import { Check, FileText, Folder, Loader2, Plus, RefreshCw } from "lucide-react";
import type { AgentFileEntry, AgentPromptPreviewPart, AgentPromptPreviewResult, MachineInfo } from "@/ipc/types";
import type { PromptAssemblyBuildOptions, PromptTemplateDraft } from "@/lib/types";
import * as ipc from "@/ipc/bridge";

export function AgentPromptStudio({
  machine,
  agent,
  canEdit,
}: {
  machine: MachineInfo;
  agent: MachineInfo["agents"][number];
  canEdit: boolean;
}) {
  const actorId = agent.spec.actor.id;
  const promptStudioKey = `${machine.id}:${actorId}`;
  const [sampleMessage, setSampleMessage] = useState(
    "This is preview placeholder text. In a real request, this will be replaced by the actual handoff content.",
  );
  const [preview, setPreview] = useState<AgentPromptPreviewResult | null>(null);
  const [files, setFiles] = useState<AgentFileEntry[]>([]);
  const [filePath, setFilePath] = useState("");
  const [fileContent, setFileContent] = useState("");
  const [fileDirty, setFileDirty] = useState(false);
  const [promptTemplates, setPromptTemplates] = useState<PromptTemplateDraft>(() =>
    promptTemplatesFromAssembly(agent.spec.promptAssembly),
  );
  const [selectedVariableKey, setSelectedVariableKey] = useState("profile_prompt_files");
  const [promptBusy, setPromptBusy] = useState<"files" | "read" | "write" | "preview" | null>(null);
  const [assemblySaving, setAssemblySaving] = useState(false);
  const [promptError, setPromptError] = useState<string | null>(null);
  const handledPromptStudioKeyRef = useRef(promptStudioKey);
  const canRunCommands = machineCanRunCommands(machine);
  const canPreview = canRunCommands && machine.capabilities.includes("agent.prompt.preview");
  const canReadFiles = canRunCommands && machine.capabilities.includes("agent.file.read");
  const canWriteFiles = canEdit && machine.capabilities.includes("agent.file.write");
  const canSaveAssembly = canEdit && machine.capabilities.includes("agent.update");
  const selectedFile = files.find((file) => file.path === filePath) ?? null;
  const fileVariables = files.map((file) => ({
    key: `file.${promptFileKey(file.path)}`,
    label: file.path,
  }));
  const variableItems = [...promptVariableOptions, ...fileVariables];
  const selectedVariable =
    variableItems.find((item) => item.key === selectedVariableKey) ??
    variableItems.find((item) => item.key === "profile_prompt_files") ??
    variableItems[0] ??
    null;
  const selectedVariablePart =
    selectedVariable && preview
      ? preview.parts.find((part) => part.key === selectedVariable.key) ?? null
      : null;

  useEffect(() => {
    if (handledPromptStudioKeyRef.current === promptStudioKey) return;
    handledPromptStudioKeyRef.current = promptStudioKey;
    setPreview(null);
    setFiles([]);
    setFilePath("");
    setFileContent("");
    setFileDirty(false);
    setPromptTemplates(promptTemplatesFromAssembly(agent.spec.promptAssembly));
    setSelectedVariableKey("profile_prompt_files");
    setPromptError(null);
  }, [promptStudioKey, agent.spec.promptAssembly]);

  useEffect(() => {
    if (!canReadFiles) return;
    void refreshPromptFiles();
  }, [machine.id, actorId, canReadFiles]);

  function updatePromptTemplate(target: keyof PromptTemplateDraft, value: string) {
    setPromptTemplates((current) => ({ ...current, [target]: value }));
  }

  function resetPromptTemplates() {
    setPromptTemplates({
      system: defaultSystemPromptTemplate,
      user: defaultUserPromptTemplate,
    });
    setPreview(null);
  }

  function startNewPromptFile() {
    setFilePath(defaultNewPromptFilePath);
    setFileContent("");
    setFileDirty(false);
    setPromptError(null);
  }

  async function refreshPromptFiles() {
    if (!canReadFiles) return;
    setPromptBusy("files");
    setPromptError(null);
    try {
      const result = await ipc.agentFileList({
        machineId: machine.id,
        actorId,
        root: "profile",
        prefix: "prompts",
      });
      setFiles(result.files);
      if (filePath && !result.files.some((file) => file.path === filePath)) {
        setFilePath("");
        setFileContent("");
        setFileDirty(false);
      }
    } catch (err) {
      setPromptError(errorText(err));
    } finally {
      setPromptBusy(null);
    }
  }

  async function openPromptFile(path = filePath) {
    const nextPath = path.trim();
    if (!nextPath || !canReadFiles) return;
    setPromptBusy("read");
    setPromptError(null);
    try {
      const result = await ipc.agentFileRead({
        machineId: machine.id,
        actorId,
        root: "profile",
        path: nextPath,
      });
      setFilePath(result.path);
      setFileContent(result.content);
      setFileDirty(false);
    } catch (err) {
      setFilePath(nextPath);
      setFileContent("");
      setFileDirty(false);
      setPromptError(errorText(err));
    } finally {
      setPromptBusy(null);
    }
  }

  async function savePromptFile() {
    const nextPath = filePath.trim();
    if (!nextPath || !canWriteFiles) return;
    setPromptBusy("write");
    setPromptError(null);
    try {
      await ipc.agentFileWrite({
        machineId: machine.id,
        actorId,
        root: "profile",
        path: nextPath,
        content: fileContent,
      });
      setFilePath(nextPath);
      setFileDirty(false);
      await refreshPromptFiles();
    } catch (err) {
      setPromptError(errorText(err));
    } finally {
      setPromptBusy(null);
    }
  }

  async function refreshPreview() {
    if (!canPreview) return;
    setPromptBusy("preview");
    setPromptError(null);
    try {
      const promptAssembly = promptAssemblyFromTemplates(promptTemplates, files, {
        includeAllFiles: true,
      });
      const result = await ipc.agentPromptPreview({
        machineId: machine.id,
        actorId,
        sampleMessage,
        promptAssembly,
      });
      setPreview(result);
    } catch (err) {
      setPromptError(errorText(err));
    } finally {
      setPromptBusy(null);
    }
  }

  async function savePromptAssembly() {
    if (!canSaveAssembly) return;
    setAssemblySaving(true);
    setPromptError(null);
    try {
      const promptAssembly = promptAssemblyFromTemplates(promptTemplates, files);
      await ipc.agentUpdate({
        machineId: machine.id,
        actorId,
        promptAssembly,
      });
    } catch (err) {
      setPromptError(errorText(err));
    } finally {
      setAssemblySaving(false);
    }
  }

  return (
    <HostDetailSection
      title="Prompt Studio"
      action={
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={savePromptAssembly}
            disabled={!canSaveAssembly || assemblySaving}
            className="rounded-lg border-[#dfe3ec] bg-white"
          >
            {assemblySaving ? (
              <Loader2 className="animate-spin" size={15} />
            ) : (
              <Check size={15} />
            )}
            Save Assembly
          </Button>
        </div>
      }
    >
      <div className="space-y-5">
        <div className="grid gap-4 2xl:grid-cols-[minmax(0,1fr)_360px]">
          <section className="min-w-0 rounded-xl border border-[#edf0f5] bg-white p-4">
            <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
              <div className="text-sm font-bold text-[#111827]">Prompt Templates</div>
              <Button
                variant="outline"
                size="sm"
                onClick={resetPromptTemplates}
                disabled={!canSaveAssembly}
                className="h-8 rounded-lg border-[#dfe3ec] bg-white"
              >
                Reset Default
              </Button>
            </div>
            <div className="grid gap-3 xl:grid-cols-2">
              <label className="block min-w-0">
                <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                  System Prompt
                </span>
                <Textarea
                  value={promptTemplates.system}
                  onChange={(event) => updatePromptTemplate("system", event.target.value)}
                  className="mt-2 min-h-56 rounded-lg border-[#dfe3ec] bg-white font-mono text-xs shadow-none"
                  disabled={!canSaveAssembly}
                />
              </label>
              <label className="block min-w-0">
                <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                  User Prompt
                </span>
                <Textarea
                  value={promptTemplates.user}
                  onChange={(event) => updatePromptTemplate("user", event.target.value)}
                  className="mt-2 min-h-56 rounded-lg border-[#dfe3ec] bg-white font-mono text-xs shadow-none"
                  disabled={!canSaveAssembly}
                />
              </label>
            </div>
          </section>

          <section className="min-w-0 rounded-xl border border-[#edf0f5] bg-[#fbfbfd] p-4">
            <div className="mb-3 flex items-center justify-between gap-2">
              <div className="text-sm font-bold text-[#111827]">Variables</div>
              <Badge variant={preview ? "secondary" : "outline"}>
                {preview ? "preview" : "no preview"}
              </Badge>
            </div>
            <div className="flex max-h-32 flex-wrap gap-1.5 overflow-y-auto pr-1 soft-scrollbar">
              {variableItems.map((item) => {
                const selected = selectedVariable?.key === item.key;
                const part = preview?.parts.find((previewPart) => previewPart.key === item.key);
                return (
                  <button
                    key={item.key}
                    type="button"
                    className={cn(
                      "rounded-md border px-2 py-1 font-mono text-[11px] font-semibold transition-colors",
                      selected
                        ? "border-[#8f82ff] bg-white text-[#503ed4] ring-2 ring-[#e4e0ff]"
                        : "border-[#dfe3ec] bg-white text-[#596174] hover:border-[#bdb7ff] hover:text-[#503ed4]",
                    )}
                    title={part ? `${item.label} / ${part.bytes} bytes` : item.label}
                    onClick={() => setSelectedVariableKey(item.key)}
                  >
                    {`{${item.key}}`}
                  </button>
                );
              })}
            </div>
            <PromptVariableInspector
              variable={selectedVariable}
              part={selectedVariablePart}
              previewReady={Boolean(preview)}
            />
          </section>
        </div>

        <section className="rounded-xl border border-[#edf0f5] bg-white p-4">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <div className="text-sm font-bold text-[#111827]">Profile Prompt Files</div>
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={refreshPromptFiles}
                disabled={!canReadFiles || promptBusy === "files"}
                className="rounded-lg border-[#dfe3ec] bg-white"
              >
                {promptBusy === "files" ? (
                  <Loader2 className="animate-spin" size={15} />
                ) : (
                  <Folder size={15} />
                )}
                Refresh
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={startNewPromptFile}
                disabled={!canWriteFiles}
                className="rounded-lg border-[#dfe3ec] bg-white"
              >
                <Plus size={15} />
                New
              </Button>
            </div>
          </div>
          <div className="grid gap-4 xl:grid-cols-[280px_minmax(0,1fr)]">
            <div className="min-w-0">
              {files.length === 0 ? (
                <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] px-3 py-4 text-sm text-[#667085]">
                  No prompt files in this profile.
                </div>
              ) : (
                <div className="max-h-72 space-y-1 overflow-y-auto rounded-xl border border-[#edf0f5] bg-[#fbfbfd] p-2 soft-scrollbar">
                  {files.map((file) => (
                    <button
                      key={file.path}
                      type="button"
                      className={cn(
                        "grid w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-lg px-2 py-1.5 text-left text-xs transition-colors",
                        filePath === file.path
                          ? "bg-white text-[#503ed4] shadow-sm"
                          : "text-[#596174] hover:bg-white",
                      )}
                      onClick={() => {
                        setFilePath(file.path);
                        void openPromptFile(file.path);
                      }}
                    >
                      <span className="truncate font-mono">{file.path}</span>
                      <span className="text-[#9aa1ae]">{file.bytes}b</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
            <div className="min-w-0 space-y-3">
              <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto]">
                <Input
                  value={filePath}
                  onChange={(event) => setFilePath(event.target.value)}
                  placeholder="prompts/example.md"
                  className="h-9 rounded-lg border-[#dfe3ec] bg-white font-mono text-xs shadow-none"
                  disabled={!canReadFiles && !canWriteFiles}
                />
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => void openPromptFile()}
                  disabled={!canReadFiles || promptBusy === "read" || !filePath.trim()}
                  className="rounded-lg border-[#dfe3ec] bg-white"
                >
                  {promptBusy === "read" ? (
                    <Loader2 className="animate-spin" size={15} />
                  ) : (
                    <FileText size={15} />
                  )}
                  Open
                </Button>
                <Button
                  size="sm"
                  onClick={savePromptFile}
                  disabled={!canWriteFiles || promptBusy === "write" || !filePath.trim() || !fileDirty}
                  className="rounded-lg"
                >
                  {promptBusy === "write" ? (
                    <Loader2 className="animate-spin" size={15} />
                  ) : (
                    <Check size={15} />
                  )}
                  Save
                </Button>
              </div>
              <div className="min-w-0 truncate text-xs text-[#667085]">
                {selectedFile ? `${selectedFile.bytes} bytes` : filePath ? "New file" : "No file selected"}
              </div>
              <Textarea
                value={fileContent}
                onChange={(event) => {
                  setFileContent(event.target.value);
                  setFileDirty(true);
                }}
                placeholder="Prompt file content"
                className="min-h-64 rounded-lg border-[#dfe3ec] bg-white font-mono text-xs shadow-none"
                disabled={!canReadFiles && !canWriteFiles}
                readOnly={!canWriteFiles}
              />
            </div>
          </div>
        </section>

        <section className="rounded-xl border border-[#edf0f5] bg-white p-4">
          <div className="mb-3 grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto]">
            <label className="block min-w-0">
              <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                Preview Message
              </span>
              <Textarea
                value={sampleMessage}
                onChange={(event) => setSampleMessage(event.target.value)}
                placeholder="Preview-only text. Real requests replace this with the actual handoff."
                className="mt-2 min-h-20 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                disabled={!canPreview}
              />
            </label>
            <div className="flex items-end">
              <Button
                variant="outline"
                size="sm"
                onClick={refreshPreview}
                disabled={!canPreview || promptBusy === "preview"}
                className="rounded-lg border-[#dfe3ec] bg-white"
              >
                {promptBusy === "preview" ? (
                  <Loader2 className="animate-spin" size={15} />
                ) : (
                  <RefreshCw size={15} />
                )}
                Preview
              </Button>
            </div>
          </div>
          {promptError && (
            <div className="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm font-medium text-red-700">
              {promptError}
            </div>
          )}
          {!canPreview && (
            <div className="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-sm font-medium text-amber-800">
              Prompt preview is not available on this host.
            </div>
          )}
          {preview && (
            <>
              <div className="grid gap-3 xl:grid-cols-3">
                <PromptOutputPreview title="System" content={preview.outputs.system} />
                <PromptOutputPreview title="User" content={preview.outputs.user} />
                <PromptOutputPreview title="Full" content={preview.outputs.full} />
              </div>
              <details className="rounded-xl border border-[#edf0f5] bg-[#fbfbfd] px-3 py-2">
                <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wide text-[#667085]">
                  Provider Binding
                </summary>
                <pre className="mt-2 max-h-28 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-white p-2 font-mono text-xs text-[#485063] soft-scrollbar">
                  {JSON.stringify(preview.bindings, null, 2)}
                </pre>
              </details>
              {preview.warnings.length > 0 && (
                <div className="whitespace-pre-line rounded-xl border border-amber-200 bg-amber-50 px-3 py-2 text-xs font-medium text-amber-800">
                  {preview.warnings.join("\n")}
                </div>
              )}
            </>
          )}
        </section>
      </div>
    </HostDetailSection>
  );
}

function promptTemplatesFromAssembly(
  assembly?: Record<string, unknown> | null,
): PromptTemplateDraft {
  if (!assembly || isLegacyDefaultPromptAssembly(assembly)) {
    return {
      system: defaultSystemPromptTemplate,
      user: defaultUserPromptTemplate,
    };
  }
  const outputs = objectRecord(assembly.outputs);
  return {
    system: promptOutputTemplate(outputs?.system, defaultSystemPromptTemplate),
    user: promptOutputTemplate(outputs?.user, defaultUserPromptTemplate),
  };
}

function promptOutputTemplate(value: unknown, fallback: string): string {
  const output = objectRecord(value);
  if (!output) return fallback;
  if (typeof output.template === "string") return output.template;
  const include = stringArray(output.include);
  if (include.length > 0) {
    return include.map((key) => `{${key}}`).join("\n\n");
  }
  const preset = typeof output.preset === "string" ? output.preset : "";
  const presetParts = promptPresetParts[preset];
  if (presetParts) {
    return presetParts.map((key) => `{${key}}`).join("\n\n");
  }
  return fallback;
}

function promptAssemblyFromTemplates(
  templates: PromptTemplateDraft,
  files: AgentFileEntry[],
  options: PromptAssemblyBuildOptions = {},
): Record<string, unknown> {
  const referencedFileKeys = promptTemplateVariables(`${templates.system}\n${templates.user}`)
    .filter((key) => key.startsWith("file."))
    .map((key) => key.slice("file.".length));
  const fileSpecs = files
    .filter(
      (file) =>
        options.includeAllFiles || referencedFileKeys.includes(promptFileKey(file.path)),
    )
    .map((file) => ({
      key: promptFileKey(file.path),
      title: promptFileTitle(file.path),
      root: "profile",
      path: file.path,
      optional: true,
    }));
  return {
    ...(fileSpecs.length > 0 ? { files: fileSpecs } : {}),
    outputs: {
      system: {
        template: templates.system.trim() || defaultSystemPromptTemplate,
      },
      user: {
        template: templates.user.trim() || defaultUserPromptTemplate,
      },
      full: {
        include: ["prompt.system", "prompt.user"],
      },
    },
  };
}

function promptTemplateVariables(template: string) {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const match of template.matchAll(/\{([^{}]+)\}/g)) {
    const key = match[1]?.trim();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    out.push(key);
  }
  return out;
}

function isLegacyDefaultPromptAssembly(assembly: Record<string, unknown>) {
  const files = Array.isArray(assembly.files) ? assembly.files : [];
  const hasLegacySystemFile = files.some((file) => {
    const item = objectRecord(file);
    return item?.key === "profile_system" && item.path === "prompts/system.md";
  });
  const hasLegacyUserFile = files.some((file) => {
    const item = objectRecord(file);
    return item?.key === "profile_user" && item.path === "prompts/user.md";
  });
  if (!hasLegacySystemFile || !hasLegacyUserFile) return false;

  const outputs = objectRecord(assembly.outputs);
  const system = objectRecord(outputs?.system);
  const user = objectRecord(outputs?.user);
  return (
    stringArray(system?.include).includes("file.profile_system") &&
    stringArray(user?.include).includes("file.profile_user")
  );
}

function promptFileKey(path: string) {
  const normalized = path
    .trim()
    .replace(/\\/g, "/")
    .split("/")
    .filter(Boolean)
    .join(".")
    .replace(/[^A-Za-z0-9_.-]+/g, "_")
    .replace(/_+/g, "_")
    .replace(/^[_ .-]+|[_ .-]+$/g, "");
  return normalized || "profile_prompt";
}

function promptFileTitle(path: string) {
  return path
    .trim()
    .replace(/\\/g, "/")
    .split("/")
    .filter(Boolean)
    .pop() || path;
}

function objectRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}

function stringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}


export function PromptOutputPreview({
  title,
  content,
}: {
  title: string;
  content: string;
}) {
  return (
    <div className="min-w-0 rounded-xl border border-[#edf0f5] bg-[#fbfbfd] p-3">
      <div className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
        {title}
      </div>
      <pre className="max-h-56 min-h-32 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-white p-3 font-mono text-xs leading-5 text-[#303849] soft-scrollbar">
        {content || "-"}
      </pre>
    </div>
  );
}


export function PromptVariableInspector({
  variable,
  part,
  previewReady,
}: {
  variable: { key: string; label: string } | null;
  part: AgentPromptPreviewPart | null;
  previewReady: boolean;
}) {
  if (!variable) {
    return (
      <div className="mt-3 rounded-lg border border-dashed border-[#dfe3ec] bg-white p-3 text-sm text-[#667085]">
        No variable selected.
      </div>
    );
  }
  const content = part?.content ?? "";
  const meta = !previewReady
    ? "preview required"
    : part
      ? `${part.source} / ${part.bytes}b`
      : "not rendered";
  const body = !previewReady
    ? "Run Preview to inspect this variable."
    : part
      ? content || "empty"
      : "Run Preview again to include the latest profile prompt files.";

  return (
    <div className="mt-3 rounded-lg border border-[#dfe3ec] bg-white">
      <div className="flex min-h-10 flex-wrap items-center justify-between gap-2 border-b border-[#edf0f5] px-3 py-2">
        <div className="min-w-0">
          <div className="truncate font-mono text-xs font-bold text-[#111827]">
            {`{${variable.key}}`}
          </div>
          <div className="mt-0.5 truncate text-[11px] font-medium text-[#667085]">
            {variable.label}
          </div>
        </div>
        <span className="rounded-md bg-[#f2f4f7] px-2 py-1 font-mono text-[11px] font-semibold text-[#667085]">
          {meta}
        </span>
      </div>
      <pre className="max-h-64 min-h-36 overflow-auto whitespace-pre-wrap break-words p-3 font-mono text-xs leading-5 text-[#303849] soft-scrollbar">
        {body}
      </pre>
    </div>
  );
}

