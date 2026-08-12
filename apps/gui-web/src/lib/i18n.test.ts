import { describe, expect, it, vi } from "vitest";
import {
  LANGUAGE_PREFERENCE_STORAGE_KEY,
  loadLanguagePreference,
  resolveLanguagePreference,
  saveLanguagePreference,
  translate,
} from "@/lib/i18n";

describe("i18n", () => {
  it("resolves system Chinese variants and defaults other locales to English", () => {
    expect(resolveLanguagePreference("system", ["zh-TW", "en-US"])).toBe("zh-CN");
    expect(resolveLanguagePreference("system", ["en-US", "zh-CN"])).toBe("en");
    expect(resolveLanguagePreference("zh-CN", ["en-US"])).toBe("zh-CN");
  });

  it("translates parameters and falls back to the English source string", () => {
    expect(translate("zh-CN", "Will connect to {{server}} before creating.", {
      server: "Loom",
    })).toBe("创建前将连接到 Loom。");
    expect(translate("zh-CN", "Uncatalogued English", { count: 2 })).toBe(
      "Uncatalogued English",
    );
  });

  it("translates the first-run setup journey and its dynamic status text", () => {
    expect(translate("zh-CN", "First run setup")).toBe("首次运行设置");
    expect(translate("zh-CN", "Set your Loom identity")).toBe("设置你的 Loom 身份");
    expect(translate("zh-CN", "Choose a server")).toBe("选择服务器");
    expect(translate("zh-CN", "Actors")).toBe("成员");
    expect(translate("zh-CN", "Start your local Host")).toBe("启动本地主机");
    expect(translate("zh-CN", "Create your first agent")).toBe("创建你的第一个智能体");
    expect(translate("zh-CN", "{{count}} runtimes detected", { count: 2 })).toBe(
      "检测到 2 个运行时",
    );
    expect(translate("zh-CN", "{{name}} is online.", { name: "Local Host" })).toBe(
      "Local Host 已在线。",
    );
    expect(translate("en", "Back to overview")).toBe("Back to overview");
  });

  it("loads and persists a valid language preference safely", () => {
    const getItem = vi.fn().mockReturnValue("zh-CN");
    const setItem = vi.fn();
    const storage = { getItem, setItem };

    expect(loadLanguagePreference(storage)).toBe("zh-CN");
    saveLanguagePreference("en", storage);
    expect(setItem).toHaveBeenCalledWith(LANGUAGE_PREFERENCE_STORAGE_KEY, "en");

    getItem.mockReturnValue("unsupported");
    expect(loadLanguagePreference(storage)).toBe("system");
  });
});
