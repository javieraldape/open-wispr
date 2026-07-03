import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { type } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  checkInputMonitoringPermission,
  requestAccessibilityPermission,
} from "tauri-plugin-macos-permissions-api";
import { Check, Loader2 } from "lucide-react";

// macOS keyboard control needs TWO distinct permissions — Accessibility (to type
// text) and Input Monitoring (to detect the shortcut). This banner surfaces both
// explicitly with their own status so re-granting isn't the confusing
// "I already did this, why is it still asking?" single-button flow it used to be.
const AccessibilityPermissions: React.FC = () => {
  const { t } = useTranslation();
  const [hasAccessibility, setHasAccessibility] = useState(false);
  const [hasInputMonitoring, setHasInputMonitoring] = useState(false);
  const [isRequesting, setIsRequesting] = useState(false);

  // Accessibility permissions are only required on macOS
  const isMacOS = type() === "macos";

  const refresh = useCallback(async (): Promise<void> => {
    const [accessibility, inputMonitoring] = await Promise.all([
      checkAccessibilityPermission(),
      checkInputMonitoringPermission(),
    ]);
    setHasAccessibility(accessibility);
    setHasInputMonitoring(inputMonitoring);
    return;
  }, []);

  // Check on mount, and re-check whenever the window regains focus (i.e. the
  // user returns from System Settings) so the statuses update on their own.
  useEffect(() => {
    if (!isMacOS) return;

    refresh();

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
      if (!hasAccessibility) {
        await requestAccessibilityPermission();
      } else if (!hasInputMonitoring) {
        await invoke("request_input_monitoring_access");
      }
    } catch (error) {
      console.error("Error requesting permissions:", error);
    } finally {
      // Statuses refresh on window focus once the user returns from Settings.
      setIsRequesting(false);
    }
  };

  // Skip rendering on non-macOS platforms or once both permissions are granted.
  if (!isMacOS || (hasAccessibility && hasInputMonitoring)) {
    return null;
  }

  const PermissionRow: React.FC<{ label: string; granted: boolean }> = ({
    label,
    granted,
  }) => (
    <div className="flex items-center gap-2 text-sm">
      {granted ? (
        <Check className="w-4 h-4 text-ok shrink-0" />
      ) : (
        <span className="w-4 h-4 shrink-0 flex items-center justify-center">
          <span className="w-1.5 h-1.5 rounded-full bg-mid-gray" />
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
    <div className="p-4 w-full rounded-lg border border-mid-gray">
      <div className="flex justify-between items-center gap-3">
        <div>
          <p className="text-sm font-medium">
            {t("accessibility.permissionsDescription")}
          </p>
          <div className="mt-2 flex flex-col gap-1">
            <PermissionRow
              label={t("onboarding.permissions.accessibility.title")}
              granted={hasAccessibility}
            />
            <PermissionRow
              label={t("onboarding.permissions.inputMonitoring.title")}
              granted={hasInputMonitoring}
            />
          </div>
        </div>
        <button
          onClick={handleOpenSettings}
          disabled={isRequesting}
          className="min-h-10 px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 hover:bg-logo-primary/10 rounded cursor-pointer hover:border-logo-primary disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5"
        >
          {isRequesting && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
          {t("accessibility.openSettings")}
        </button>
      </div>
    </div>
  );
};

export default AccessibilityPermissions;
