import { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  requestAccessibilityPermission,
  checkMicrophonePermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { getMacOSKeyboardReadiness } from "@/lib/utils/macosKeyboardReadiness";
import { useSettingsStore } from "@/stores/settingsStore";
import BrandLockup from "./BrandLockup";
import StepProgress from "./StepProgress";
import { Accessibility, Mic, Check, Loader2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";

// Onboarding is 3 steps total (mic -> keyboard -> model, see App.tsx and the v4
// design spec's "Surface 5 — First run"). This screen covers the first two
// (microphone, then the keyboard permissions); Onboarding.tsx renders step 3.
//
// macOS needs Microphone for recording and Accessibility for the current
// keyboard/input backends. Input Monitoring is observable for diagnostics, but
// it is not a required onboarding gate for the default HandyKeys path.
const TOTAL_ONBOARDING_STEPS = 3;

interface AccessibilityOnboardingProps {
  onComplete: () => void;
}

type PermissionStatus = "checking" | "needed" | "waiting" | "granted";
type PermissionPlatform = "macos" | "windows" | "other";

interface PermissionsState {
  accessibility: PermissionStatus;
  inputMonitoring: PermissionStatus;
  microphone: PermissionStatus;
}

// A single permission row: icon, title, why-it's-needed copy, live status +
// action. Kept local so the three macOS permissions stay visually identical.
const PermissionCard: React.FC<{
  icon: LucideIcon;
  title: string;
  description: string;
  status: PermissionStatus;
  grantLabel: string;
  waitingLabel: string;
  grantedLabel: string;
  onGrant: () => void;
}> = ({
  icon: Icon,
  title,
  description,
  status,
  grantLabel,
  waitingLabel,
  grantedLabel,
  onGrant,
}) => (
  <div className="w-full p-4 rounded-lg bg-white/5 border border-mid-gray/20">
    <div className="flex items-center gap-4">
      <div className="p-3 rounded-full bg-text/10 shrink-0">
        <Icon className="w-6 h-6 text-text" />
      </div>
      <div className="flex-1 min-w-0">
        <h3 className="font-medium text-text">{title}</h3>
        <p className="text-sm text-text/60 mb-3">{description}</p>
        {status === "granted" ? (
          <div className="flex items-center gap-2 text-ok text-sm">
            <Check className="w-4 h-4" />
            {grantedLabel}
          </div>
        ) : status === "waiting" ? (
          <div className="flex items-center gap-2 text-text/50 text-sm">
            <Loader2 className="w-4 h-4 animate-spin" />
            {waitingLabel}
          </div>
        ) : (
          <button
            onClick={onGrant}
            className="px-4 py-2 rounded-lg bg-text hover:bg-text/90 text-background text-sm font-medium transition-colors"
          >
            {grantLabel}
          </button>
        )}
      </div>
    </div>
  </div>
);

const AccessibilityOnboarding: React.FC<AccessibilityOnboardingProps> = ({
  onComplete,
}) => {
  const { t } = useTranslation();
  const refreshAudioDevices = useSettingsStore(
    (state) => state.refreshAudioDevices,
  );
  const refreshOutputDevices = useSettingsStore(
    (state) => state.refreshOutputDevices,
  );
  const [permissionPlatform, setPermissionPlatform] =
    useState<PermissionPlatform | null>(null);
  const [permissions, setPermissions] = useState<PermissionsState>({
    accessibility: "checking",
    inputMonitoring: "checking",
    microphone: "checking",
  });
  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const restartHelpTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const errorCountRef = useRef<number>(0);
  const MAX_POLLING_ERRORS = 3;
  const RESTART_HELP_DELAY_MS = 15_000;

  const isMacOS = permissionPlatform === "macos";
  const isWindows = permissionPlatform === "windows";
  const showMicrophonePermission = isMacOS || isWindows;
  const showKeyboardPermissions = isMacOS;

  const allGranted = isMacOS
    ? permissions.accessibility === "granted" &&
      permissions.microphone === "granted"
    : isWindows
      ? permissions.microphone === "granted"
      : true;

  const completeOnboarding = useCallback(async () => {
    await Promise.all([refreshAudioDevices(), refreshOutputDevices()]);
    timeoutRef.current = setTimeout(() => onComplete(), 300);
  }, [onComplete, refreshAudioDevices, refreshOutputDevices]);

  const hasWindowsMicrophoneAccess = useCallback(async (): Promise<boolean> => {
    const microphoneStatus =
      await commands.getWindowsMicrophonePermissionStatus();

    if (!microphoneStatus.supported) {
      return true;
    }

    return microphoneStatus.overall_access !== "denied";
  }, []);

  const clearRestartHelpTimeout = useCallback(() => {
    if (restartHelpTimeoutRef.current) {
      clearTimeout(restartHelpTimeoutRef.current);
      restartHelpTimeoutRef.current = null;
    }
  }, []);

  const [showRestartHelp, setShowRestartHelp] = useState(false);

  const scheduleRestartHelp = useCallback(() => {
    clearRestartHelpTimeout();
    setShowRestartHelp(false);

    restartHelpTimeoutRef.current = setTimeout(() => {
      setShowRestartHelp(true);
    }, RESTART_HELP_DELAY_MS);
  }, [clearRestartHelpTimeout]);

  const restartApp = async () => {
    try {
      await relaunch();
    } catch (error) {
      console.error("Failed to relaunch after permission grant:", error);
      toast.error(t("onboarding.permissions.errors.restartFailed"));
    }
  };

  // Check platform and permission status on mount
  useEffect(() => {
    const currentPlatform = platform();
    const nextPlatform: PermissionPlatform =
      currentPlatform === "macos"
        ? "macos"
        : currentPlatform === "windows"
          ? "windows"
          : "other";

    setPermissionPlatform(nextPlatform);

    // Skip immediately on unsupported platforms
    if (nextPlatform === "other") {
      onComplete();
      return;
    }

    const checkInitial = async () => {
      if (nextPlatform === "macos") {
        try {
          const [keyboardReadiness, microphoneGranted] = await Promise.all([
            getMacOSKeyboardReadiness({ allowOperationalFallback: false }),
            checkMicrophonePermission(),
          ]);
          const accessibilityGranted =
            keyboardReadiness.hasAccessibilityPermission;

          if (keyboardReadiness.hasRawKeyboardPermissions) {
            clearRestartHelpTimeout();
            setShowRestartHelp(false);
          }

          setPermissions({
            accessibility: accessibilityGranted ? "granted" : "needed",
            inputMonitoring: "granted",
            microphone: microphoneGranted ? "granted" : "needed",
          });

          if (
            keyboardReadiness.hasRawKeyboardPermissions &&
            microphoneGranted
          ) {
            await completeOnboarding();
          }
        } catch (error) {
          console.error("Failed to check macOS permissions:", error);
          toast.error(t("onboarding.permissions.errors.checkFailed"));
          setPermissions({
            accessibility: "needed",
            inputMonitoring: "needed",
            microphone: "needed",
          });
        }

        return;
      }

      // Windows: microphone is the only required permission.
      try {
        const microphoneGranted = await hasWindowsMicrophoneAccess();

        setPermissions({
          accessibility: "granted",
          inputMonitoring: "granted",
          microphone: microphoneGranted ? "granted" : "needed",
        });

        if (microphoneGranted) {
          await completeOnboarding();
        }
      } catch (error) {
        console.warn("Failed to check Windows microphone permissions:", error);
        setPermissions({
          accessibility: "granted",
          inputMonitoring: "granted",
          microphone: "granted",
        });
        await completeOnboarding();
      }
    };

    checkInitial();
  }, [completeOnboarding, hasWindowsMicrophoneAccess, onComplete, t]);

  // Polling for permissions after the user clicks a grant button. Each
  // permission is tracked independently so a card only flips to "granted" when
  // its own permission clears.
  const startPolling = useCallback(() => {
    if (pollingRef.current || permissionPlatform === null) return;

    pollingRef.current = setInterval(async () => {
      try {
        if (permissionPlatform === "windows") {
          const microphoneGranted = await hasWindowsMicrophoneAccess();

          if (microphoneGranted) {
            setPermissions((prev) => ({ ...prev, microphone: "granted" }));

            if (pollingRef.current) {
              clearInterval(pollingRef.current);
              pollingRef.current = null;
            }

            await completeOnboarding();
          }

          errorCountRef.current = 0;
          return;
        }

        const [keyboardReadiness, microphoneGranted] = await Promise.all([
          getMacOSKeyboardReadiness({ allowOperationalFallback: false }),
          checkMicrophonePermission(),
        ]);
        const accessibilityGranted =
          keyboardReadiness.hasAccessibilityPermission;
        const keyboardPermissionsGranted =
          keyboardReadiness.hasRawKeyboardPermissions;

        setPermissions((prev) => {
          const next = { ...prev };

          if (accessibilityGranted && prev.accessibility !== "granted") {
            next.accessibility = "granted";
          }
          if (microphoneGranted && prev.microphone !== "granted") {
            next.microphone = "granted";
          }

          // Clear restart help once the required keyboard permission is present.
          if (keyboardPermissionsGranted && prev.accessibility !== "granted") {
            clearRestartHelpTimeout();
            setShowRestartHelp(false);
          }

          return next;
        });

        // If everything is granted, stop polling and proceed.
        if (keyboardPermissionsGranted && microphoneGranted) {
          if (pollingRef.current) {
            clearInterval(pollingRef.current);
            pollingRef.current = null;
          }
          await completeOnboarding();
        }

        // Reset error count on success
        errorCountRef.current = 0;
      } catch (error) {
        console.error("Error checking permissions:", error);
        errorCountRef.current += 1;

        if (errorCountRef.current >= MAX_POLLING_ERRORS) {
          // Stop polling after too many consecutive errors
          if (pollingRef.current) {
            clearInterval(pollingRef.current);
            pollingRef.current = null;
          }
          toast.error(t("onboarding.permissions.errors.checkFailed"));
        }
      }
    }, 1000);
  }, [completeOnboarding, hasWindowsMicrophoneAccess, permissionPlatform, t]);

  // Cleanup polling and timeouts on unmount
  useEffect(() => {
    return () => {
      if (pollingRef.current) {
        clearInterval(pollingRef.current);
      }
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
      clearRestartHelpTimeout();
    };
  }, [clearRestartHelpTimeout]);

  const handleGrantMicrophone = async () => {
    try {
      if (isWindows) {
        await commands.openMicrophonePrivacySettings();
      } else {
        await requestMicrophonePermission();
      }

      setPermissions((prev) => ({ ...prev, microphone: "waiting" }));
      startPolling();
    } catch (error) {
      console.error("Failed to request microphone permission:", error);
      toast.error(t("onboarding.permissions.errors.requestFailed"));
    }
  };

  const handleGrantAccessibility = async () => {
    try {
      await requestAccessibilityPermission();
      setPermissions((prev) => ({ ...prev, accessibility: "waiting" }));
      scheduleRestartHelp();
      startPolling();
    } catch (error) {
      console.error("Failed to request accessibility permission:", error);
      toast.error(t("onboarding.permissions.errors.requestFailed"));
    }
  };

  const isChecking =
    permissionPlatform === null ||
    (isMacOS &&
      permissions.accessibility === "checking" &&
      permissions.microphone === "checking") ||
    (isWindows && permissions.microphone === "checking");

  // Step 1 = microphone (brand lockup + privacy pitch live here); step 2 = the
  // two keyboard permissions; step 3 = model (rendered by Onboarding.tsx once
  // this component calls onComplete). Advance to step 2 once mic is granted.
  const currentStep = permissions.microphone === "granted" ? 2 : 1;

  // Still checking platform/initial permissions
  if (isChecking) {
    return (
      <div className="h-screen w-screen flex items-center justify-center">
        <Loader2 className="w-8 h-8 animate-spin text-text/50" />
      </div>
    );
  }

  // All permissions granted - show success briefly
  if (allGranted) {
    return (
      <div className="h-screen w-screen flex flex-col items-center justify-center gap-4">
        <div className="p-4 rounded-full bg-ok/15">
          <Check className="w-12 h-12 text-ok" />
        </div>
        <p className="text-lg font-medium text-text">
          {t("onboarding.permissions.allGranted")}
        </p>
      </div>
    );
  }

  const grantLabel = t("onboarding.permissions.grant");
  const waitingLabel = t("onboarding.permissions.waiting");
  const grantedLabel = t("onboarding.permissions.granted");

  // Show permissions request screen
  return (
    <div className="h-screen w-screen flex flex-col p-6 gap-6 items-center justify-center overflow-y-auto">
      <div className="max-w-md w-full">
        <StepProgress current={currentStep} total={TOTAL_ONBOARDING_STEPS} />
      </div>

      <div className="max-w-md w-full flex flex-col items-center gap-4">
        {currentStep === 1 ? (
          <>
            {/* Step 1 — microphone: brand lockup + privacy-first pitch. */}
            <div className="text-center mb-2 flex flex-col items-center gap-3">
              <BrandLockup />
              <h2 className="text-xl font-semibold text-text">
                {t("onboarding.brand.headline")}
              </h2>
              <p className="text-text/70">{t("onboarding.brand.body")}</p>
            </div>

            {showMicrophonePermission && (
              <PermissionCard
                icon={Mic}
                title={t("onboarding.permissions.microphone.title")}
                description={t("onboarding.permissions.microphone.description")}
                status={permissions.microphone}
                grantLabel={
                  isWindows
                    ? t("accessibility.openSettings")
                    : t("onboarding.brand.allowMicrophone")
                }
                waitingLabel={waitingLabel}
                grantedLabel={grantedLabel}
                onGrant={handleGrantMicrophone}
              />
            )}
          </>
        ) : (
          <>
            {/* Step 2 — keyboard control. */}
            <div className="text-center mb-2">
              <h2 className="text-xl font-semibold text-text mb-2">
                {t("onboarding.permissions.keyboardTitle")}
              </h2>
              <p className="text-text/70">
                {t("onboarding.permissions.keyboardDescription")}
              </p>
            </div>

            {showKeyboardPermissions && (
              <>
                <PermissionCard
                  icon={Accessibility}
                  title={t("onboarding.permissions.accessibility.title")}
                  description={t(
                    "onboarding.permissions.accessibility.description",
                  )}
                  status={permissions.accessibility}
                  grantLabel={grantLabel}
                  waitingLabel={waitingLabel}
                  grantedLabel={grantedLabel}
                  onGrant={handleGrantAccessibility}
                />
                {showRestartHelp && (
                  <div className="w-full p-4 rounded-lg border border-mid-gray/30 bg-text/[0.03]">
                    <p className="text-sm font-medium text-text">
                      {t("onboarding.permissions.restartHelp.title")}
                    </p>
                    <p className="text-sm text-text/60 mt-1">
                      {t("onboarding.permissions.restartHelp.description")}
                    </p>
                    <button
                      onClick={restartApp}
                      className="mt-3 px-4 py-2 rounded-lg bg-text hover:bg-text/90 text-background text-sm font-medium transition-colors"
                    >
                      {t("onboarding.permissions.restartHelp.action")}
                    </button>
                  </div>
                )}
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
};

export default AccessibilityOnboarding;
