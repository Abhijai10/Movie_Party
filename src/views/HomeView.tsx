import { motion } from "framer-motion";
import { ArrowRight, Ticket } from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { MovieReelHero } from "../components/mp/MovieReelHero";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";

type HomeViewProps = {
  onCreate: () => void;
  onJoin: () => void;
};

export function HomeView({ onCreate, onJoin }: HomeViewProps) {
  return (
    <div className="relative w-screen h-screen overflow-hidden">
      <SilkBackground />

      <header className="relative z-10 flex items-center justify-between px-12 pt-8">
        <div className="flex items-center gap-3">
          <div
            className="relative w-9 h-9 rounded-full flex items-center justify-center"
            style={{
              background: "linear-gradient(135deg, #6B46C1, #2E1B5A)",
              boxShadow: "0 0 24px rgba(159,122,234,0.4)",
            }}
          >
            <div className="w-3 h-3 rounded-full bg-white/95" />
          </div>
          <span className="font-serif-display text-xl tracking-tight text-white">Move Party</span>
        </div>
        <div className="hidden md:flex items-center gap-8 text-[11px] tracking-[0.28em] uppercase text-white/50">
          <span>Cinema</span>
          <span>Sync</span>
          <span>Together</span>
        </div>
        <StatusIndicator state="sync" label="Strict Sync" />
      </header>

      <main className="relative z-10 max-w-[1300px] mx-auto px-12 h-[calc(100vh-80px)] grid grid-cols-12 gap-8 items-center">
        <motion.section
          initial={{ opacity: 0, y: 24 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.9, ease: [0.22, 1, 0.36, 1] }}
          className="col-span-12 lg:col-span-6 flex flex-col justify-center"
        >
          <span className="text-[11px] tracking-[0.32em] uppercase text-white/50 mb-6">
            ● Welcome back
          </span>

          <h1 className="font-serif-display text-white text-[68px] xl:text-[84px] leading-[0.94] tracking-[-0.02em]">
            What are we
            <br />
            <span className="italic text-white/90">watching</span>{" "}
            <span className="inline-block relative">
              tonight?
              <motion.span
                initial={{ scaleX: 0 }}
                animate={{ scaleX: 1 }}
                transition={{ delay: 0.7, duration: 1 }}
                className="absolute -bottom-1 left-0 right-0 h-[2px] origin-left"
                style={{ background: "linear-gradient(90deg, #9F7AEA, transparent)" }}
              />
            </span>
          </h1>

          <p className="mt-8 text-white/60 text-base leading-relaxed max-w-md">
            A private movie night where playback stays perfectly in sync. Two seats, one cinema —
            dim the room and press play together.
          </p>

          <div className="mt-10 flex items-center gap-4">
            <CinemaButton onClick={onCreate} icon={ArrowRight} data-testid="home-create-party-btn">
              Create Party
            </CinemaButton>
            <CinemaButton
              variant="ghost"
              onClick={onJoin}
              icon={Ticket}
              iconPos="left"
              data-testid="home-join-party-btn"
            >
              Join Party
            </CinemaButton>
          </div>

          <div className="mt-12 flex items-center gap-6 text-[11px] tracking-[0.24em] uppercase text-white/40">
            <StatusIndicator state="ready" label="Strict sync on" />
            <span className="w-px h-3 bg-white/15" />
            <span>End-to-end private</span>
            <span className="w-px h-3 bg-white/15" />
            <span>2 seats only</span>
          </div>
        </motion.section>

        <motion.section
          initial={{ opacity: 0, scale: 0.95 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={{ duration: 1.2, ease: "easeOut", delay: 0.2 }}
          className="col-span-12 lg:col-span-6 h-[600px] relative"
        >
          <MovieReelHero />
        </motion.section>
      </main>
    </div>
  );
}
