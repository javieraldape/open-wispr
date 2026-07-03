import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./RecordingOverlay.css";
import { commands, events } from "@/bindings";
import type {
  StreamPhase,
  StreamPhaseEvent,
  StreamTextEvent,
  StreamWorkKind,
} from "@/bindings";
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
// hide, in milliseconds. Mirrors the design spec's "linger 4s" decision.
const LINGER_MS = 4000;
const FADE_MS = 200;

const clamp01 = (value: number) => Math.max(0, Math.min(1, value));

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [levels, setLevels] = useState<number[]>(Array(WAVE_BARS).fill(0));
  const [compactEnergy, setCompactEnergy] = useState(0);
  const [streamText, setStreamText] = useState<StreamTextEvent>({
    committed: "",
    tentative: "",
  });
  const [phase, setPhase] = useState<StreamPhase>("listening");
  const [workKind, setWorkKind] = useState<StreamWorkKind>("transcribing");
  const [elapsed, setElapsed] = useState(0);
  // Bumped on each new streaming session so the Live card remounts fresh (replays
  // the pop-in, and never animates in from the previous panel's open size).
  const [session, setSession] = useState(0);
  // Overlay placement (top vs bottom of the screen). The Live panel grows downward
  // from a top overlay (oldest line under the pill) and upward from a bottom one.
  const [position, setPosition] = useState<"top" | "bottom">("bottom");
  // True once live text overflows the cap. A top overlay fades its top edge only
  // while overflowing, so the resting first line stays crisp flush under the pill.
  const [overflowing, setOverflowing] = useState(false);
  // Bumped each time we (re-)enter the "linger" state, so its auto-hide effect
  // restarts even on a re-entrant paste-complete event.
  const [lingerToken, setLingerToken] = useState(0);

  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));
  const compactEnergyRef = useRef(0);
  // Adaptive ambient floor used by the gate (see constants above).
  const noiseFloorRef = useRef(0.02);
  // Live-text scroll-back: the text region "sticks" to the newest line while the
  // user is at the bottom; if they scroll up to read history, auto-follow pauses
  // until they scroll back down.
  const capRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);
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
          setStreamText({ committed: "", tentative: "" });
        }
        if (overlayState === "recording") {
          compactEnergyRef.current = 0;
          noiseFloorRef.current = 0.02;
          setCompactEnergy(0);
        }
        if (overlayState === "streaming") {
          setPhase("listening");
          setWorkKind("transcribing");
          setElapsed(0);
          setSession((s) => s + 1); // remount the card fresh for this session
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
          rawEnergy > noiseFloorRef.current * NOISE_GATE_RATIO + NOISE_GATE_MARGIN;

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

      const unlistenStream = await events.streamTextEvent.listen((event) => {
        setStreamText(event.payload);
      });

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
  // after LINGER_MS (design spec: "linger 4s"). Cleared/restarted whenever we
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

  // Elapsed timer while the Live overlay is visible.
  useEffect(() => {
    if (state !== "streaming" || !isVisible) return;
    const id = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(id);
  }, [state, isVisible]);

  // Stick to the bottom as text streams in — but only while pinned, so a user who
  // has scrolled up to read history isn't yanked back down by the next chunk.
  useLayoutEffect(() => {
    const el = capRef.current;
    if (!el) return;
    // Fade the top edge only once text actually overflows the cap.
    setOverflowing(el.scrollHeight > el.clientHeight + 1);
    if (pinnedRef.current) el.scrollTop = el.scrollHeight;
  }, [streamText]);

  // Each fresh streaming session starts pinned to the bottom, fade cleared.
  useEffect(() => {
    pinnedRef.current = true;
    setOverflowing(false);
  }, [session]);

  // Re-pin when the user is within ~a line of the bottom; unpin otherwise.
  const handleStreamScroll = () => {
    const el = capRef.current;
    if (!el) return;
    pinnedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= 16;
  };

  const fmtTime = (s: number) =>
    `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

  // ---- Shared building blocks (one visual language for every overlay form) ----
  const waveform = (
    <div className="swave">
      {levels.map((v, i) => (
        <i
          key={i}
          style={{
            height: `${Math.max(3, Math.min(18, 3 + Math.pow(v, 0.7) * 15))}px`,
          }}
        />
      ))}
    </div>
  );

  const cancelBtn = (
    <button
      className="sx"
      aria-label="cancel"
      onClick={() => commands.cancelOperation()}
    >
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path
          d="M4 4 L12 12 M12 4 L4 12"
          stroke="currentColor"
          strokeWidth="1.6"
          strokeLinecap="round"
        />
      </svg>
    </button>
  );

  // dot (left) | waveform (center) | timer + cancel (right) — same structure for
  // pill & panel, so the Live morph is a pure width change.
  const listeningRow = (showTimer: boolean, showCancel: boolean) => (
    <div className="sbase">
      <div className="sbase-l">
        <span className="sdot" />
      </div>
      {waveform}
      <div className="sbase-r">
        {showTimer && <span className="stimer">{fmtTime(elapsed)}</span>}
        {showCancel && cancelBtn}
      </div>
    </div>
  );

  // spinner (left) | label (center) | cancel (right) — same 3-zone grid as the
  // listening row, so the label is centered.
  const workingRow = (label: string, showCancel: boolean) => (
    <div className="sbase">
      <div className="sbase-l">
        <span className="sspinner" />
      </div>
      <span className="swork-label">{label}</span>
      <div className="sbase-r">{showCancel && cancelBtn}</div>
    </div>
  );

  // ---- Live overlay: a pill that sculpts open into a panel ----
  if (state === "streaming") {
    const hasText =
      streamText.committed.length > 0 || streamText.tentative.length > 0;
    const working = phase === "working";
    // Keep the panel open whenever there's text — even while finalizing — so the
    // transcript stays put under a working spinner instead of collapsing and
    // squishing the text mid-stream. Only fall back to the small working pill
    // when there was no text to preserve.
    const open = hasText;
    const collapsed = working && !hasText;

    return (
      <div dir={direction} className={`ov-stage ${position}`}>
        <div
          key={session}
          className={`scard ${open ? "open" : ""} ${collapsed ? "working" : ""} ${
            isVisible ? "" : "leaving"
          }`}
        >
          <div className="stext">
            <div className="stext-clip">
              <div
                className={`stext-cap ${overflowing ? "overflowing" : ""}`}
                ref={capRef}
                onScroll={handleStreamScroll}
              >
                <p>
                  <span className="committed">
                    {streamText.committed ? streamText.committed + " " : ""}
                  </span>
                  <span className="tentative">{streamText.tentative}</span>
                  {/* Drop the blinking caret once finalizing — it's no longer
                      capturing, and a static spinner conveys the work. */}
                  {!working && <span className="scaret" />}
                </p>
              </div>
            </div>
          </div>
          {working
            ? workingRow(
                workKind === "polishing"
                  ? t("overlay.processing")
                  : t("overlay.transcribing"),
                true,
              )
            : listeningRow(open, true)}
        </div>
      </div>
    );
  }

  // ---- Minimal overlay (v4): exactly one row at a time, no timer, no text.
  // The compact pill keeps one fixed size across recording, working, and linger.
  const working = state === "transcribing" || state === "processing";
  const linger = state === "linger";

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

  const compactWorkingRow = (
    <div className="srow" role="status" aria-label={workLabelFor(t, state)}>
      <span className="sspin-mono" />
    </div>
  );

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
        {linger ? lingerRow : working ? compactWorkingRow : compactRecordingRow}
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
