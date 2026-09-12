import { useMemo, useState } from "react";
import { motion } from "framer-motion";
import { ArrowLeft, CalendarClock, TriangleAlert } from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import {
  createAndBroadcastSchedule,
  friendStatusFor,
  type AppSnapshot,
  type StoredFriend,
} from "../backend/appRuntime";
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
 *
 * Guests are picked from saved friends (Add Friend on Home) — the schedule
 * targets the verified friend by default and falls back to the connected
 * participant or a manual id.
 */
type ScheduleViewProps = {
  snapshot: AppSnapshot;
  onBack: () => void;
  onScheduled: () => void;
  /** Saved friends (Add Friend surface) for the guest picker. */
  friends: StoredFriend[];
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

/** "Connection verified 2 h ago" style relative label. */
function summarizeFriendWhen(friend: StoredFriend): string {
  if (friend.lastVerifiedAtMs == null) return "";
  const deltaMs = Date.now() - friend.lastVerifiedAtMs;
  const minutes = Math.round(deltaMs / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${String(minutes)} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${String(hours)} h ago`;
  return `${String(Math.round(hours / 24))} d ago`;
}

export function ScheduleView({ snapshot, onBack, onScheduled, friends }: ScheduleViewProps) {
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
  /** Guest selection: a saved friend's peer key, or "connected" (the live
   * participant), or "manual" (type a device id). */
  const [guestChoice, setGuestChoice] = useState<string>("connected");
  const [manualGuestId, setManualGuestId] = useState("");

  const scheduledUtcMs = localInputToUtcMs(dateValue, timeValue);

  /**
   * §18 estimated transfer: remaining bytes at the measured goodput with the
   * same 1.4 safety + 15-min margin the backend's canonical
   * scheduling::calculate_preload_start applies (bits-correct).
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
  /** The chosen saved friend (when the guest picker targets one). */
  const selectedFriend = friends.find((f) => f.peerKey === guestChoice);
  /** guestDeviceId submitted with the schedule. */
  const guestDeviceId =
    guestChoice === "connected" || guestChoice === "manual"
      ? guest?.id ?? "guest"
      : (selectedFriend?.peerKey ?? guest?.id ?? "guest");

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
    if (guestChoice === "manual" && manualGuestId.trim().length === 0) {
      setError("Enter the guest's device id, or pick a friend.");
      return;
    }
    const resolvedGuestDeviceId =
      guestChoice === "manual" ? manualGuestId.trim() : guestDeviceId;
    setError(null);
    setSubmitting(true);
    void (async () => {
      const scheduleId = await createAndBroadcastSchedule({
        roomId,
        mediaId,
        scheduledStartUtcMs: scheduledUtcMs,
        plannedPreloadUtcMs: plannedPreloadUtcMs,
        guestDeviceId: resolvedGuestDeviceId,
        callMode: callMode,
      });
      setSubmitting(false);
      if (scheduleId) {
        // notification permission is requested NOW — the user
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
    <div className="relative w-screen min-h-screen overflow-y-auto" data-testid="schedule-screen">
      <div className="fixed inset-0 pointer-events-none">
        <SilkBackground variant="calm" />
      </div>
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

      <main className="relative z-10 max-w-2xl w-full mx-auto px-12 mt-8 pb-20">
        <motion.section
          initial={{ opacity: 0, y: 14 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
        >
          <h1 className="font-serif-display text-white text-4xl tracking-[-0.02em]">
            Schedule the movie
          </h1>
          <p className="mt-3 text-sm text-white/50">
            Pick a time — Movie Party preloads the movie so the night starts perfectly in sync.
          </p>

          <div className="mt-10 space-y-6" data-testid="schedule-form">
            <div className="rounded-2xl border border-white/10 bg-white/[0.03] p-5">
              <div className="grid grid-cols-2 gap-5">
                <label className="block">
                  <span className="text-xs tracking-[0.18em] uppercase text-white/50">Date</span>
                  <input
                    type="date"
                    value={dateValue}
                    onChange={(event) => {
                      setDateValue(event.target.value);
                    }}
                    style={{ colorScheme: "dark" }}
                    className="mt-2 w-full bg-[#0D0B14] border border-white/15 rounded-lg px-3.5 py-2.5 text-sm text-white/95 focus:outline-none focus:border-sky-400/50"
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
                    style={{ colorScheme: "dark" }}
                    className="mt-2 w-full bg-[#0D0B14] border border-white/15 rounded-lg px-3.5 py-2.5 text-sm text-white/95 focus:outline-none focus:border-sky-400/50"
                  />
                </label>
              </div>
            </div>

            <div className="rounded-2xl border border-white/10 bg-white/[0.03] p-5 space-y-6">
              <div>
                <span className="text-xs tracking-[0.18em] uppercase text-white/50">Movie</span>
                <p className="mt-2 text-sm text-white/80">
                  {media ? media.filename : "No movie chosen yet — pick one in Create Party first"}
                </p>
              </div>

              <div>
                <span className="text-xs tracking-[0.18em] uppercase text-white/50">Guest</span>
              {friends.length > 0 ? (
                <div className="mt-2.5 flex flex-wrap gap-2.5" data-testid="schedule-guest-picker">
                  {friends.map((friend) => {
                    const status = friendStatusFor(friend, undefined);
                    const chosen = guestChoice === friend.peerKey;
                    return (
                      <button
                        key={friend.peerKey}
                        type="button"
                        onClick={() => {
                          setGuestChoice(friend.peerKey);
                        }}
                        aria-pressed={chosen}
                        title={
                          status === "connected"
                            ? `Connection verified · ${friend.lastPath ?? ""} · ${String(friend.lastLatencyMs ?? "?")} ms`
                            : status === "online"
                              ? "Online — not verified yet"
                              : "Offline"
                        }
                        className={`px-4 py-2 rounded-full text-xs tracking-[0.12em] uppercase border transition ${
                          chosen
                            ? "border-sky-400/50 bg-sky-400/10 text-sky-100"
                            : "border-white/15 text-white/55 hover:text-white"
                        }`}
                        data-testid={`schedule-guest-${friend.displayName}`}
                      >
                        {friend.displayName}
                        {status === "connected" ? " ✓" : ""}
                      </button>
                    );
                  })}
                  {guest ? (
                    <button
                      type="button"
                      onClick={() => {
                        setGuestChoice("connected");
                      }}
                      aria-pressed={guestChoice === "connected"}
                      className={`px-4 py-2 rounded-full text-xs tracking-[0.12em] uppercase border transition ${
                        guestChoice === "connected"
                          ? "border-sky-400/50 bg-sky-400/10 text-sky-100"
                          : "border-white/15 text-white/55 hover:text-white"
                      }`}
                      data-testid="schedule-guest-connected"
                    >
                      {guestName} (connected)
                    </button>
                  ) : null}
                  <button
                    type="button"
                    onClick={() => {
                      setGuestChoice("manual");
                    }}
                    aria-pressed={guestChoice === "manual"}
                    className={`px-4 py-2 rounded-full text-xs tracking-[0.12em] uppercase border transition ${
                      guestChoice === "manual"
                        ? "border-sky-400/50 bg-sky-400/10 text-sky-100"
                        : "border-white/15 text-white/55 hover:text-white"
                    }`}
                    data-testid="schedule-guest-manual"
                  >
                    Someone else
                  </button>
                </div>
              ) : (
                <p className="mt-2 text-sm text-white/80" data-testid="schedule-guest-name">
                  {guestName}
                  {guest ? "" : " — add a friend on Home to schedule directly"}
                </p>
              )}
              {guestChoice === "manual" ? (
                <input
                  value={manualGuestId}
                  onChange={(event) => {
                    setManualGuestId(event.target.value);
                  }}
                  placeholder="Guest device id"
                  style={{ colorScheme: "dark" }}
                  className="mt-2.5 w-full bg-[#0D0B14] border border-white/15 rounded-lg px-3.5 py-2.5 text-sm text-white/95 placeholder:text-white/40 focus:outline-none focus:border-sky-400/50 font-mono-mp"
                  data-testid="schedule-guest-manual-input"
                />
              ) : null}
              {guestChoice !== "manual" && guestChoice !== "connected" && selectedFriend ? (
                <p className="mt-2 text-xs text-white/45">
                  {selectedFriend.lastVerifiedAtMs != null
                    ? `Connection verified ${summarizeFriendWhen(selectedFriend)}`
                    : "Not verified yet — use Connect on the Friends panel to test the tunnel first."}
                </p>
              ) : null}
              </div>
            </div>

            <fieldset className="rounded-2xl border border-white/10 bg-white/[0.03] p-5">
              <legend className="text-xs tracking-[0.18em] uppercase text-white/50">
                Call mode
              </legend>
              <div className="mt-3 flex flex-wrap gap-2.5">
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

            <div className="rounded-2xl border border-white/10 bg-white/[0.03] p-5 flex flex-wrap items-center gap-4">
              <CinemaButton
                onClick={scheduleNow}
                disabled={submitting || scheduledUtcMs == null}
                data-testid="schedule-create-btn"
              >
                {submitting ? "Saving…" : "Save Schedule"}
              </CinemaButton>
              <p className="text-xs text-white/40">
                Your friend gets a reminder — even if they're offline right now.
              </p>
            </div>
          </div>
        </motion.section>
      </main>
    </div>
  );
}
