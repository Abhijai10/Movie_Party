/**
 * AUD-16 — the end-of-media surface.
 *
 * The backend has been truthful about this since AUD-03 (room `ENDED`, player
 * `COMPLETED`), but nothing showed it, so the end of a film looked like a
 * stalled player. This is the smallest honest thing that fixes that: say the
 * movie finished, and offer exactly one action.
 *
 * Deliberately restrained, and deliberately *not* a redesign:
 *
 *  * it does not cover the frame — it is a centred card over a still-visible
 *    final frame, the same visual weight as `.buffer-overlay`;
 *  * it offers **one** action. No replay, no "watch again", no rating, no
 *    next-up. Replay is a product decision with real semantics (playhead
 *    ownership, guest following, position reset) and inventing one here would
 *    be inventing product;
 *  * it reuses the existing cinematic card language (blurred dark panel, purple
 *    accent, serif title) rather than introducing a new one.
 */

import {
  MOVIE_FINISHED_ACTION,
  MOVIE_FINISHED_BODY,
  MOVIE_FINISHED_TITLE,
} from "../views/cinemaEndState";

type MovieFinishedOverlayProps = {
  /** Returns the room to the lobby through the existing `back_to_lobby` path. */
  onBackToLobby: () => void;
};

export function MovieFinishedOverlay({ onBackToLobby }: MovieFinishedOverlayProps) {
  return (
    <section
      className="movie-finished-overlay"
      // `status` rather than `alertdialog`: this is an announcement, not a
      // question the user must answer. It is not modal and nothing is blocked —
      // chat, reactions, the call tile and the dock all stay usable behind it.
      role="status"
      aria-live="polite"
      aria-label={MOVIE_FINISHED_TITLE}
      data-testid="movie-finished-overlay"
    >
      <h1>{MOVIE_FINISHED_TITLE}</h1>
      <p>{MOVIE_FINISHED_BODY}</p>
      <button
        type="button"
        onClick={onBackToLobby}
        className="movie-finished-action"
        data-testid="movie-finished-back-to-lobby"
      >
        {MOVIE_FINISHED_ACTION}
      </button>
    </section>
  );
}

export default MovieFinishedOverlay;
