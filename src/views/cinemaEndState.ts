/**
 * AUD-16 — the end-of-media state, as decisions the Cinema UI can be tested on.
 *
 * ## The problem this exists to fix
 *
 * AUD-03 made the *data* truthful at end of media: the host's player reports
 * `COMPLETED` and the room reports `ENDED` (see `apply_end_of_media` in
 * `app_runtime.rs`). Nothing surfaced it. The user was left looking at the
 * cinema screen with:
 *
 *  * no "the film finished" message at all;
 *  * a sync indicator reading **"Syncing"**, because the indicator only knew
 *    `PLAYING` and "everything else" — so an ended room claimed to be actively
 *    synchronizing;
 *  * a play control drawn in its **Play** state, which is the *resume*
 *    affordance. Pressing it called `host_play` from `Ended`, which re-entered
 *    the play protocol and committed a play from the end position.
 *
 * ## Why these are pure functions
 *
 * The repo has no DOM test environment (`vitest` runs in `node`, there is no
 * `@testing-library/react` or `jsdom`), so a component cannot be rendered in a
 * test. Every existing UI decision of this kind is therefore extracted into a
 * pure function and tested directly — `reconnectLatchAfter`,
 * `cinemaDockInteractionClass`, `closedChatOverlay`/`openChatManually`. This
 * module follows that pattern deliberately rather than adding a UI testing
 * framework for one batch.
 *
 * The functions take `roomState` as a plain `string` because that is what the
 * wire actually carries (`AppSnapshot.sync.roomState` is a string copy of the
 * Rust `RoomState`; see `sync_room_snapshot`). A narrower union type would
 * describe a guarantee the transport does not make.
 */

/** Room state the backend reports when the movie has finished (AUD-03). */
export const ROOM_STATE_ENDED = "ENDED";

/** Room state in which playback is genuinely running. */
export const ROOM_STATE_PLAYING = "PLAYING";

export const MOVIE_FINISHED_TITLE = "Movie Finished";
export const MOVIE_FINISHED_BODY = "The movie has ended.";
export const MOVIE_FINISHED_ACTION = "Back to Lobby";

/** Whether the movie has finished and the room has ended. */
export function isMovieEnded(roomState: string): boolean {
  return roomState === ROOM_STATE_ENDED;
}

/**
 * What the dock's play/pause control should do in this room state.
 *
 * `"NONE"` is the point of this function. At `ENDED` there is no correct
 * playback action: the film is over, and `PLAY` would call `host_play` on an
 * ended room — the defect AUD-16 was raised for. Returning `"NONE"` (rather
 * than, say, treating `ENDED` as "not playing, therefore resume") makes the
 * absence of an action explicit instead of an accident of the comparison.
 *
 * This is deliberately *not* a replay affordance. Replay is a product decision
 * with its own semantics (where does the playhead go, who owns it, does the
 * guest follow), and inventing one here would be inventing product.
 */
export type PlaybackToggleAction = "PLAY" | "PAUSE" | "NONE";

export function playbackToggleAction(roomState: string): PlaybackToggleAction {
  if (isMovieEnded(roomState)) {
    return "NONE";
  }
  return roomState === ROOM_STATE_PLAYING ? "PAUSE" : "PLAY";
}

/**
 * Whether the dock's play/pause/seek controls may act at all.
 *
 * Drives the controls' `disabled` state so an ended room does not *offer* an
 * action it will refuse to perform. The button is disabled rather than hidden
 * on purpose: a control that silently disappears reads as a bug, whereas a
 * disabled one reads as "not available right now" — which is the truth.
 */
export function playbackControlsEnabled(roomState: string): boolean {
  return !isMovieEnded(roomState);
}

/**
 * What the sync indicator should say.
 *
 * `"NONE"` exists because the old two-branch indicator labelled every
 * non-`PLAYING` state "Syncing". An ended room is not synchronizing anything,
 * so the honest answer is to show nothing rather than to invent a third label.
 */
export type SyncIndicatorState = "IN_SYNC" | "SYNCING" | "NONE";

export function syncIndicatorFor(roomState: string): SyncIndicatorState {
  if (isMovieEnded(roomState)) {
    return "NONE";
  }
  return roomState === ROOM_STATE_PLAYING ? "IN_SYNC" : "SYNCING";
}

/**
 * Whether the buffering overlay may render.
 *
 * ADV-05 already stopped the *backend* from setting `strict_sync_paused` when
 * the film ends, which is what made the app announce "Paused to keep you
 * together — <peer> is buffering" at the end of every movie. That fixed the
 * cause it could reach; this fixes the surface. `bufferingParticipant` is a
 * *separate* input that can still be set when the movie ends (a guest whose
 * buffer ran low as the credits rolled), so relying on the flag alone would
 * leave the same overlay reachable by a different route.
 *
 * The overlay is suppressed outright at `ENDED`: whatever the peer's buffer is
 * doing, the film is over and "we paused to keep you together" is false.
 */
export function shouldShowBufferingOverlay(input: {
  roomState: string;
  strictSyncPaused: boolean;
  bufferingParticipant: string | null;
}): boolean {
  if (isMovieEnded(input.roomState)) {
    return false;
  }
  return input.strictSyncPaused || input.bufferingParticipant != null;
}
