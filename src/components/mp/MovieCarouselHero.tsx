import { useEffect, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";

/**
 * The Home hero — a rotating "Now Showing" wall. Feature posters cycle
 * automatically with a stacked-card parallax so the first screen reads as
 * cinema, not a dashboard. Titles are decorative atmosphere only: real
 * party content always comes from the user's own media.
 */

type Feature = {
  title: string;
  genre: string;
  year: string;
  tagline: string;
  from: string;
  to: string;
  accent: string;
};

const FEATURES: Feature[] = [
  {
    title: "Inception",
    genre: "Sci-Fi Thriller",
    year: "2010",
    tagline: "A dream within a dream.",
    from: "#1E1B4B",
    to: "#4C1D95",
    accent: "#A78BFA",
  },
  {
    title: "Interstellar",
    genre: "Epic Sci-Fi",
    year: "2014",
    tagline: "Go further than anyone before.",
    from: "#082F49",
    to: "#0E7490",
    accent: "#67E8F9",
  },
  {
    title: "La La Land",
    genre: "Musical Romance",
    year: "2016",
    tagline: "Here's to the fools who dream.",
    from: "#7C2D12",
    to: "#B45309",
    accent: "#FDBA74",
  },
  {
    title: "Spirited Away",
    genre: "Animated Fantasy",
    year: "2001",
    tagline: "The tunnel leads somewhere new.",
    from: "#064E3B",
    to: "#0F766E",
    accent: "#6EE7B7",
  },
  {
    title: "Dune",
    genre: "Sci-Fi Odyssey",
    year: "2021",
    tagline: "Beyond fear, destiny awaits.",
    from: "#78350F",
    to: "#A16207",
    accent: "#FCD34D",
  },
  {
    title: "The Grand Budapest",
    genre: "Comedy Drama",
    year: "2014",
    tagline: "A perfect holiday, mostly.",
    from: "#831843",
    to: "#BE185D",
    accent: "#F9A8D4",
  },
];

const ROTATE_MS = 4200;

// FEATURES is a non-empty literal; the modulo lookups can still be typed
// as undefined, so funnel them through one definite accessor.
function featureAt(offset: number): Feature {
  const item = FEATURES[offset % FEATURES.length];
  if (item != null) {
    return item;
  }
  // Unreachable while FEATURES is non-empty — fail loud if that ever changes.
  throw new Error("MovieCarouselHero requires at least one feature");
}

export function MovieCarouselHero() {
  const [index, setIndex] = useState(0);
  // Reduced motion: the wall holds a single still feature — no rotation,
  // no slide transitions. Auto-advance is decorative, so it pauses first.
  const prefersReducedMotion = useReducedMotion() ?? false;

  useEffect(() => {
    if (prefersReducedMotion) {
      return;
    }
    const timer = window.setInterval(() => {
      setIndex((current) => (current + 1) % FEATURES.length);
    }, ROTATE_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [prefersReducedMotion]);

  const feature = featureAt(index);
  const next = featureAt(index + 1);
  const after = featureAt(index + 2);

  return (
    <div
      className="relative w-full h-full flex flex-col items-center justify-center"
      data-testid="movie-carousel-hero"
    >
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          background:
            "radial-gradient(circle at 60% 40%, rgba(159,122,234,0.30) 0%, transparent 55%), radial-gradient(circle at 30% 70%, rgba(107,70,193,0.24) 0%, transparent 60%)",
          filter: "blur(24px)",
        }}
      />

      <div className="relative flex items-center gap-3 mb-6">
        <span className="flex gap-1.5" aria-hidden="true">
          {FEATURES.map((item, dotIndex) => (
            <span
              key={item.title}
              className="w-1.5 h-1.5 rounded-full transition-colors"
              style={{ background: dotIndex === index ? item.accent : "rgba(255,255,255,0.18)" }}
            />
          ))}
        </span>
        <span className="text-[11px] tracking-[0.34em] uppercase text-white/55">Now Showing</span>
      </div>

      <div className="relative w-[300px] h-[420px]" style={{ perspective: 1200 }}>
        <div
          className="absolute inset-0 rounded-2xl border border-white/10"
          style={{
            transform: "rotate(7deg) scale(0.9) translateY(26px)",
            opacity: 0.3,
            background: `linear-gradient(150deg, ${after.from}, ${after.to})`,
          }}
          aria-hidden="true"
        />
        <div
          className="absolute inset-0 rounded-2xl border border-white/10"
          style={{
            transform: "rotate(-5deg) scale(0.95) translateY(12px)",
            opacity: 0.5,
            background: `linear-gradient(150deg, ${next.from}, ${next.to})`,
          }}
          aria-hidden="true"
        />

        <AnimatePresence mode="wait">
          <motion.article
            key={feature.title}
            initial={{ opacity: 0, x: 48, rotate: 2 }}
            animate={{ opacity: 1, x: 0, rotate: 0 }}
            exit={{ opacity: 0, x: -48, rotate: -2 }}
            transition={{ duration: 0.55, ease: [0.22, 1, 0.36, 1] }}
            className="absolute inset-0 rounded-2xl border border-white/15 overflow-hidden flex flex-col"
            style={{ background: `linear-gradient(160deg, ${feature.from} 0%, ${feature.to} 100%)` }}
            data-testid={`hero-feature-${feature.title}`}
          >
            <div className="h-3 flex justify-between px-2 items-center shrink-0" aria-hidden="true">
              {Array.from({ length: 10 }).map((_, i) => (
                <span key={i} className="w-2.5 h-1.5 rounded-[2px] bg-black/40" />
              ))}
            </div>

            <div className="flex-1 flex flex-col items-center justify-center px-6 text-center relative">
              <span
                aria-hidden="true"
                className="absolute -bottom-8 -right-3 font-serif-display text-[190px] leading-none text-white/[0.06] select-none"
              >
                {feature.title.charAt(0)}
              </span>
              <p
                className="text-[10px] tracking-[0.3em] uppercase"
                style={{ color: feature.accent }}
              >
                {feature.genre} · {feature.year}
              </p>
              <h2 className="mt-3 font-serif-display text-white text-[34px] leading-[1.05] tracking-tight">
                {feature.title}
              </h2>
              <p className="mt-3 text-sm italic text-white/60">{feature.tagline}</p>
            </div>

            <div className="h-3 flex justify-between px-2 items-center shrink-0" aria-hidden="true">
              {Array.from({ length: 10 }).map((_, i) => (
                <span key={i} className="w-2.5 h-1.5 rounded-[2px] bg-black/40" />
              ))}
            </div>
          </motion.article>
        </AnimatePresence>
      </div>

      <p className="mt-6 text-sm italic text-white/40 font-light" aria-hidden="true">
        a double feature, every night
      </p>

      {/* Constantly-drifting banner strip — the wall crossfades once per
          rotation while this strip keeps moving between rotations, so the
          hero is always in motion. Decorative atmosphere: the title list is
          duplicated for the seamless -50% translate loop, and CSS pauses
          the drift entirely under prefers-reduced-motion. */}
      <div className="mp-marquee-mask mt-6 w-full" aria-hidden="true">
        <div className="mp-marquee">
          {[...FEATURES, ...FEATURES].map((item, chipIndex) => (
            <div
              key={`${item.title}-${String(chipIndex)}`}
              className="w-28 h-16 rounded-lg border border-white/10 flex flex-col items-center justify-center shrink-0 mr-3"
              style={{ background: `linear-gradient(140deg, ${item.from} 0%, ${item.to} 100%)` }}
            >
              <span className="font-serif-display text-[11px] text-white/90">{item.title}</span>
              <span
                className="text-[8px] tracking-[0.22em] uppercase"
                style={{ color: item.accent }}
              >
                {item.year}
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export default MovieCarouselHero;
