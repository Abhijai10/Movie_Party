import { useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import {
  FALLBACK_FEATURES,
  LIVE_REFRESH_MS,
  loadTmdbTrending,
  type TmdbFeature,
} from "../../home/tmdbFeed";

/**
 * The Home hero — a rotating "Now Showing" wall. Feature posters cycle
 * automatically with a stacked-card parallax so the first screen reads as
 * cinema, not a dashboard. Titles are decorative atmosphere only: real
 * party content always comes from the user's own media.
 *
 * The wall upgrades itself to TMDB's trending week when a TMDB token is
 * configured (Settings → General) — with a local cache and the bundled
 * fallback wall when TMDB is unavailable. TMDB attribution renders under
 * the wall whenever its data/images are shown.
 */

const ROTATE_MS = 4200;
const MAX_FEATURES = FALLBACK_FEATURES.length;

function featureAt(features: TmdbFeature[], offset: number): TmdbFeature {
  const item = features[offset % features.length];
  if (item != null) {
    return item;
  }
  // Unreachable while the list is non-empty — fail loud if that ever changes.
  throw new Error("MovieCarouselHero requires at least one feature");
}

export function MovieCarouselHero() {
  // The shipped wall shows instantly; a live/cached TMDB wall replaces it
  // as soon as it resolves (never blocking on the network).
  const [features, setFeatures] = useState<TmdbFeature[]>(FALLBACK_FEATURES);
  const [usingTmdb, setUsingTmdb] = useState(false);
  const [index, setIndex] = useState(0);
  // Reduced motion: the wall holds a single still feature — no rotation,
  // no slide transitions. Auto-advance is decorative, so it pauses first.
  const prefersReducedMotion = useReducedMotion() ?? false;

  useEffect(() => {
    const stop = loadTmdbTrending((next) => {
      setFeatures(next);
      setUsingTmdb(true);
      setIndex((current) => Math.min(current, next.length - 1));
    });
    return stop;
  }, []);

  // Periodic refresh: while Home stays open, re-check the feed so new
  // trending titles appear automatically (cache TTL still throttles the
  // actual network calls to one per hour at most).
  useEffect(() => {
    const timer = window.setInterval(() => {
      const stop = loadTmdbTrending((next) => {
        setFeatures(next);
        setUsingTmdb(true);
      });
      stop();
    }, LIVE_REFRESH_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, []);

  const wall = useMemo(
    () => (features.length > 0 ? features.slice(0, MAX_FEATURES) : FALLBACK_FEATURES),
    [features],
  );

  useEffect(() => {
    if (prefersReducedMotion) {
      return;
    }
    const timer = window.setInterval(() => {
      setIndex((current) => (current + 1) % wall.length);
    }, ROTATE_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [prefersReducedMotion, wall.length]);

  const feature = featureAt(wall, index);
  const next = featureAt(wall, index + 1);
  const after = featureAt(wall, index + 2);
  const hasTmdbArt = feature.posterUrl != null || feature.backdropUrl != null;

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
          {wall.map((item, dotIndex) => (
            <span
              key={`${item.title}-${String(dotIndex)}`}
              className="w-1.5 h-1.5 rounded-full transition-colors"
              style={{ background: dotIndex === index ? item.accent : "rgba(255,255,255,0.18)" }}
            />
          ))}
        </span>
        <span className="text-[11px] tracking-[0.34em] uppercase text-white/55">Now Showing</span>
      </div>

      <div className="relative w-[300px] h-[420px]" style={{ perspective: 1200 }}>
        <div
          className="absolute inset-0 rounded-2xl border border-white/10 overflow-hidden"
          style={{
            transform: "rotate(7deg) scale(0.9) translateY(26px)",
            opacity: 0.3,
            background: `linear-gradient(150deg, ${after.from}, ${after.to})`,
          }}
          aria-hidden="true"
        >
          {after.posterUrl ? (
            <img
              src={after.posterUrl}
              alt=""
              className="w-full h-full object-cover opacity-60"
              loading="lazy"
              onError={(event) => {
                event.currentTarget.style.display = "none";
              }}
            />
          ) : null}
        </div>
        <div
          className="absolute inset-0 rounded-2xl border border-white/10 overflow-hidden"
          style={{
            transform: "rotate(-5deg) scale(0.95) translateY(12px)",
            opacity: 0.5,
            background: `linear-gradient(150deg, ${next.from}, ${next.to})`,
          }}
          aria-hidden="true"
        >
          {next.posterUrl ? (
            <img
              src={next.posterUrl}
              alt=""
              className="w-full h-full object-cover opacity-70"
              loading="lazy"
              onError={(event) => {
                event.currentTarget.style.display = "none";
              }}
            />
          ) : null}
        </div>

        <AnimatePresence mode="wait">
          <motion.article
            key={`${feature.title}-${String(index)}`}
            initial={{ opacity: 0, x: 48, rotate: 2 }}
            animate={{ opacity: 1, x: 0, rotate: 0 }}
            exit={{ opacity: 0, x: -48, rotate: -2 }}
            transition={{ duration: 0.55, ease: [0.22, 1, 0.36, 1] }}
            className="absolute inset-0 rounded-2xl border border-white/15 overflow-hidden flex flex-col"
            style={{ background: `linear-gradient(160deg, ${feature.from} 0%, ${feature.to} 100%)` }}
            data-testid={`hero-feature-${feature.title}`}
          >
            {feature.posterUrl ? (
              <img
                src={feature.posterUrl}
                alt=""
                className="absolute inset-0 w-full h-full object-cover"
                loading="eager"
                onError={(event) => {
                  // Poster failed → the gradient wall stays, fully readable.
                  event.currentTarget.style.display = "none";
                }}
              />
            ) : null}
            {feature.posterUrl ? (
              <div
                className="absolute inset-0"
                style={{
                  background:
                    "linear-gradient(180deg, rgba(5,5,11,0.05) 0%, rgba(5,5,11,0.25) 55%, rgba(5,5,11,0.88) 100%)",
                }}
                aria-hidden="true"
              />
            ) : null}

            <div className="h-3 flex justify-between px-2 items-center shrink-0" aria-hidden="true">
              {Array.from({ length: 10 }).map((_, i) => (
                <span key={i} className="w-2.5 h-1.5 rounded-[2px] bg-black/40" />
              ))}
            </div>

            <div className="flex-1 flex flex-col items-center justify-end pb-10 px-6 text-center relative">
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
                {feature.year !== "—" ? `Now Trending · ${feature.year}` : "Now Trending"}
              </p>
              <h2
                className="mt-3 font-serif-display text-white text-[34px] leading-[1.05] tracking-tight"
                style={{ textShadow: "0 2px 18px rgba(0,0,0,0.55)" }}
              >
                {feature.title}
              </h2>
              {feature.tagline ? (
                <p
                  className="mt-3 text-sm italic text-white/70 line-clamp-2 max-w-[240px]"
                  style={{ textShadow: "0 1px 8px rgba(0,0,0,0.5)" }}
                >
                  {feature.tagline}
                </p>
              ) : null}
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
          {[...wall, ...wall].map((item, chipIndex) => (
            <div
              key={`${item.title}-${String(chipIndex)}`}
              className="w-28 h-16 rounded-lg border border-white/10 flex flex-col items-center justify-center shrink-0 mr-3 overflow-hidden relative"
              style={{ background: `linear-gradient(140deg, ${item.from} 0%, ${item.to} 100%)` }}
            >
              {item.posterUrl ? (
                <img
                  src={item.posterUrl}
                  alt=""
                  className="absolute inset-0 w-full h-full object-cover opacity-55"
                  loading="lazy"
                  onError={(event) => {
                    event.currentTarget.style.display = "none";
                  }}
                />
              ) : null}
              <span
                className="relative font-serif-display text-[11px] text-white/90 truncate max-w-[100px] px-1"
                style={{ textShadow: "0 1px 6px rgba(0,0,0,0.5)" }}
              >
                {item.title}
              </span>
              <span
                className="relative text-[8px] tracking-[0.22em] uppercase"
                style={{ color: item.accent }}
              >
                {item.year}
              </span>
            </div>
          ))}
        </div>
      </div>

      {/* TMDB attribution — required whenever the wall shows its data or
          images. Hidden on the pure bundled fallback. */}
      {usingTmdb && hasTmdbArt ? (
        <p
          className="mt-4 text-[9px] tracking-[0.14em] uppercase text-white/30 select-none"
          data-testid="hero-tmdb-attribution"
        >
          Now-trending titles &amp; art from&nbsp;
          <a
            href="https://www.themoviedb.org/"
            target="_blank"
            rel="noreferrer noopener"
            className="underline decoration-white/20 hover:decoration-white/50"
          >
            The Movie Database (TMDB)
          </a>
          &nbsp;· distributed under CC BY-NC 4.0
        </p>
      ) : null}
    </div>
  );
}

export default MovieCarouselHero;
