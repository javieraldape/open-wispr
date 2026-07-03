import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const projectRoot = process.cwd();
const themeCss = readFileSync(
  resolve(projectRoot, "src/styles/theme.css"),
  "utf8",
);
const overlayCss = readFileSync(
  resolve(projectRoot, "src/overlay/RecordingOverlay.css"),
  "utf8",
).replace(/^@import\s+["'][^"']+["'];\s*$/m, "");
const overlaySource = readFileSync(
  resolve(projectRoot, "src/overlay/RecordingOverlay.tsx"),
  "utf8",
);

const bars = Array.from({ length: 12 }, (_, i) => {
  return `<i style="--i: ${i}"></i>`;
}).join("");

test.describe("compact recording overlay", () => {
  test("does not render a separate recording status dot", async () => {
    expect(overlaySource).not.toContain('className="sdot"');
  });

  test("keeps streaming recordings compact after text arrives", async () => {
    expect(overlaySource).toContain('if (state === "streaming")');
    expect(overlaySource).not.toContain("streamText.committed");
    expect(overlaySource).not.toContain("streamText.tentative");
    expect(overlaySource).not.toContain('className={`scard open');
  });

  test("uses the Quiet Native Ribbon layout", async ({ page }) => {
    await page.setContent(`
      <style>
        ${themeCss}
        ${overlayCss}
        body {
          margin: 0;
          width: 420px;
          height: 220px;
          display: grid;
          place-items: center;
          background: #d9d9d6;
        }
        .spec-stack {
          display: grid;
          gap: 16px;
        }
      </style>
      <div class="spec-stack">
        <div class="scard compact recording">
          <div class="srow" role="status" aria-label="Recording">
            <div class="swave-mark active" style="--swave-low: 4px; --swave-high: 23px; --swave-duration: 900ms" aria-hidden="true">
              ${bars}
            </div>
          </div>
        </div>
        <div class="scard compact cworking">
          <div class="srow" role="status" aria-label="Transcribing">
            <span class="sspin-mono"></span>
          </div>
        </div>
        <div class="scard compact clinger">
          <div class="srow">
            <div class="slinger">
              <button type="button" class="seditbtn">Edit</button>
              <span class="skeycap" aria-hidden="true">Opt Cmd F</span>
            </div>
          </div>
        </div>
      </div>
    `);

    const recording = page.locator(".scard.compact.recording");
    await expect(recording).toHaveCSS("width", "148px");
    await expect(recording).toHaveCSS("height", "34px");
    await expect(recording.locator(".sdot, .a-dot")).toHaveCount(0);

    const recordingBox = await recording.boundingBox();
    const waveformBox = await recording.locator(".swave-mark").boundingBox();
    expect(recordingBox).not.toBeNull();
    expect(waveformBox).not.toBeNull();

    const recordingCenter = recordingBox!.x + recordingBox!.width / 2;
    const waveformCenter = waveformBox!.x + waveformBox!.width / 2;
    expect(Math.abs(recordingCenter - waveformCenter)).toBeLessThanOrEqual(1);

    const working = page.locator(".scard.compact.cworking");
    await expect(working).toHaveCSS("width", "148px");
    await expect(working).toHaveCSS("height", "34px");

    const linger = page.locator(".scard.compact.clinger");
    await expect(linger).toHaveCSS("width", "148px");
    await expect(linger).toHaveCSS("height", "34px");

    const lingerBox = await linger.boundingBox();
    const controlsBox = await linger.locator(".slinger").boundingBox();
    expect(lingerBox).not.toBeNull();
    expect(controlsBox).not.toBeNull();
    expect(controlsBox!.width).toBeLessThanOrEqual(lingerBox!.width - 12);
  });
});
