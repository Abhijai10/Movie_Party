import { motion } from "framer-motion";

export function MovieReelHero() {
  return (
    <div
      className="relative w-full h-full flex items-center justify-center"
      data-testid="movie-reel-hero"
    >
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          background:
            "radial-gradient(circle at 60% 45%, rgba(159,122,234,0.35) 0%, transparent 55%), radial-gradient(circle at 30% 70%, rgba(107,70,193,0.28) 0%, transparent 60%)",
          filter: "blur(20px)",
        }}
      />

      <motion.div
        initial={{ opacity: 0, scale: 0.9, x: 40 }}
        animate={{ opacity: 0.35, scale: 1, x: 60 }}
        transition={{ duration: 1.4, ease: "easeOut" }}
        className="absolute top-10 right-4 movie-reel-ghost"
        aria-hidden="true"
      >
        <Reel className="reel-spin-slow" />
      </motion.div>

      <motion.div
        initial={{ opacity: 0, scale: 0.85, rotate: -10 }}
        animate={{ opacity: 1, scale: 1, rotate: 0 }}
        transition={{ duration: 1.2, ease: [0.22, 1, 0.36, 1] }}
        className="relative movie-reel-stage"
      >
        <div className="reel-film-wrap" aria-hidden="true">
          {Array.from({ length: 9 }).map((_, i) => (
            <FilmFrame key={i} idx={i} />
          ))}
        </div>
        <Reel className="reel-spin" />
      </motion.div>

      <motion.div
        initial={{ opacity: 0, x: -60 }}
        animate={{ opacity: 1, x: 0 }}
        transition={{ delay: 0.5, duration: 1 }}
        className="absolute -bottom-8 left-0 right-0 h-24 overflow-hidden"
      >
        <div className="film-scroll flex gap-1.5" style={{ width: "200%" }}>
          {Array.from({ length: 24 }).map((_, i) => (
            <FilmFrame key={i} idx={i} />
          ))}
        </div>
      </motion.div>

      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          background:
            "linear-gradient(135deg, rgba(255,255,255,0.06) 0%, transparent 40%, transparent 60%, rgba(255,255,255,0.02) 100%)",
        }}
      />
    </div>
  );
}

function Reel({ className = "" }: { className?: string }) {
  return (
    <div className={`movie-reel-front ${className}`} aria-hidden="true">
      <div className="movie-reel-disc">
        {Array.from({ length: 6 }).map((_, i) => (
          <span key={i} className="movie-reel-port" />
        ))}
        <span className="movie-reel-hub" />
      </div>
    </div>
  );
}

function FilmFrame({ idx }: { idx: number }) {
  const shades = ["#0F0A1A", "#150C22", "#1B0F2C", "#0B0713"];
  const tint = [
    "rgba(159,122,234,0.15)",
    "rgba(107,70,193,0.20)",
    "rgba(76,29,149,0.14)",
    "rgba(159,122,234,0.10)",
  ];
  return (
    <div
      className="relative shrink-0 rounded-sm"
      style={{ width: 120, height: 76, background: shades[idx % 4] }}
    >
      <div
        className="absolute inset-1 rounded-sm"
        style={{ background: tint[idx % 4], border: "1px solid rgba(159,122,234,0.14)" }}
      />
      <div className="absolute top-0 left-0 right-0 h-2 flex justify-between px-1">
        {Array.from({ length: 5 }).map((_, i) => (
          <div key={i} className="w-2 h-1 rounded-[1px]" style={{ background: "#050309" }} />
        ))}
      </div>
      <div className="absolute bottom-0 left-0 right-0 h-2 flex justify-between px-1">
        {Array.from({ length: 5 }).map((_, i) => (
          <div key={i} className="w-2 h-1 rounded-[1px]" style={{ background: "#050309" }} />
        ))}
      </div>
    </div>
  );
}

export default MovieReelHero;
