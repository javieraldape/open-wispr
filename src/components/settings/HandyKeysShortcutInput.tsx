import React, { useEffect, useState, useRef, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { formatKeyCombination } from "../../lib/utils/keyboard";
import { ResetButton } from "../ui/ResetButton";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";
import { commands } from "@/bindings";
import { toast } from "sonner";

interface HandyKeysShortcutInputProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  shortcutId: string;
  disabled?: boolean;
}

interface HandyKeysEvent {
  modifiers: string[];
  key: string | null;
  is_key_down: boolean;
  hotkey_string: string;
}

const MODIFIER_ALIASES: Record<string, string> = {
  alt_left: "alt",
  alt_right: "alt",
  command_left: "command",
  command_right: "command",
  control_left: "ctrl",
  control_right: "ctrl",
  ctrl_left: "ctrl",
  ctrl_right: "ctrl",
  option_left: "option",
  option_right: "option",
  shift_left: "shift",
  shift_right: "shift",
  super_left: "super",
  super_right: "super",
};

const MODIFIER_KEYS = new Set([
  "alt",
  "cmd",
  "command",
  "control",
  "ctrl",
  "fn",
  "option",
  "shift",
  "super",
]);

const normalizeHandyHotkey = (hotkey: string): string =>
  hotkey
    .split("+")
    .map((part) => part.trim().toLowerCase())
    .filter(Boolean)
    .map((part) => MODIFIER_ALIASES[part] ?? part)
    .join("+");

const isSupportedShortcut = (hotkey: string): boolean => {
  const parts = normalizeHandyHotkey(hotkey).split("+").filter(Boolean);
  if (parts.length === 1 && parts[0] === "fn") {
    return true;
  }

  const hasNonModifierKey = parts.some((part) => !MODIFIER_KEYS.has(part));
  if (hasNonModifierKey) {
    return true;
  }

  return new Set(parts.filter((part) => MODIFIER_KEYS.has(part))).size >= 2;
};

const ensureCommandOk = (
  result: { status: "ok" | "error"; error?: string },
  fallbackMessage: string,
) => {
  if (result.status === "error") {
    throw new Error(result.error || fallbackMessage);
  }
};

export const HandyKeysShortcutInput: React.FC<HandyKeysShortcutInputProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
  shortcutId,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const {
    getSetting,
    updateBinding,
    resetBinding,
    refreshSettings,
    isUpdating,
    isLoading,
  } = useSettings();
  const [isRecording, setIsRecording] = useState(false);
  const [currentKeys, setCurrentKeys] = useState<string>("");
  const shortcutRef = useRef<HTMLDivElement | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);
  // Use a ref to track currentKeys for the event handler (avoids stale closure)
  const currentKeysRef = useRef<string>("");
  const keyedShortcutRef = useRef<string>("");
  const modifierOnlyShortcutRef = useRef<string>("");
  const isRecordingRef = useRef(false);
  const osType = useOsType();

  const bindings = getSetting("bindings") || {};

  const detachListener = useCallback(() => {
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }
  }, []);

  const resetRecordingState = useCallback(() => {
    setIsRecording(false);
    setCurrentKeys("");
    currentKeysRef.current = "";
    keyedShortcutRef.current = "";
    modifierOnlyShortcutRef.current = "";
    isRecordingRef.current = false;
  }, []);

  const stopBackendRecording = useCallback(async (reason: string) => {
    console.debug(`Stopping handy-keys shortcut capture: ${reason}`);
    const result = await commands.stopHandyKeysRecording();
    ensureCommandOk(result, "Failed to stop handy-keys recording");
  }, []);

  const cancelRecording = useCallback(async () => {
    if (!isRecordingRef.current) return;

    console.debug("Canceling handy-keys shortcut capture");
    isRecordingRef.current = false;
    detachListener();

    try {
      await stopBackendRecording("cancel");
    } catch (error) {
      console.error("Failed to stop handy-keys shortcut capture:", error);
      await refreshSettings();
    }

    resetRecordingState();
  }, [
    detachListener,
    refreshSettings,
    resetRecordingState,
    stopBackendRecording,
  ]);

  const commitRecording = useCallback(
    async (keysToCommit: string) => {
      if (!isRecordingRef.current) return;

      if (!isSupportedShortcut(keysToCommit)) {
        currentKeysRef.current = "";
        setCurrentKeys("");
        return;
      }

      console.debug("Committing handy-keys shortcut capture");
      isRecordingRef.current = false;
      detachListener();

      try {
        await updateBinding(shortcutId, keysToCommit);
        try {
          await stopBackendRecording("commit");
        } catch (stopError) {
          console.error(
            "Failed to stop handy-keys shortcut capture:",
            stopError,
          );
          await refreshSettings();
        }
      } catch (error) {
        console.error("Failed to change handy-keys binding:", error);
        try {
          await stopBackendRecording("failed commit");
        } catch (stopError) {
          console.error(
            "Failed to stop handy-keys shortcut capture:",
            stopError,
          );
        }
        await refreshSettings();
        toast.error(
          t("settings.general.shortcut.errors.set", {
            error: String(error),
          }),
        );
      } finally {
        resetRecordingState();
      }
    },
    [
      detachListener,
      refreshSettings,
      resetRecordingState,
      shortcutId,
      stopBackendRecording,
      t,
      updateBinding,
    ],
  );

  useEffect(() => {
    return () => {
      if (isRecordingRef.current) {
        void cancelRecording();
      }
    };
  }, [cancelRecording]);

  // Handle click outside
  useEffect(() => {
    if (!isRecording) return;

    const handleClickOutside = (e: MouseEvent) => {
      if (
        shortcutRef.current &&
        !shortcutRef.current.contains(e.target as Node)
      ) {
        void cancelRecording();
      }
    };

    window.addEventListener("click", handleClickOutside);
    return () => window.removeEventListener("click", handleClickOutside);
  }, [isRecording, cancelRecording]);

  // Start recording a new shortcut
  const startRecording = useCallback(async () => {
    if (disabled || isRecordingRef.current) return;

    isRecordingRef.current = true;
    currentKeysRef.current = "";
    keyedShortcutRef.current = "";
    modifierOnlyShortcutRef.current = "";

    setIsRecording(true);
    setCurrentKeys("");

    try {
      try {
        await stopBackendRecording("stale session before start");
      } catch (error) {
        console.warn("Failed to stop stale handy-keys capture:", error);
      }

      if (!isRecordingRef.current) return;

      const unlisten = await listen<HandyKeysEvent>(
        "handy-keys-event",
        (event) => {
          if (!isRecordingRef.current) return;

          const { hotkey_string, is_key_down, key, modifiers } = event.payload;
          const normalizedHotkey = normalizeHandyHotkey(hotkey_string);

          if (is_key_down && key && normalizedHotkey) {
            keyedShortcutRef.current = normalizedHotkey;
            currentKeysRef.current = normalizedHotkey;
            setCurrentKeys(normalizedHotkey);
          } else if (is_key_down && !key && normalizedHotkey) {
            if (isSupportedShortcut(normalizedHotkey)) {
              modifierOnlyShortcutRef.current = normalizedHotkey;
              currentKeysRef.current = normalizedHotkey;
              setCurrentKeys(normalizedHotkey);
            } else if (!currentKeysRef.current) {
              currentKeysRef.current = normalizedHotkey;
              setCurrentKeys(normalizedHotkey);
            }
          } else if (!is_key_down && key) {
            const shortcutToCommit =
              keyedShortcutRef.current ||
              normalizedHotkey ||
              currentKeysRef.current;
            if (shortcutToCommit) {
              void commitRecording(shortcutToCommit);
            }
          } else if (
            !is_key_down &&
            !key &&
            modifiers.length === 0 &&
            !keyedShortcutRef.current &&
            modifierOnlyShortcutRef.current
          ) {
            void commitRecording(modifierOnlyShortcutRef.current);
          }
        },
      );
      unlistenRef.current = unlisten;

      if (!isRecordingRef.current) {
        detachListener();
        return;
      }

      console.debug("Starting handy-keys shortcut capture");
      const startResult = await commands.startHandyKeysRecording(shortcutId);
      ensureCommandOk(startResult, "Failed to start handy-keys recording");

      if (!isRecordingRef.current) {
        try {
          await stopBackendRecording("canceled setup");
        } catch (stopError) {
          console.error(
            "Failed to stop handy-keys shortcut capture:",
            stopError,
          );
        }
      }
    } catch (error) {
      console.error("Failed to start handy-keys shortcut capture:", error);
      toast.error(
        t("settings.general.shortcut.errors.set", { error: String(error) }),
      );

      detachListener();
      try {
        await stopBackendRecording("failed setup");
      } catch (stopError) {
        console.error("Failed to stop handy-keys shortcut capture:", stopError);
      }
      await refreshSettings();
      resetRecordingState();
    }
  }, [
    commitRecording,
    detachListener,
    disabled,
    refreshSettings,
    resetRecordingState,
    shortcutId,
    stopBackendRecording,
    t,
  ]);

  // Format the current shortcut keys being recorded
  const formatCurrentKeys = (): string => {
    if (!currentKeys) return t("settings.general.shortcut.pressKeys");
    return formatKeyCombination(currentKeys, osType);
  };

  // If still loading, show loading state
  if (isLoading) {
    return (
      <SettingContainer
        title={t("settings.general.shortcut.title")}
        description={t("settings.general.shortcut.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-[13px] text-text-secondary">
          {t("settings.general.shortcut.loading")}
        </div>
      </SettingContainer>
    );
  }

  // If no bindings are loaded, show empty state
  if (Object.keys(bindings).length === 0) {
    return (
      <SettingContainer
        title={t("settings.general.shortcut.title")}
        description={t("settings.general.shortcut.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-[13px] text-text-secondary">
          {t("settings.general.shortcut.none")}
        </div>
      </SettingContainer>
    );
  }

  const binding = bindings[shortcutId];
  if (!binding) {
    return (
      <SettingContainer
        title={t("settings.general.shortcut.title")}
        description={t("settings.general.shortcut.notFound")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-[13px] text-text-secondary">
          {t("settings.general.shortcut.none")}
        </div>
      </SettingContainer>
    );
  }

  // Get translated name and description for the binding
  const translatedName = t(
    `settings.general.shortcut.bindings.${shortcutId}.name`,
    binding.name,
  );
  const translatedDescription = t(
    `settings.general.shortcut.bindings.${shortcutId}.description`,
    binding.description,
  );

  return (
    <SettingContainer
      title={translatedName}
      description={translatedDescription}
      descriptionMode={descriptionMode}
      grouped={grouped}
      disabled={disabled}
      layout="horizontal"
    >
      <div className="flex items-center gap-1">
        {isRecording ? (
          <div
            ref={shortcutRef}
            className="settings-shortcut-field settings-shortcut-field-active"
          >
            {formatCurrentKeys()}
          </div>
        ) : (
          <div
            className="settings-shortcut-field cursor-pointer"
            onClick={startRecording}
          >
            {formatKeyCombination(binding.current_binding, osType)}
          </div>
        )}
        <ResetButton
          onClick={() => resetBinding(shortcutId)}
          disabled={isUpdating(`binding_${shortcutId}`)}
        />
      </div>
    </SettingContainer>
  );
};
