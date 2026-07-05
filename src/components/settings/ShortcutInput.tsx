import React from "react";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";
import { GlobalShortcutInput } from "./GlobalShortcutInput";
import { HandyKeysShortcutInput } from "./HandyKeysShortcutInput";
import { shouldUseHandyKeysShortcutInput } from "./shortcutInputRouting";

interface ShortcutInputProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  shortcutId: string;
  disabled?: boolean;
}

/**
 * Wrapper component that selects the appropriate shortcut input implementation
 * based on the keyboard_implementation setting.
 *
 * - "tauri" (default): Uses GlobalShortcutInput with JS keyboard events
 * - "handy_keys": Uses HandyKeysShortcutInput with backend key events
 * - macOS Transcribe: Uses HandyKeysShortcutInput so the fn key can be captured
 */
export const ShortcutInput: React.FC<ShortcutInputProps> = (props) => {
  const { getSetting } = useSettings();
  const osType = useOsType();
  const keyboardImplementation = getSetting("keyboard_implementation");

  if (
    shouldUseHandyKeysShortcutInput(
      osType,
      props.shortcutId,
      keyboardImplementation,
    )
  ) {
    return <HandyKeysShortcutInput {...props} />;
  }

  return <GlobalShortcutInput {...props} />;
};
