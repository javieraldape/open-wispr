import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const projectRoot = process.cwd();
const appSource = readFileSync(resolve(projectRoot, "src/App.tsx"), "utf8");
const accessibilityOnboardingSource = readFileSync(
  resolve(projectRoot, "src/components/onboarding/AccessibilityOnboarding.tsx"),
  "utf8",
);

test.describe("macOS permission flow", () => {
  test("does not initialize keyboard input as a returning-user permission fallback", () => {
    expect(appSource).not.toMatch(
      /\|\|\s*\(?\s*await\s+initializeKeyboardInput\(/,
    );
    expect(appSource).toContain("await revealMainWindowForPermissions();");
    expect(appSource).toContain('setOnboardingStep("accessibility");');
  });

  test("does not initialize onboarding keyboard input before both keyboard permissions are granted", () => {
    expect(accessibilityOnboardingSource).not.toMatch(
      /\|\|\s*\(?\s*await\s+initializeKeyboard\(/,
    );
    expect(accessibilityOnboardingSource).toContain(
      "accessibilityGranted && inputMonitoringGranted",
    );
    expect(accessibilityOnboardingSource).toContain(
      "await initializeKeyboard();",
    );
  });
});
