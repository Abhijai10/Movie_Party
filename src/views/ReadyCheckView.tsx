import { motion } from "framer-motion";
import { ArrowLeft, ArrowRight, Film } from "lucide-react";
import { useEffect, useState } from "react";
import type { AppSnapshot } from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";

type ReadyCheckViewProps = {
  snapshot: AppSnapshot;
  onStart: () => void;
};

export function ReadyCheckView({ snapshot, onStart }: ReadyCheckViewProps) {
  const [countdown, setCountdown] = useState<number | null>(null);

  const everyoneReady =
    snapshot.participants.every((participant) => participant.mediaReady) &&
    (snapshot.media != null || snapshot.provider.url != null) &&
    snapshot.network.connected &&
    snapshot.room.strictSync;

  useEffect(() => {
    if (countdown === null) return;
    if (countdown <= 0) {
      onStart();
      return;
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
          className="font-serif-display text-white text-[92px] xl:text-[112px] leading-[0.94] tracking-tight mt-6 max-w-4xl"
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
          <span className="font-serif-display text-2xl italic">
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
            <CinemaButton onClick={enterCinema} icon={ArrowRight} data-testid="enter-cinema-btn">
              Enter Cinema
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
