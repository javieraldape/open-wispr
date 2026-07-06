import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./RecordingOverlay.css";
import { commands, events } from "@/bindings";
import type { StreamPhase, StreamPhaseEvent, StreamWorkKind } from "@/bindings";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";

type OverlayState =
  | "recording"
  | "streaming"
  | "transcribing"
  | "processing"
  // v4 "linger" state: shown briefly after a successful paste so the user can
  // one-tap into the Fix editor.
  | "linger";

// Number of reactive bars in the waveform (the simple, smoothed style shared by
// every overlay form). Mic levels arrive as 16 FFT buckets; we take the first N.
const WAVE_BARS = 9;
const MARK_LEVEL_INDEXES = [0, 1, 2, 3, 4, 5, 6, 7, 8, 7, 6, 5] as const;
// Waveform sweep geometry (px). The active bars pulse between LOW (trough, just
// above the idle dots) and a crest that scales with voice energy from HIGH_MIN
// (barely-active) up to HIGH_MAX (loud). GAMMA (<1) makes the response feel
// snappier at conversational volumes.
const COMPACT_WAVE_LOW = 4;
const COMPACT_WAVE_HIGH_MIN = 6;
const COMPACT_WAVE_HIGH_MAX = 23;
const COMPACT_WAVE_GAMMA = 0.7;
const COMPACT_WAVE_ACTIVE_THRESHOLD = 0.05;

// Adaptive gate: keep silence idle by activating only when energy rises well
// above the ambient floor (learned live), so it works across mics/rooms.
const NOISE_GATE_RATIO = 2.2; // activate when energy exceeds ambient x this
const NOISE_GATE_MARGIN = 0.012;

// Absolute loudness gradient: raw voice energy is mapped across this input range
// to bar height, so quiet speech = short bars and loud speech = tall bars. These
// two numbers are the sensitivity knobs — widen or shift the range if the meter
// reads over- or under-sensitive on a given mic.
const ENERGY_IN_MIN = 0.15; // rawEnergy at/below this -> shortest active bars
const ENERGY_IN_MAX = 0.6; // rawEnergy at/above this -> full-height bars
// Meter attack/release. A slow attack means a lone noise spike can't reach the
// active threshold in one frame — only sustained energy (voice) builds up — so
// silence stays idle even right after speech.
const ENERGY_ATTACK = 0.22;
const ENERGY_RELEASE = 0.35;

// How long the "linger" Edit pill stays up before the overlay would normally
// hide, in milliseconds. Mirrors the design spec's "linger 2s" decision.
const LINGER_MS = 2000;
const FADE_MS = 200;

const clamp01 = (value: number) => Math.max(0, Math.min(1, value));

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [levels, setLevels] = useState<number[]>(Array(WAVE_BARS).fill(0));
  const [compactEnergy, setCompactEnergy] = useState(0);
  const [phase, setPhase] = useState<StreamPhase>("listening");
  const [workKind, setWorkKind] = useState<StreamWorkKind>("transcribing");
  // Overlay placement (top vs bottom of the screen).
  const [position, setPosition] = useState<"top" | "bottom">("bottom");
  // Bumped each time we (re-)enter the "linger" state, so its auto-hide effect
  // restarts even on a re-entrant paste-complete event.
  const [lingerToken, setLingerToken] = useState(0);

  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));
  const compactEnergyRef = useRef(0);
  // Adaptive ambient floor used by the gate (see constants above).
  const noiseFloorRef = useRef(0.02);
  const lingerWindowHideTimerRef = useRef<number | null>(null);
  const direction = getLanguageDirection(i18n.language);

  const clearLingerWindowHideTimer = () => {
    if (lingerWindowHideTimerRef.current !== null) {
      window.clearTimeout(lingerWindowHideTimerRef.current);
      lingerWindowHideTimerRef.current = null;
    }
  };

  const scheduleLingerWindowHide = () => {
    clearLingerWindowHideTimer();
    lingerWindowHideTimerRef.current = window.setTimeout(() => {
      lingerWindowHideTimerRef.current = null;
      getCurrentWebviewWindow().hide().catch(console.error);
    }, FADE_MS);
  };

  const hideLingerOverlay = () => {
    setIsVisible(false);
    scheduleLingerWindowHide();
  };

  const enterLinger = () => {
    clearLingerWindowHideTimer();
    setState("linger");
    setIsVisible(true);
    // Bump the token so the auto-hide effect below restarts even if we were
    // already in the linger state (re-entrant paste-complete events).
    setLingerToken((tkn) => tkn + 1);
  };

  useEffect(() => {
    const setupEventListeners = async () => {
      const unlistenShow = await listen("show-overlay", async (event) => {
        clearLingerWindowHideTimer();
        await syncLanguageFromSettings();
        // The Live panel flows downward from a top overlay and upward from a
        // bottom one; read the placement so the layout can flip to match.
        try {
          const settings = await commands.getAppSettings();
          if (settings.status === "ok") {
            setPosition(
              settings.data.overlay_position === "top" ? "top" : "bottom",
            );
          }
        } catch {
          // Keep the previous/default placement if settings can't be read.
        }
        const overlayState = event.payload as OverlayState;
        setState(overlayState);
        if (overlayState === "recording" || overlayState === "streaming") {
          compactEnergyRef.current = 0;
          noiseFloorRef.current = 0.02;
          setCompactEnergy(0);
        }
        if (overlayState === "streaming") {
          setPhase("listening");
          setWorkKind("transcribing");
        }
        setIsVisible(true);
      });

      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
      });

      const unlistenPasteComplete = await events.pasteCompleteEvent.listen(
        () => {
          enterLinger();
        },
      );

      const unlistenLevel = await listen<number[]>("mic-level", (event) => {
        const newLevels = event.payload as number[];
        // Exponential smoothing across the 16 buckets, then take the first N
        // bars for the shared waveform.
        const smoothed = smoothedLevelsRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.55 + target * 0.45;
        });
        smoothedLevelsRef.current = smoothed;
        setLevels(smoothed.slice(0, WAVE_BARS));

        const vocalLevels = smoothed.slice(0, WAVE_BARS);
        const peak = Math.max(...vocalLevels);
        const average =
          vocalLevels.reduce((sum, value) => sum + value, 0) /
          Math.max(1, vocalLevels.length);
        // Average-dominant: voice lights up many bands (high average) while
        // silence only spikes isolated bins (low average, high peak), so
        // weighting toward the average rejects noise spikes.
        const rawEnergy = clamp01(peak * 0.2 + average * 0.95);

        // Adaptive ambient floor: drop instantly to any new quiet minimum, creep
        // up only very slowly, so speech never inflates the gate.
        noiseFloorRef.current =
          rawEnergy < noiseFloorRef.current
            ? rawEnergy
            : noiseFloorRef.current * 0.999 + rawEnergy * 0.001;
        const gated =
          rawEnergy >
          noiseFloorRef.current * NOISE_GATE_RATIO + NOISE_GATE_MARGIN;

        // Absolute loudness gradient: map raw energy across the calibrated input
        // range so louder voice = taller bars. Zeroed below the gate so ambient
        // noise never lifts the bars off their idle dots.
        const norm = clamp01(
          (rawEnergy - ENERGY_IN_MIN) / (ENERGY_IN_MAX - ENERGY_IN_MIN),
        );
        const displayEnergy = gated ? norm : 0;

        const energyAlpha =
          displayEnergy > compactEnergyRef.current
            ? ENERGY_ATTACK
            : ENERGY_RELEASE;
        compactEnergyRef.current =
          compactEnergyRef.current * (1 - energyAlpha) +
          displayEnergy * energyAlpha;
        setCompactEnergy(compactEnergyRef.current);
      });

      const unlistenStream = await events.streamTextEvent.listen(() => {});

      const unlistenPhase = await events.streamPhaseEvent.listen((event) => {
        const payload: StreamPhaseEvent = event.payload;
        setPhase(payload.phase);
        if (payload.kind) setWorkKind(payload.kind);
      });

      return () => {
        unlistenShow();
        unlistenHide();
        unlistenPasteComplete();
        unlistenLevel();
        unlistenStream();
        unlistenPhase();
      };
    };

    setupEventListeners();
  }, []);

  // Linger auto-hide: once in the "linger" state, fade back out on its own
  // after LINGER_MS (design spec: "linger 2s"). Cleared/restarted whenever we
  // (re-)enter linger; cancelled if the overlay is hidden or state changes away
  // from linger for any other reason.
  useEffect(() => {
    if (state !== "linger" || !isVisible) return;
    const id = setTimeout(hideLingerOverlay, LINGER_MS);
    return () => clearTimeout(id);
  }, [state, isVisible, lingerToken]);

  useEffect(() => {
    if (state !== "linger") clearLingerWindowHideTimer();
  }, [state]);

  useEffect(() => clearLingerWindowHideTimer, []);

  // Esc dismisses the overlay: cancels the in-flight recording/transcription,
  // or — while lingering — just hides the Edit pill early.
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !isVisible) return;
      if (state === "linger") {
        hideLingerOverlay();
        return;
      }
      commands.cancelOperation();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [state, isVisible]);

  // ---- Shared building blocks (one visual language for every overlay form) ----
  const compactHigh =
    COMPACT_WAVE_HIGH_MIN +
    Math.pow(clamp01(compactEnergy), COMPACT_WAVE_GAMMA) *
      (COMPACT_WAVE_HIGH_MAX - COMPACT_WAVE_HIGH_MIN);
  // Louder voice sweeps a touch faster (1040ms quiet -> 760ms loud).
  const compactDuration = 1040 - clamp01(compactEnergy) * 280;
  const compactActive = compactEnergy > COMPACT_WAVE_ACTIVE_THRESHOLD;
  const compactWaveStyle = {
    "--swave-low": `${COMPACT_WAVE_LOW}px`,
    "--swave-high": `${compactHigh}px`,
    "--swave-duration": `${compactDuration}ms`,
  } as React.CSSProperties;

  const compactRecordingRow = (
    <div className="srow" role="status" aria-label={t("overlay.recording")}>
      <div
        className={`swave-mark ${compactActive ? "active" : ""}`}
        style={compactWaveStyle}
        aria-hidden="true"
      >
        {MARK_LEVEL_INDEXES.map((_, i) => (
          <i key={i} style={{ "--i": i } as React.CSSProperties} />
        ))}
      </div>
    </div>
  );

  const compactWorkingRow = (label: string) => (
    <div className="srow" role="status" aria-label={label}>
      <span className="sspin-mono" />
    </div>
  );

  // ---- Live overlay: keep the same compact ribbon for the entire recording.
  // Streaming text still flows through backend events, but the recording surface
  // never expands into the old transcript/timer panel while the shortcut is held.
  if (state === "streaming") {
    const working = phase === "working";
    return (
      <div
        dir={direction}
        className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
      >
        <div className={`scard compact ${working ? "cworking" : ""}`}>
          {working
            ? compactWorkingRow(
                workKind === "polishing"
                  ? t("overlay.processing")
                  : t("overlay.transcribing"),
              )
            : compactRecordingRow}
        </div>
      </div>
    );
  }

  // ---- Minimal overlay (v4): exactly one row at a time, no timer, no text.
  // The compact pill keeps one fixed size across recording, working, and linger.
  const working = state === "transcribing" || state === "processing";
  const linger = state === "linger";

  const editIcon = (
    <svg viewBox="0 0 16 16" aria-hidden="true">
      <path
        d="M11.4 2.6a1.4 1.4 0 0 1 2 2L6 12l-3 1 1-3z"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinejoin="round"
        strokeLinecap="round"
        fill="none"
      />
    </svg>
  );

  const lingerRow = (
    <div className="srow">
      <div className="slinger">
        <button
          type="button"
          className="seditbtn"
          onClick={() => commands.openFixEditor()}
        >
          {editIcon}
          {t("overlay.edit")}
        </button>
        <span className="skeycap" aria-hidden="true">
          {t("overlay.editShortcut")}
        </span>
      </div>
    </div>
  );

  return (
    <div
      dir={direction}
      className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
    >
      <div
        className={`scard compact ${working && isVisible ? "cworking" : ""} ${
          linger && isVisible ? "clinger" : ""
        }`}
      >
        {linger
          ? lingerRow
          : working
            ? compactWorkingRow(workLabelFor(t, state))
            : compactRecordingRow}
      </div>
    </div>
  );
};

// Accessible label for the working spinner row (screen-reader-only; no visible
// text per the v4 spec). Kept out of the component body so it can be shared by
// both `workingRow` above.
const workLabelFor = (
  t: (key: string) => string,
  state: OverlayState,
): string =>
  state === "processing" ? t("overlay.processing") : t("overlay.transcribing");

export default RecordingOverlay;
