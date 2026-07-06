import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const projectRoot = process.cwd();
const appSource = readFileSync(resolve(projectRoot, "src/App.tsx"), "utf8");
const accessibilityOnboardingSource = readFileSync(
  resolve(projectRoot, "src/components/onboarding/AccessibilityOnboarding.tsx"),
  "utf8",
);
const accessibilityPermissionsSource = readFileSync(
  resolve(projectRoot, "src/components/AccessibilityPermissions.tsx"),
  "utf8",
);
const keyboardReadinessSource = readFileSync(
  resolve(projectRoot, "src/lib/utils/macosKeyboardReadiness.ts"),
  "utf8",
);

test.describe("macOS permission flow", () => {
  test("exposes raw permission and operational keyboard readiness", () => {
    expect(keyboardReadinessSource).toContain("hasAccessibilityPermission");
    expect(keyboardReadinessSource).toContain("hasInputMonitoringPermission");
    expect(keyboardReadinessSource).toContain("hasRawKeyboardPermissions");
    expect(keyboardReadinessSource).toContain("isKeyboardOperational");
    expect(keyboardReadinessSource).toContain("usedOperationalFallback");
    expect(keyboardReadinessSource).toContain(
      "const hasRawKeyboardPermissions = hasAccessibilityPermission",
    );
    expect(keyboardReadinessSource).toContain("commands.initializeEnigo()");
    expect(keyboardReadinessSource).toContain("commands.initializeShortcuts()");
  });

  test("uses keyboard operational fallback for returning users", () => {
    expect(appSource).toContain("getMacOSKeyboardReadiness()");
    expect(appSource).toContain(
      "keyboardReadiness.hasRawKeyboardPermissions ||",
    );
    expect(appSource).toContain("keyboardReadiness.isKeyboardOperational");
    expect(appSource).toContain("if (!hasMicrophone || !hasKeyboardAccess)");
    expect(appSource).toContain("await revealMainWindowForPermissions();");
    expect(appSource).toContain('setOnboardingStep("accessibility");');
  });

  test("keeps first-run onboarding strict on required macOS permissions", () => {
    expect(accessibilityOnboardingSource).toContain(
      "getMacOSKeyboardReadiness({ allowOperationalFallback: false })",
    );
    expect(accessibilityOnboardingSource).toContain(
      "keyboardReadiness.hasRawKeyboardPermissions &&",
    );
    expect(accessibilityOnboardingSource).not.toMatch(
      /isKeyboardOperational\s*&&\s*microphoneGranted/,
    );
    expect(accessibilityOnboardingSource).not.toContain(
      "request_input_monitoring_access",
    );
    expect(accessibilityOnboardingSource).not.toContain(
      "onboarding.permissions.inputMonitoring.title",
    );
  });

  test("settings banner only requests required keyboard permission", () => {
    expect(accessibilityPermissionsSource).toContain(
      "getMacOSKeyboardReadiness()",
    );
    expect(accessibilityPermissionsSource).toContain(
      "keyboardReadiness.hasRawKeyboardPermissions",
    );
    expect(accessibilityPermissionsSource).toContain(
      "keyboardReadiness.isKeyboardOperational",
    );
    expect(accessibilityPermissionsSource).not.toContain(
      "request_input_monitoring_access",
    );
    expect(accessibilityPermissionsSource).not.toContain(
      "onboarding.permissions.inputMonitoring.title",
    );
  });
});
