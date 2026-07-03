import { spawnSync } from "node:child_process";
import { copyFile, mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { openWisprMarkSvg } from "../src/lib/brand/openWisprMark";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sourceSvgPath = resolve(repoRoot, "src-tauri/icons/openwispr-source.svg");
const logoWorkDir = resolve(repoRoot, ".context/brand/generated-logo");
const trayWorkDir = resolve(repoRoot, ".context/brand/generated-tray");

const run = (args: string[]) => {
  const result = spawnSync("bunx", ["tauri", "icon", ...args], {
    cwd: repoRoot,
    stdio: "inherit",
  });

  if (result.status !== 0) {
    throw new Error(`bunx tauri icon ${args.join(" ")} failed`);
  }
};

const writeTrayPng = async (
  name: string,
  stroke: string,
  state: "idle" | "recording" | "transcribing",
) => {
  const key = name.replace(/[^a-z0-9]+/gi, "-").replace(/^-|-$/g, "");
  const stateDir = resolve(trayWorkDir, key);
  const svgPath = resolve(stateDir, `${key}.svg`);
  const outDir = resolve(stateDir, "out");

  await mkdir(stateDir, { recursive: true });
  await writeFile(svgPath, openWisprMarkSvg({ stroke, state }));
  run(["--png", "64", "-o", outDir, svgPath]);
  await copyFile(resolve(outDir, "64x64.png"), resolve(repoRoot, name));
};

await mkdir(dirname(sourceSvgPath), { recursive: true });
await writeFile(
  sourceSvgPath,
  openWisprMarkSvg({ stroke: "#111111", background: "#ffffff" }),
);

run([sourceSvgPath]);

await rm(logoWorkDir, { recursive: true, force: true });
run(["--png", "1024", "-o", logoWorkDir, sourceSvgPath]);
await copyFile(
  resolve(logoWorkDir, "1024x1024.png"),
  resolve(repoRoot, "src-tauri/icons/logo.png"),
);
await rm(logoWorkDir, { recursive: true, force: true });

await rm(trayWorkDir, { recursive: true, force: true });

await writeTrayPng("src-tauri/resources/tray_idle.png", "#ffffff", "idle");
await writeTrayPng(
  "src-tauri/resources/tray_recording.png",
  "#ffffff",
  "recording",
);
await writeTrayPng(
  "src-tauri/resources/tray_transcribing.png",
  "#ffffff",
  "transcribing",
);
await writeTrayPng("src-tauri/resources/tray_idle_dark.png", "#111111", "idle");
await writeTrayPng(
  "src-tauri/resources/tray_recording_dark.png",
  "#111111",
  "recording",
);
await writeTrayPng(
  "src-tauri/resources/tray_transcribing_dark.png",
  "#111111",
  "transcribing",
);
await writeTrayPng("src-tauri/resources/handy.png", "#111111", "idle");
await writeTrayPng("src-tauri/resources/recording.png", "#111111", "recording");
await writeTrayPng(
  "src-tauri/resources/transcribing.png",
  "#111111",
  "transcribing",
);

await rm(trayWorkDir, { recursive: true, force: true });
