import {
  checkAccessibilityPermission,
  checkInputMonitoringPermission,
} from "tauri-plugin-macos-permissions-api";
import { commands } from "@/bindings";

export interface MacOSKeyboardReadiness {
  hasAccessibilityPermission: boolean;
  hasInputMonitoringPermission: boolean;
  hasRawKeyboardPermissions: boolean;
  isKeyboardOperational: boolean;
  usedOperationalFallback: boolean;
}

interface MacOSKeyboardReadinessOptions {
  allowOperationalFallback?: boolean;
}

export const initializeMacOSKeyboardInput = async (): Promise<boolean> => {
  try {
    const [enigoResult, shortcutsResult] = await Promise.all([
      commands.initializeEnigo(),
      commands.initializeShortcuts(),
    ]);

    return enigoResult.status === "ok" && shortcutsResult.status === "ok";
  } catch (error) {
    console.warn("Failed to initialize keyboard input:", error);
    return false;
  }
};

export const getMacOSKeyboardReadiness = async ({
  allowOperationalFallback = true,
}: MacOSKeyboardReadinessOptions = {}): Promise<MacOSKeyboardReadiness> => {
  const [hasAccessibilityPermission, hasInputMonitoringPermission] =
    await Promise.all([
      checkAccessibilityPermission(),
      checkInputMonitoringPermission(),
    ]);
  const hasRawKeyboardPermissions = hasAccessibilityPermission;

  const shouldCheckOperational =
    hasRawKeyboardPermissions || allowOperationalFallback;
  const isKeyboardOperational = shouldCheckOperational
    ? await initializeMacOSKeyboardInput()
    : false;

  return {
    hasAccessibilityPermission,
    hasInputMonitoringPermission,
    hasRawKeyboardPermissions,
    isKeyboardOperational,
    usedOperationalFallback:
      !hasRawKeyboardPermissions && isKeyboardOperational,
  };
};
