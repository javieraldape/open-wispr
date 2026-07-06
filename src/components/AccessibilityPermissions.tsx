import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { requestAccessibilityPermission } from "tauri-plugin-macos-permissions-api";
import { AlertTriangle, Check, Loader2 } from "lucide-react";
import {
  getMacOSKeyboardReadiness,
  type MacOSKeyboardReadiness,
} from "@/lib/utils/macosKeyboardReadiness";

// macOS keyboard control needs Accessibility for the current input backends.
// Input Monitoring is not a required gate for the default HandyKeys path.
const AccessibilityPermissions: React.FC = () => {
  const { t } = useTranslation();
  const [keyboardReadiness, setKeyboardReadiness] =
    useState<MacOSKeyboardReadiness | null>(null);
  const [isRequesting, setIsRequesting] = useState(false);

  // Accessibility permissions are only required on macOS
  const isMacOS = type() === "macos";

  const refresh = useCallback(async (): Promise<void> => {
    setKeyboardReadiness(await getMacOSKeyboardReadiness());
  }, []);

  // Check on mount, and re-check whenever the window regains focus (i.e. the
  // user returns from System Settings) so the statuses update on their own.
  useEffect(() => {
    if (!isMacOS) return;

    refresh().catch((error) =>
      console.error("Error checking permissions:", error),
    );

    const onFocus = () => {
      refresh().catch((error) =>
        console.error("Error re-checking permissions:", error),
      );
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [isMacOS, refresh]);

  // Open the System Settings pane for the first permission still missing.
  const handleOpenSettings = async (): Promise<void> => {
    setIsRequesting(true);
    try {
      if (!keyboardReadiness?.hasAccessibilityPermission) {
        await requestAccessibilityPermission();
      }
    } catch (error) {
      console.error("Error requesting permissions:", error);
    } finally {
      // Statuses refresh on window focus once the user returns from Settings.
      setIsRequesting(false);
    }
  };

  // Skip rendering once raw permissions are granted, or when macOS reports stale
  // permission checks but the keyboard backend is already operational.
  if (
    !isMacOS ||
    !keyboardReadiness ||
    keyboardReadiness.hasRawKeyboardPermissions ||
    keyboardReadiness.isKeyboardOperational
  ) {
    return null;
  }

  const PermissionRow: React.FC<{ label: string; granted: boolean }> = ({
    label,
    granted,
  }) => (
    <div className="flex min-h-[38px] items-center gap-2 px-4 py-2 text-[13px]">
      {granted ? (
        <Check className="h-4 w-4 shrink-0 text-ok" />
      ) : (
        <span className="flex h-4 w-4 shrink-0 items-center justify-center">
          <span className="h-1.5 w-1.5 rounded-full bg-mid-gray" />
        </span>
      )}
      <span className={granted ? "text-text/50" : "text-text"}>{label}</span>
      {granted && (
        <span className="text-xs text-text/40">
          {t("onboarding.permissions.granted")}
        </span>
      )}
    </div>
  );

  return (
    <div className="w-full overflow-hidden rounded-lg bg-card settings-card-ring">
      <div className="grid min-h-[56px] grid-cols-[1fr_auto] items-center gap-3 px-4 py-3">
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            <span className="flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-full bg-yellow-500/15 text-yellow-700 dark:text-yellow-400">
              <AlertTriangle className="h-3.5 w-3.5" />
            </span>
            <p className="min-w-0 text-[13px] font-semibold text-text">
              {t("onboarding.permissions.keyboardTitle")}
            </p>
          </div>
          <p className="mt-1 text-[12.5px] leading-[17px] text-text-secondary">
            {t("accessibility.permissionsDescription")}
          </p>
        </div>
        <button
          onClick={handleOpenSettings}
          disabled={isRequesting}
          className="flex min-h-8 shrink-0 cursor-pointer items-center gap-1.5 rounded-[5.5px] bg-accent px-3 py-1 text-[13px] font-medium text-white shadow-[0_0_0_.5px_rgba(0,0,0,.12),0_.5px_1px_rgba(0,0,0,.18)] transition-colors hover:bg-accent/90 disabled:cursor-not-allowed disabled:opacity-40"
        >
          {isRequesting && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {t("accessibility.openSettings")}
        </button>
      </div>
      <div className="border-t border-separator">
        <PermissionRow
          label={t("onboarding.permissions.accessibility.title")}
          granted={keyboardReadiness.hasAccessibilityPermission}
        />
      </div>
    </div>
  );
};

export default AccessibilityPermissions;
