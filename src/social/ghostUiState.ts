/**
 * Ghost Mode local-UI state bookkeeping.
 *
 * Ghost Mode is a pure local visual hide (MASTER_PRD: "the screen should
 * look like normal movie playback"). It never touches device tracks,
 * playback, or the network session — the backend `set_ghost_mode`
 * guarantees camera/microphone stay exactly as they were.
 *
 * What needs manual bookkeeping is the *chat overlay visibility*, which
 * the modes actively close on entry. The captured value comes back on
 * exit so the user's prior UI state is restored:
 *
 * - a manually opened chat returns open;
 * - a transient ~5 s preview stays closed (its timer long elapsed);
 * - the call tile needs no snapshot at all — its session (position,
 *   minimized, hidden) lives in AppShell state and is only visually
 *   suppressed by the `social-hidden` class, so it reappears exactly
 *   where it was;
 * - the unread-chat flag is monotonic during a mode (arrivals may set
 *   it, nothing can clear it while the chat button is hidden), so it
 *   also survives without a snapshot.
 *
 * Privacy Mode reuses the same hide rules for the visual layer only.
 * Devices are its own concern and deliberately stay disabled on exit
 * until the user re-enables them explicitly.
 */

import type { ChatOverlayVisibility } from "../chat/overlayState";

export type GhostUiSnapshot = {
  chatVisibility: ChatOverlayVisibility;
};

/**
 * Capture the chat overlay visibility immediately before the mode hides
 * it. Called once, on the ghost-on / privacy-on transition.
 */
export function captureGhostUiSnapshot(chatVisibility: ChatOverlayVisibility): GhostUiSnapshot {
  return { chatVisibility: { ...chatVisibility } };
}

/**
 * Restore the captured chat visibility on ghost-off / privacy-off.
 * A manually opened chat returns open; a transient preview does not.
 */
export function restoreGhostUiSnapshot(snapshot: GhostUiSnapshot): ChatOverlayVisibility {
  return {
    manualOpen: snapshot.chatVisibility.manualOpen,
    transientOpen: false,
    historyDismissed: false,
  };
}
