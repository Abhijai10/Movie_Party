import { useMemo, useState } from "react";
import { motion } from "framer-motion";
import { ArrowLeft, CalendarClock, TriangleAlert } from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import { createAndBroadcastSchedule, type AppSnapshot } from "../backend/appRuntime";
import type { CallMode } from "../call/webrtc";

/**
 * UI_UX_SPEC §18 + §19 — the Schedule screen.
 *
 * Fields: date/time, media (the party's chosen movie), guest, call mode.
 * For local media it shows the estimated transfer + the recommended
 * preload start, allows moving preload EARLIER only (§18: "Allow user to
 * move preload earlier. Do not allow moving it later than estimated safe
 * point without warning") — moving later than the safe point shows the
 * explicit warning and still refuses the unsafe value.
 * §19: if the guest is currently offline the schedule still saves, with
 * the honest offline notice.
 */
type ScheduleViewProps = {
  snapshot: AppSnapshot;
  onBack: () => void;
  onScheduled: () => void;
};

const CALL_MODES: Array<{ id: CallMode; label: string }> = [
  { id: "VIDEO_VOICE", label: "Video + Voice" },
  { id: "VOICE_ONLY", label: "Voice only" },
  { id: "OFF", label: "Off" },
];

/** §18 transfer estimate from measured goodput — bits-correct, 1.4× safety. */
export function transferEstimateFrom(input: {
  remainingBytes: number;
  goodputBps: number;
}): { minutes: number | null; goodputKnown: boolean } | null {
  if (input.remainingBytes === 0) return { minutes: 0, goodputKnown: true };
  if (input.goodputBps <= 0) return { minutes: null, goodputKnown: false };
  const transferSeconds = (input.remainingBytes * 8) / input.goodputBps;
  return { minutes: (transferSeconds * 1.4) / 60, goodputKnown: true };
}

/** §18: the planned preload can only move EARLIER than the recommended
 * safe point — a negative adjustment clamps back to the recommendation
 * (never later, §18). */
export function preloadEarliestOnly(recommendedUtcMs: number, bufferMinutes: number): number {
  if (!Number.isFinite(bufferMinutes) || bufferMinutes <= 0) return recommendedUtcMs;
  return recommendedUtcMs - bufferMinutes * 60_000;
}

/** Local date-time → UTC epoch ms (wall clock is the scheduling domain, §55). */
function localInputToUtcMs(dateValue: string, timeValue: string): number | null {
  if (!dateValue || !timeValue) return null;
  const date = new Date(`${dateValue}T${timeValue}`);
  if (Number.isNaN(date.getTime())) return null;
  return date.getTime();
}

function formatTimeLabel(utcMs: number): string {
  return new Date(utcMs).toLocaleString(undefined, {
    weekday: "short",
    hour: "numeric",
    minute: "2-digit",
  });
}

function formatMinutes(minutes: number): string {
  if (!Number.isFinite(minutes) || minutes <= 0) return "—";
  if (minutes < 60) return `${String(Math.round(minutes))} min`;
  const hours = Math.floor(minutes / 60);
  const rest = Math.round(minutes % 60);
  return rest > 0 ? `${String(hours)} h ${String(rest)} min` : `${String(hours)} h`;
}

export function ScheduleView({ snapshot, onBack, onScheduled }: ScheduleViewProps) {
  const media = snapshot.media;
  const guest = snapshot.participants.find((participant) => participant.role !== snapshot.room.role);
  const roomId = snapshot.room.roomId ?? "";
  const mediaId = media?.mediaId ?? "";

  const [dateValue, setDateValue] = useState("");
  const [timeValue, setTimeValue] = useState("");
  const [callMode, setCallMode] = useState<CallMode>("VIDEO_VOICE");
  const [preloadMinutesEarly, setPreloadMinutesEarly] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const scheduledUtcMs = localInputToUtcMs(dateValue, timeValue);

  /**
   * §18 estimated transfer: remaining bytes at the measured goodput with the
   * same 1.4 safety + 15-min margin the backend's canonical
   * scheduling::calculate_preload_start applies (audit P14 — bits-correct).
   * Computed for display; the backend recomputes authoritatively on create.
   */
  const transferEstimate = useMemo(() => {
    if (!media) return null;
    return transferEstimateFrom({
      remainingBytes: Math.max(
        media.fileSize - (snapshot.transfer?.bytesAvailable ?? 0),
        0,
      ),
      goodputBps: snapshot.network.goodputBps || 0,
    });
  }, [media, snapshot.network.goodputBps, snapshot.transfer]);

  const recommendedPreloadUtcMs = useMemo(() => {
    if (scheduledUtcMs == null) return null;
    const transferMinutes = transferEstimate?.minutes ?? 0;
    return scheduledUtcMs - (transferMinutes + 15) * 60_000;
  }, [scheduledUtcMs, transferEstimate]);

  // §18: preload can move EARLIER only. The slider adds buffer minutes on
  // top of the recommended start; it cannot shave the safe point.
  const plannedPreloadUtcMs =
    recommendedPreloadUtcMs != null
      ? preloadEarliestOnly(recommendedPreloadUtcMs, preloadMinutesEarly)
      : null;

  const guestOnline = guest?.connected ?? false;
  const guestName = guest?.displayName ?? "your partner";

  const scheduleNow = () => {
    if (scheduledUtcMs == null) {
      setError("Pick a date and time for the movie.");
      return;
    }
    if (plannedPreloadUtcMs == null) {
      setError("Preload start could not be calculated — is the movie chosen?");
      return;
    }
    if (!roomId || !mediaId) {
      setError("Create the party and choose the movie first.");
      return;
    }
    setError(null);
    setSubmitting(true);
    void (async () => {
      const scheduleId = await createAndBroadcastSchedule({
        roomId,
        mediaId,
        scheduledStartUtcMs: scheduledUtcMs,
        plannedPreloadUtcMs: plannedPreloadUtcMs,
        guestDeviceId: guest?.id ?? "guest",
        callMode: callMode,
      });
      setSubmitting(false);
      if (scheduleId) {
        // Batch 16: notification permission is requested NOW — the user
        // just expressed intent to be reminded (§11: no premature prompts).
        // A denial never blocks the schedule; reminders just can't toast.
        if (typeof Notification !== "undefined" && Notification.permission === "default") {
          try {
            await Notification.requestPermission();
          } catch {
            /* permission API unavailable — the schedule is still saved */
          }
        }
        onScheduled();
      } else {
        setError("Movie Party could not save the schedule. Try again.");
      }
    })();
  };

  return (
    <div className="relative w-screen h-screen overflow-hidden" data-testid="schedule-screen">
      <SilkBackground variant="calm" />
      <header className="relative z-10 flex items-center justify-between px-12 pt-8">
        <button
          type="button"
          onClick={onBack}
          className="flex items-center gap-2 text-white/60 hover:text-white transition text-sm tracking-wider"
          data-testid="schedule-back-btn"
        >
          <ArrowLeft className="w-4 h-4" strokeWidth={1.6} /> Back
        </button>
        <StatusIndicator state="sync" label="Strict Sync" />
      </header>

      <main className="relative z-10 max-w-xl mx-auto px-12 mt-6 h-[calc(100vh-140px)] overflow-y-auto">
        <motion.section
          initial={{ opacity: 0, y: 14 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
        >
          <h1 className="font-serif-display text-white text-4xl tracking-[-0.02em]">
            Schedule the movie
          </h1>

          <div className="mt-8 space-y-5" data-testid="schedule-form">
            <div className="grid grid-cols-2 gap-4">
              <label className="block">
                <span className="text-xs tracking-[0.18em] uppercase text-white/50">Date</span>
                <input
                  type="date"
                  value={dateValue}
                  onChange={(event) => {
                    setDateValue(event.target.value);
                  }}
                  className="mt-2 w-full bg-white/[0.04] border border-white/15 rounded-lg px-3.5 py-2.5 text-sm text-white/90 focus:outline-none focus:border-sky-400/50"
                />
              </label>
              <label className="block">
                <span className="text-xs tracking-[0.18em] uppercase text-white/50">Time</span>
                <input
                  type="time"
                  value={timeValue}
                  onChange={(event) => {
                    setTimeValue(event.target.value);
                  }}
                  className="mt-2 w-full bg-white/[0.04] border border-white/15 rounded-lg px-3.5 py-2.5 text-sm text-white/90 focus:outline-none focus:border-sky-400/50"
                />
              </label>
            </div>

            <div>
              <span className="text-xs tracking-[0.18em] uppercase text-white/50">Movie</span>
              <p className="mt-2 text-sm text-white/80">
                {media ? media.filename : "No movie chosen yet — pick one in Create Party first"}
              </p>
            </div>

            <div>
              <span className="text-xs tracking-[0.18em] uppercase text-white/50">Guest</span>
              <p className="mt-2 text-sm text-white/80">{guestName}</p>
            </div>

            <fieldset>
              <legend className="text-xs tracking-[0.18em] uppercase text-white/50">
                Call mode
              </legend>
              <div className="mt-2.5 flex flex-wrap gap-2.5">
                {CALL_MODES.map((mode) => (
                  <button
                    key={mode.id}
                    type="button"
                    onClick={() => {
                      setCallMode(mode.id);
                    }}
                    aria-pressed={callMode === mode.id}
                    className={`px-4 py-2 rounded-full text-xs tracking-[0.12em] uppercase border transition ${
                      callMode === mode.id
                        ? "border-sky-400/50 bg-sky-400/10 text-sky-100"
                        : "border-white/15 text-white/55 hover:text-white"
                    }`}
                  >
                    {mode.label}
                  </button>
                ))}
              </div>
            </fieldset>

            {media && transferEstimate ? (
              <div
                className="rounded-xl border border-white/10 bg-white/[0.03] p-4 space-y-2"
                data-testid="schedule-estimate"
              >
                <p className="text-sm text-white/80">
                  {transferEstimate.minutes != null && transferEstimate.goodputKnown ? (
                    <>
                      Estimated transfer:{" "}
                      <span className="text-white">{formatMinutes(transferEstimate.minutes)}</span>
                    </>
                  ) : (
                    "Transfer estimate: will be measured when both devices are online"
                  )}
                </p>
                {recommendedPreloadUtcMs != null ? (
                  <p className="text-sm text-white/60 flex items-center gap-2">
                    <CalendarClock className="w-4 h-4 text-white/40" strokeWidth={1.6} />
                    Recommended preload start: {formatTimeLabel(recommendedPreloadUtcMs)}
                  </p>
                ) : null}
                <label className="block pt-1">
                  <span className="text-xs text-white/50">
                    Move preload earlier (safety buffer): {preloadMinutesEarly} min
                  </span>
                  <input
                    type="range"
                    min={0}
                    max={180}
                    step={15}
                    value={preloadMinutesEarly}
                    onChange={(event) => {
                      setPreloadMinutesEarly(Number(event.target.value));
                    }}
                    className="mt-2 w-full accent-sky-400"
                  />
                </label>
                <p className="text-xs text-white/40">
                  §18: preload can only move earlier than the recommended safe point — never later.
                </p>
              </div>
            ) : null}

            {scheduledUtcMs != null ? (
              <p className="text-sm text-white/60">
                Scheduled movie:{" "}
                <span className="text-white/90">{formatTimeLabel(scheduledUtcMs)}</span>
              </p>
            ) : null}

            {!guestOnline ? (
              <div
                className="rounded-xl border border-amber-400/25 bg-amber-400/[0.06] p-4"
                data-testid="schedule-offline-warning"
                role="status"
              >
                <p className="text-sm text-amber-100/90 flex items-start gap-2">
                  <TriangleAlert
                    className="w-4 h-4 shrink-0 mt-0.5 text-amber-300/90"
                    strokeWidth={1.7}
                  />
                  <span>
                    {guestName} is offline. The schedule will still be saved. Movie Party will
                    begin transfer when both devices are online, and their local reminder will
                    appear if their device is running. (§19)
                  </span>
                </p>
              </div>
            ) : null}

            {error ? (
              <p className="text-sm text-rose-300/90" data-testid="schedule-error" role="alert">
                {error}
              </p>
            ) : null}

            <div className="pt-2 pb-10">
              <CinemaButton
                onClick={scheduleNow}
                disabled={submitting || scheduledUtcMs == null}
                data-testid="schedule-create-btn"
              >
                {submitting ? "Saving…" : "Save Schedule"}
              </CinemaButton>
            </div>
          </div>
        </motion.section>
      </main>
    </div>
  );
}
