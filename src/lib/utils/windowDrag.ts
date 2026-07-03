import type React from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

const INTERACTIVE_SELECTOR =
  "button, a, input, select, textarea, [role=button]";

export const startWindowDrag = (e: React.MouseEvent) => {
  if (
    e.button !== 0 ||
    (e.target as HTMLElement).closest(INTERACTIVE_SELECTOR)
  ) {
    return;
  }

  getCurrentWindow().startDragging().catch((error: unknown) => {
    console.error("Failed to start window drag:", error);
  });
};
