import { motion } from "framer-motion";
import { ArrowLeft, ArrowRight, Film } from "lucide-react";
import { useEffect, useState } from "react";
import type { AppSnapshot } from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import { useReducedMotion } from "../hooks/useReducedMotion";

type ReadyCheckViewProps = {
  snapshot: AppSnapshot;
  onStart: () => void;
};

export function ReadyCheckView({ snapshot, onStart }: ReadyCheckViewProps) {
  const [countdown, setCountdown] = useState<number | null>(null);
  const transitionActive = countdown !== null;
  const prefersReducedMotion = useReducedMotion();

  const everyoneReady =
    snapshot.participants.every((participant) => participant.mediaReady) &&
    (snapshot.media != null || snapshot.provider.url != null) &&
    snapshot.network.connected &&
    snapshot.room.strictSync;

  useEffect(() => {
    if (countdown === null) return;
    if (countdown === 0) {
      const t = setTimeout(() => {
        onStart();
      }, 620);
      return () => {
        clearTimeout(t);
      };
    }
    const t = setTimeout(() => {
      setCountdown((c) => (c ?? 0) - 1);
    }, 900);
    return () => {
      clearTimeout(t);
    };
  }, [countdown, onStart]);

  const enterCinema = () => {
    setCountdown(3);
  };

  const host = snapshot.participants.find((p) => p.role === "HOST");
  const guest = snapshot.participants.find((p) => p.role === "GUEST");

  return (
    <div className="relative w-screen h-screen overflow-hidden">
      <SilkBackground variant="dim" />

      <header className="relative z-10 flex items-center justify-between px-12 pt-8">
        <button
          type="button"
          onClick={() => {
            setCountdown(null);
          }}
          className="flex items-center gap-2 text-white/60 hover:text-white transition text-sm tracking-wider"
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

      {transitionActive && (
        <motion.div
          className="ready-cinema-transition"
          initial={{ opacity: 0 }}
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
