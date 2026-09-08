import { useEffect, useState } from "react";

/**
 * UI_UX_SPEC §40 — DISCONNECT overlay:
 *
 *   "<peer> disconnected.
 *    The movie has been paused.
 *    Reconnecting..."
 *
 * Buttons appear after a short grace:
 *
 *   [ Keep Waiting ]
 *   [ Continue Without <peer> ]   (host only)
 *
 * The grace delay exists so a brief network blip resolves without the user
 * facing a decision. "Continue Without <peer>" is the spec's explicit
 * host-only override of the strict-sync guest gate (§14) — the coordinator
 * marks the guest abandoned and playback resumes solo.
 */

const GRACE_MS = 4_000;

type ReconnectOverlayProps = {
  peerName: string;
  isReconnecting: boolean;
  /** Host sees Continue Without Guest; the guest can only keep waiting. */
  isHost: boolean;
  /** §40 Keep Waiting: dismiss the decision, keep waiting for reconnect. */
  onKeepWaiting: () => void;
  /** §40 host-only: continue solo via continue_without_guest. */
  onContinueWithoutGuest: () => void;
};

export function ReconnectOverlay({
  peerName,
  isReconnecting,
  isHost,
  onKeepWaiting,
  onContinueWithoutGuest,
}: ReconnectOverlayProps) {
  const [graceElapsed, setGraceElapsed] = useState(false);

  useEffect(() => {
    if (!isReconnecting) {
      setGraceElapsed(false);
      return undefined;
    }
    const timer = window.setTimeout(() => {
      setGraceElapsed(true);
    }, GRACE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [isReconnecting]);

  if (!isReconnecting) {
    return null;
  }

  return (
    <section
      className="reconnect-overlay"
      role="alertdialog"
      aria-live="assertive"
      aria-label={`${peerName} disconnected`}
      data-testid="reconnect-overlay"
    >
      <h2>{peerName} disconnected.</h2>
      <p>The movie has been paused. Reconnecting...</p>
      {graceElapsed ? (
        <div className="flex items-center gap-3 mt-6">
          <button
            type="button"
            onClick={onKeepWaiting}
            className="px-5 py-2.5 rounded-full border border-white/20 text-sm text-white/85 hover:bg-white/10 transition"
            data-testid="reconnect-keep-waiting"
          >
            Keep Waiting
          </button>
          {isHost ? (
            <button
              type="button"
              onClick={onContinueWithoutGuest}
              className="px-5 py-2.5 rounded-full bg-[#6B46C1] hover:bg-[#553592] text-sm text-white transition"
              data-testid="reconnect-continue-without-guest"
            >
              Continue Without {peerName}
            </button>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

export default ReconnectOverlay;
