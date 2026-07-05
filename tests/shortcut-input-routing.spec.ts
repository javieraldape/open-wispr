import { expect, test } from "@playwright/test";
import {
  ensureHandyKeysShortcutEditingBackend,
  shouldUseHandyKeysShortcutInput,
} from "../src/components/settings/shortcutInputRouting";

test.describe("shortcut input routing", () => {
  test("routes macOS transcribe editing through HandyKeys even when Tauri is active", () => {
    expect(
      shouldUseHandyKeysShortcutInput("macos", "transcribe", "tauri"),
    ).toBe(true);
  });

  test("keeps non-transcribe Tauri shortcuts on the global shortcut input", () => {
    expect(shouldUseHandyKeysShortcutInput("macos", "cancel", "tauri")).toBe(
      false,
    );
  });

  test("uses HandyKeys for every shortcut when HandyKeys is active", () => {
    expect(
      shouldUseHandyKeysShortcutInput("windows", "cancel", "handy_keys"),
    ).toBe(true);
  });

  test("switches macOS transcribe from Tauri before starting HandyKeys recording", async () => {
    const calls: string[] = [];

    const ready = await ensureHandyKeysShortcutEditingBackend({
      osType: "macos",
      shortcutId: "transcribe",
      keyboardImplementation: "tauri",
      changeKeyboardImplementationSetting: async (implementation) => {
        calls.push(`switch:${implementation}`);
        return { status: "ok", data: { success: true } };
      },
      refreshSettings: async () => {
        calls.push("refresh");
      },
    });

    if (ready) {
      calls.push("start-recording");
    }

    expect(calls).toEqual(["switch:handy_keys", "refresh", "start-recording"]);
  });
});
