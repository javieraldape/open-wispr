import type { OSType } from "../../lib/utils/keyboard";

type KeyboardImplementationChangeResult = {
  success: boolean;
};

type CommandResult<T> =
  { status: "ok"; data: T } | { status: "error"; error: string };

interface EnsureHandyKeysShortcutEditingBackendOptions {
  osType: OSType;
  shortcutId: string;
  keyboardImplementation?: string;
  changeKeyboardImplementationSetting: (
    implementation: "handy_keys",
  ) => Promise<CommandResult<KeyboardImplementationChangeResult>>;
  refreshSettings: () => Promise<void>;
}

export const shouldUseHandyKeysShortcutInput = (
  osType: OSType,
  shortcutId: string,
  keyboardImplementation?: string,
): boolean => {
  if (keyboardImplementation === "handy_keys") {
    return true;
  }

  return osType === "macos" && shortcutId === "transcribe";
};

export const ensureHandyKeysShortcutEditingBackend = async ({
  osType,
  shortcutId,
  keyboardImplementation,
  changeKeyboardImplementationSetting,
  refreshSettings,
}: EnsureHandyKeysShortcutEditingBackendOptions): Promise<boolean> => {
  if (keyboardImplementation === "handy_keys") {
    return true;
  }

  if (osType !== "macos" || shortcutId !== "transcribe") {
    return false;
  }

  const result = await changeKeyboardImplementationSetting("handy_keys");
  if (result.status === "error") {
    throw new Error(result.error || "Failed to switch to handy-keys");
  }

  if (!result.data.success) {
    throw new Error("Failed to switch to handy-keys");
  }

  await refreshSettings();
  return true;
};
