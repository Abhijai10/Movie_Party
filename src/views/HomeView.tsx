import { motion } from "framer-motion";
import { ArrowRight, Settings, Ticket } from "lucide-react";
import type { StoredSchedule } from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";
import { MovieReelHero } from "../components/mp/MovieReelHero";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import logoMark from "../assets/logo_mark.png";

type HomeViewProps = {
  onCreate: () => void;
  onJoin: () => void;
  /** Batch 16 (§53): persisted upcoming schedules. */
  upcoming: StoredSchedule[];
  /** Latest preload progress per schedule id (0..1), from PRELOAD_STATE. */
  preloadProgress: Record<string, number>;
  /** Local media names by media id, for honest card titles. */
  mediaNameById: (mediaId: string) => string;
  /** Batch 15 (§55–§62): open Settings. */
  onOpenSettings: () => void;
  /** Batch 16 (§18): open the Schedule form. */
  onOpenSchedule: () => void;
};

export function HomeView({
  onCreate,
  onJoin,
  upcoming,
  preloadProgress,
  mediaNameById,
  onOpenSettings,
  onOpenSchedule,
}: HomeViewProps) {
  const mediaNameFor = mediaNameById;
  const preloadPercentLabel = (scheduleId: string): string => {
    const percent = preloadProgress[scheduleId];
    return percent != null ? String(Math.round(percent * 100)) : "0";
  };
  return (
    <div className="relative w-screen h-screen overflow-hidden">
      <SilkBackground />

      <header className="relative z-10 flex items-center justify-between px-12 pt-8">
        <div className="flex items-center gap-3">
          <img
            src={logoMark}
            alt="Movie Party logo"
            className="w-9 h-9 rounded-full object-cover"
            style={{ boxShadow: "0 0 24px rgba(159,122,234,0.4)" }}
          />
          <span className="font-serif-display text-xl tracking-tight text-white">Movie Party</span>
        </div>
        <div className="hidden md:flex items-center gap-8 text-[11px] tracking-[0.28em] uppercase text-white/50">
          <span>Cinema</span>
          <span>Sync</span>
          <span>Together</span>
        </div>
        <div className="flex items-center gap-5">
          <button
            type="button"
            onClick={onOpenSchedule}
            className="text-[11px] tracking-[0.22em] uppercase text-white/50 hover:text-white transition"
            data-testid="home-schedule-btn"
            title="Schedule a movie for later"
          >
            Schedule
          </button>
          <button
            type="button"
            onClick={onOpenSettings}
            aria-label="Open settings"
            data-testid="home-settings-btn"
            className="text-white/50 hover:text-white transition"
          >
            <Settings className="w-[18px] h-[18px]" strokeWidth={1.6} />
          </button>
          <StatusIndicator state="sync" label="Strict Sync" />
        </div>
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

          {/* UI_UX_SPEC §53 — Upcoming parties with preload progress.
              Honest states only: a card shows the REAL schedule status
              from storage; "Waiting for partner to come online" when the
              guest is offline (§19). Nothing is faked. */}
          {upcoming.length > 0 ? (
            <section className="mt-10" data-testid="home-upcoming" aria-label="Upcoming parties">
              <p className="text-[11px] tracking-[0.28em] uppercase text-white/45 mb-3">
                Upcoming
              </p>
              <ul className="space-y-3">
                {upcoming.map((schedule) => (
                  <li
                    key={schedule.scheduleId}
                    className="rounded-xl border border-white/10 bg-white/[0.03] px-5 py-4"
                    data-testid={`upcoming-card-${schedule.scheduleId}`}
                  >
                    <div className="flex items-baseline justify-between gap-4">
                      <p className="text-sm text-white/85 truncate">
                        {mediaNameFor(schedule.mediaId)}
                      </p>
                      <p className="text-xs text-white/50 shrink-0">
                        {new Date(schedule.scheduledStartUtcMs).toLocaleString(undefined, {
                          weekday: "short",
                          hour: "numeric",
                          minute: "2-digit",
                        })}
                      </p>
                    </div>
                    <p className="mt-1.5 text-xs text-white/45">
                      {schedule.status === "WaitingForPeer" || schedule.status === "WaitingForGuest"
                        ? "Waiting for your partner to come online"
                        : schedule.status === "Transferring"
                          ? `Preload ${preloadPercentLabel(schedule.scheduleId)}% complete`
                          : schedule.status === "Cancelled"
                            ? "Cancelled"
                            : schedule.status === "PreloadFailed"
                              ? "Preload failed — retry from the schedule"
                              : schedule.status === "Accepted"
                                ? "Accepted — reminders set"
                                : "Ready to preload"}
                    </p>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
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
