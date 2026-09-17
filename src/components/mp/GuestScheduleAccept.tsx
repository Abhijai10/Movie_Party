import { useState } from "react";
import { Check, X } from "lucide-react";
import { guestAcceptSchedule } from "../../backend/appRuntime";

/**
 * Whether the guest's answer may dismiss the prompt (F38).
 *
 * `guestAcceptSchedule` resolves to `null` when the call did not reach the
 * backend. Dismissing on null would mark the schedule answered in the UI while
 * nothing was recorded, losing the guest's choice with no way back — so the
 * prompt stays up and offers a retry.
 */
export function answerRecorded(result: unknown): boolean {
  return result != null;
}

/**
 * §56 GUEST SCHEDULE ACCEPT — when the host schedules a movie, the guest
 * explicitly accepts or declines. Acceptance is the guest's own decision
 * (recorded locally + sent to the host); declining is always allowed.
 * The banner is an overlay — it never blocks the room (§27).
 */
export function GuestScheduleAccept({
  scheduleId,
  scheduledStartUtcMs,
  onAnswered,
}: {
  scheduleId: string;
  scheduledStartUtcMs: number;
  onAnswered: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [answering, setAnswering] = useState(false);

  const answer = (accepted: boolean) => {
    setError(null);
    setAnswering(true);
    void guestAcceptSchedule(scheduleId, accepted)
      .then((next) => {
        if (!answerRecorded(next)) {
          setError("Could not record your answer. Try again.");
          return;
        }
        onAnswered();
      })
      .finally(() => {
        setAnswering(false);
      });
  };

  const when = new Date(scheduledStartUtcMs);
  const whenLabel = Number.isNaN(when.getTime())
    ? "the planned time"
    : when.toLocaleString(undefined, {
        weekday: "short",
        month: "short",
        day: "numeric",
        hour: "numeric",
        minute: "2-digit",
      });

  return (
    <div
      className="fixed bottom-6 left-1/2 -translate-x-1/2 z-[65] w-full max-w-[min(560px,92vw)]"
      role="alertdialog"
      aria-label="Accept the scheduled movie?"
      // F40: this container and the Accept button below both used
      // `guest-schedule-accept`, so a test id could not identify either one
      // unambiguously. The prompt and its action are now distinct ids.
      data-testid="guest-schedule-prompt"
    >
      <div
        className="rounded-2xl px-5 py-4"
        style={{
          background: "rgba(13, 11, 20, 0.92)",
          border: "1px solid rgba(159,122,234,0.25)",
          backdropFilter: "blur(14px)",
        }}
      >
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <span className="text-[10px] tracking-[0.26em] uppercase text-white/45">
              Your friend scheduled a movie
            </span>
            <p className="mt-1 text-sm text-white/85 truncate">
              Movie night · <span className="text-white/60">{whenLabel}</span>
            </p>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            <button
              type="button"
              disabled={answering}
              onClick={() => {
                answer(false);
              }}
              className="flex items-center gap-1.5 px-3.5 py-2 rounded-full border border-white/10 bg-white/[0.03] hover:bg-white/[0.07] transition text-xs tracking-wider uppercase text-white/70 disabled:opacity-50"
              data-testid="guest-schedule-decline"
            >
              <X className="w-3.5 h-3.5" />
              Decline
            </button>
            <button
              type="button"
              autoFocus
              disabled={answering}
              onClick={() => {
                answer(true);
              }}
              className="flex items-center gap-1.5 px-4 py-2 rounded-full bg-[#9F7AEA]/25 hover:bg-[#9F7AEA]/40 border border-[#9F7AEA]/40 transition text-xs tracking-wider uppercase text-white disabled:opacity-50"
              data-testid="guest-schedule-accept"
            >
              <Check className="w-3.5 h-3.5" />
              Accept
            </button>
          </div>
        </div>
        {/* F38: the prompt stays up and says so when the answer was not
            recorded, instead of dismissing and losing the choice. */}
        {error ? (
          <p
            className="mt-2 text-xs text-[#F87171]"
            role="status"
            data-testid="guest-schedule-error"
          >
            {error}
          </p>
        ) : null}
      </div>
    </div>
  );
}
