import { motion } from "framer-motion";
import { ArrowLeft, ArrowRight, Film } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { AppSnapshot } from "../backend/appRuntime";
import { countdownDisplayFrom, countdownHandoff } from "../sync/countdownModel";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import { useReducedMotion } from "../hooks/useReducedMotion";
import { CallTile } from "../overlays/CallTile";
import type { CallTileSessionState } from "../overlays/callTileState";

type ReadyCheckViewProps = {
  snapshot: AppSnapshot;
  /** §25: host presses Start — schedules the backend-owned countdown. */
  onRequestCountdown: () => void;
  /** §25: called when the countdown completes so the shell enters cinema. */
  onStarted: () => void;
  /** Leave Ready Check and return to the lobby (cleanup is backend-owned). */
  onBack: () => void;
  callTileSession: CallTileSessionState;
  onCallTileSessionChange: (next: CallTileSessionState) => void;
};

export function ReadyCheckView({
  snapshot,
  onRequestCountdown,
  onStarted,
  onBack,
  callTileSession,
  onCallTileSessionChange,
}: ReadyCheckViewProps) {
  // §25: the countdown comes from the backend's scheduled operation —
  // never a frontend-invented timer chain. A rAF ticker re-derives the
  // display from the backend-provided deadline each frame.
  const [nowWallMs, setNowWallMs] = useState(() => Date.now());
  const executeAtWallMs = snapshot.sync.pendingOperation?.executeAtWallMs ?? null;
  const display = countdownDisplayFrom(executeAtWallMs, nowWallMs);
  const countdown =
    display.phase === "running"
      ? display.label
      : display.phase === "done"
        ? 0
        : null;
  const transitionActive = countdown !== null;
  const prefersReducedMotion = useReducedMotion();

  const everyoneReady =
    snapshot.participants.every((participant) => participant.mediaReady) &&
    (snapshot.media != null || snapshot.provider.url != null) &&
    snapshot.network.connected &&
    snapshot.room.strictSync;

  // F8: fire the hand-off into the cinema exactly once per countdown. The
  // effect below restarts whenever its deps change — and `onStarted` is
  // re-created by the parent on every render — so a guard local to the effect
  // resets on each run and re-fired on an already-elapsed deadline. Keying the
  // guard on the deadline (held in a ref, so it survives effect re-runs) keeps
  // duplicate ready events, re-renders and reconnects from starting the
  // transition twice, while still allowing a genuinely new countdown.
  const startedForRef = useRef<number | null>(null);

  useEffect(() => {
    if (executeAtWallMs == null) {
      startedForRef.current = null;
      return;
    }
    let frame = 0;
    const tick = () => {
      const next = countdownDisplayFrom(executeAtWallMs, Date.now());
      setNowWallMs(Date.now());
      if (next.phase === "done") {
        const { shouldStart, nextStartedFor } = countdownHandoff(
          executeAtWallMs,
          startedForRef.current,
        );
        startedForRef.current = nextStartedFor;
        if (shouldStart) {
          // The backend commit at the deadline is authoritative; this is
          // the visual handoff into cinema once it has fired.
          onStarted();
        }
        return;
      }
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => {
      window.cancelAnimationFrame(frame);
    };
  }, [executeAtWallMs, onStarted]);

  const enterCinema = () => {
    // §25: Start schedules the backend countdown (host authority, §15).
    onRequestCountdown();
  };

  const host = snapshot.participants.find((p) => p.role === "HOST");
  const guest = snapshot.participants.find((p) => p.role === "GUEST");
  const peer = snapshot.participants.find((p) => p.role !== snapshot.room.role);
  const peerName = peer?.displayName ?? "Movie partner";

  return (
    <div
      className={`relative w-screen h-screen overflow-hidden ${
        snapshot.ghostMode || snapshot.privacyMode ? "social-hidden" : ""
      }`}
    >
      <SilkBackground variant="dim" />

      <header className="relative z-10 flex items-center justify-between px-12 pt-8">
        <button
          type="button"
          disabled={transitionActive}
          onClick={() => {
            // §25: once the countdown is scheduled the commit is in
            // flight; Back is disabled until it completes. Otherwise Back
            // genuinely returns to the lobby — the backend clears the
            // readiness votes so neither side is stuck on this screen.
            onBack();
          }}
          className="flex items-center gap-2 text-white/60 hover:text-white transition text-sm tracking-wider disabled:opacity-30 disabled:hover:text-white/60"
          data-testid="ready-back-btn"
        >
          <ArrowLeft className="w-4 h-4" strokeWidth={1.6} /> Back to lobby
        </button>
        <span className="text-[11px] tracking-[0.28em] uppercase text-white/50">Final check</span>
      </header>

      <main className="relative z-10 max-w-[1300px] mx-auto px-12 h-[calc(100vh-100px)] flex flex-col items-center justify-center text-center">
        <motion.span
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6 }}
          className="text-[11px] tracking-[0.4em] uppercase text-white/50"
        >
          ● The room is dimmed
        </motion.span>

        <motion.h1
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.9, delay: 0.1, ease: [0.22, 1, 0.36, 1] }}
          className="font-serif-display text-white text-[60px] xl:text-[76px] leading-[0.96] tracking-tight mt-6 max-w-3xl"
        >
          Getting ready
          <br />
          <span className="italic">for cinema</span>
        </motion.h1>

        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.6, delay: 0.5 }}
          className="mt-10 flex items-center gap-4 text-white/70"
        >
          <Film className="w-4 h-4" strokeWidth={1.5} />
          <span className="font-serif-display text-xl italic max-w-2xl truncate">
            {snapshot.media?.filename ?? snapshot.provider.url ?? "A Private Cinema"}
          </span>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 15 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.8, delay: 0.7 }}
          className="mt-12 flex items-center gap-10"
        >
          <ReadyPill name={host?.displayName} role="Host" ready={host?.mediaReady} />
          <div
            className="w-24 h-px"
            style={{
              background: "linear-gradient(90deg, transparent, rgba(159,122,234,0.5), transparent)",
            }}
          />
          <ReadyPill name={guest?.displayName ?? "Guest"} role="Guest" ready={guest?.mediaReady} />
        </motion.div>

        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.8, delay: 1 }}
          className="mt-10"
        >
          <StatusIndicator
            state={everyoneReady ? "ready" : "waiting"}
            label={everyoneReady ? "Everyone is ready" : "Preparing cinema"}
          />
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.8, delay: 1.1 }}
          className="mt-10"
        >
          {countdown === null ? (
            <CinemaButton
              onClick={enterCinema}
              icon={ArrowRight}
              disabled={!everyoneReady}
              data-testid="enter-cinema-btn"
            >
              {everyoneReady ? "Enter Cinema" : "Waiting for readiness"}
            </CinemaButton>
          ) : (
            <div
              className="font-serif-display text-white text-6xl tracking-tight"
              data-testid="cinema-countdown"
            >
              {countdown > 0 ? countdown : "•"}
            </div>
          )}
        </motion.div>
      </main>

      {/* The call tile stays reachable above the Ready Check content —
          it must never disappear behind this "Dim the Lights" screen,
          and (reservedBottomPx) never cover the Enter Cinema button. */}
      <CallTile
        peerName={peerName}
        remoteCameraEnabled={peer?.cameraEnabled ?? false}
        remoteMicrophoneEnabled={peer?.microphoneEnabled ?? false}
        remoteConnected={peer?.connected ?? false}
        remoteStream={null}
        localStream={null}
        session={callTileSession}
        onSessionChange={onCallTileSessionChange}
        reservedBottomPx={150}
      />

      {transitionActive && (
        <motion.div
          className="ready-cinema-transition"          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: prefersReducedMotion ? 0.01 : 0.24 }}
          aria-live="assertive"
          data-testid="ready-cinema-transition"
        >
          <div className="ready-curtain ready-curtain-left" aria-hidden="true" />
          <div className="ready-curtain ready-curtain-right" aria-hidden="true" />
          <div className="ready-spotlight" aria-hidden="true" />
          <motion.div
            key={countdown}
            className="ready-countdown"
            initial={
              prefersReducedMotion
                ? { opacity: 0 }
                : { opacity: 0, scale: 0.86, filter: "blur(10px)" }
            }
            animate={
              prefersReducedMotion
                ? { opacity: 1 }
                : { opacity: 1, scale: 1, filter: "blur(0px)" }
            }
            exit={
              prefersReducedMotion
                ? { opacity: 0 }
                : { opacity: 0, scale: 1.08, filter: "blur(12px)" }
            }
            transition={{
              duration: prefersReducedMotion ? 0.01 : 0.34,
              ease: [0.22, 1, 0.36, 1],
            }}
          >
            {countdown > 0 ? countdown : "START"}
          </motion.div>
        </motion.div>
      )}
    </div>
  );
}

function ReadyPill({ name, role, ready }: { name?: string; role: string; ready?: boolean }) {
  const initials = name
    ?.split(" ")
    .map((s) => s[0])
    .join("")
    .slice(0, 2)
    .toUpperCase();
  return (
    <div className="flex flex-col items-center gap-3">
      <div
        className="relative w-20 h-20 rounded-full flex items-center justify-center font-serif-display text-2xl text-white"
        style={{
          background: "linear-gradient(135deg, #4A2B85, #201142)",
          border: "1px solid rgba(159,122,234,0.4)",
          boxShadow: "0 10px 40px -10px rgba(107,70,193,0.6), 0 0 0 6px rgba(52,211,153,0.08)",
        }}
      >
        {initials || "?"}
        {ready && (
          <span className="absolute -bottom-1 -right-1 w-4 h-4 rounded-full bg-[#34D399] border-2 border-[#05050B] status-dot" />
        )}
      </div>
      <div className="text-center">
        <p className="text-white text-sm">{name}</p>
        <p className="text-[10px] tracking-[0.28em] uppercase text-white/45 mt-1">{role}</p>
      </div>
    </div>
  );
}
