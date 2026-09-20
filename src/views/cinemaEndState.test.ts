import { describe, expect, it } from "vitest";

import {
  MOVIE_FINISHED_ACTION,
  MOVIE_FINISHED_BODY,
  MOVIE_FINISHED_TITLE,
  ROOM_STATE_ENDED,
  playbackControlsEnabled,
  playbackToggleAction,
  shouldShowBufferingOverlay,
  syncIndicatorFor,
  isMovieEnded,
} from "./cinemaEndState";

/**
 * AUD-16 — the end-of-media UI.
 *
 * The backend has reported room `ENDED` / player `COMPLETED` since AUD-03. What
 * was missing was any UI that surfaced it, so an ended room looked like a
 * stalled player: no message, a "Syncing" indicator, and a play button offering
 * *resume* (which called `host_play` on an ended room).
 *
 * Each describe block carries its own **negative control** — a case that fails
 * if the defect is still present. A test that passes on the broken code proves
 * nothing, so every decision below is checked in both directions.
 */

describe("AUD-16 / 1 — ENDED renders the finished state", () => {
  it("recognises the ended room", () => {
    expect(isMovieEnded(ROOM_STATE_ENDED)).toBe(true);
  });

  it("carries the restrained copy the overlay renders", () => {
    expect(MOVIE_FINISHED_TITLE).toBe("Movie Finished");
    expect(MOVIE_FINISHED_BODY).toBe("The movie has ended.");
    expect(MOVIE_FINISHED_ACTION).toBe("Back to Lobby");
  });

  it("offers exactly one action, and it is not a replay affordance", () => {
    // Replay is a product decision with real semantics and is deliberately NOT
    // invented in this batch. Guard the wording so a later change has to make
    // that decision on purpose rather than by wording drift.
    expect(MOVIE_FINISHED_ACTION.toLowerCase()).not.toMatch(/replay|watch again|restart/);
  });

  /**
   * Negative control. If `isMovieEnded` returned `true` unconditionally — the
   * shape a "make the test green" shortcut takes — the finished card would show
   * during a normal movie. These states must NOT be treated as ended.
   */
  it("negative control: no other room state is treated as finished", () => {
    for (const state of [
      "LOBBY",
      "CREATED",
      "WAITING_FOR_GUEST",
      "PREPARING",
      "READYCHECK",
      "PLAYING",
      "PAUSING",
      "PAUSED",
      "SEEKING",
      "BUFFERING",
      "RECONNECTING",
      "ERROR",
      "",
    ]) {
      expect(isMovieEnded(state)).toBe(false);
    }
  });
});

describe("AUD-16 / 2 — ENDED does not render the buffering overlay", () => {
  it("suppresses the overlay at ENDED from either input", () => {
    // `strictSyncPaused` is the flag ADV-05 stopped the backend setting at EOF.
    expect(
      shouldShowBufferingOverlay({
        roomState: "ENDED",
        strictSyncPaused: true,
        bufferingParticipant: null,
      }),
    ).toBe(false);
    // `bufferingParticipant` is a SECOND, independent input that can still be
    // set as the credits roll — so the ADV-05 fix alone did not keep this
    // overlay off the end of the movie. This is the case that proves it.
    expect(
      shouldShowBufferingOverlay({
        roomState: "ENDED",
        strictSyncPaused: false,
        bufferingParticipant: "Guest",
      }),
    ).toBe(false);
    expect(
      shouldShowBufferingOverlay({
        roomState: "ENDED",
        strictSyncPaused: true,
        bufferingParticipant: "Guest",
      }),
    ).toBe(false);
  });

  /**
   * Negative control, and the reason the assertions above mean something: the
   * same inputs in a *non*-ended room DO show the overlay. If the function
   * simply returned `false`, this block would fail.
   */
  it("negative control: the overlay still works while the room is live", () => {
    expect(
      shouldShowBufferingOverlay({
        roomState: "PLAYING",
        strictSyncPaused: true,
        bufferingParticipant: null,
      }),
    ).toBe(true);
    expect(
      shouldShowBufferingOverlay({
        roomState: "BUFFERING",
        strictSyncPaused: false,
        bufferingParticipant: "Guest",
      }),
    ).toBe(true);
    // And it is genuinely off when nothing is buffering.
    expect(
      shouldShowBufferingOverlay({
        roomState: "PLAYING",
        strictSyncPaused: false,
        bufferingParticipant: null,
      }),
    ).toBe(false);
  });
});

describe("AUD-16 / 3 — ENDED exposes no active play/resume action", () => {
  it("has no playback action at ENDED", () => {
    expect(playbackToggleAction(ROOM_STATE_ENDED)).toBe("NONE");
  });

  it("disables the play/pause/seek controls at ENDED", () => {
    expect(playbackControlsEnabled(ROOM_STATE_ENDED)).toBe(false);
  });

  /**
   * The defect itself, stated as an assertion.
   *
   * CinemaView used to compute the action with
   * `roomState === "PLAYING" ? pausePlayback : resumePlayback`. At ENDED that
   * falls to the `else`, i.e. `resumePlayback` -> `host_play` on an ended room.
   * Reproducing the old expression here and asserting the new one differs is
   * what makes this test able to fail on the broken code.
   */
  it("negative control: the old expression offered resume at ENDED", () => {
    const oldExpression = (roomState: string): "PLAY" | "PAUSE" =>
      roomState === "PLAYING" ? "PAUSE" : "PLAY";

    expect(oldExpression(ROOM_STATE_ENDED)).toBe("PLAY"); // the defect
    expect(playbackToggleAction(ROOM_STATE_ENDED)).not.toBe(oldExpression(ROOM_STATE_ENDED));
    expect(playbackToggleAction(ROOM_STATE_ENDED)).not.toBe("PLAY");
    expect(playbackToggleAction(ROOM_STATE_ENDED)).not.toBe("PAUSE");
  });

  it("negative control: playback is still controllable in live states", () => {
    expect(playbackToggleAction("PLAYING")).toBe("PAUSE");
    expect(playbackToggleAction("PAUSED")).toBe("PLAY");
    expect(playbackToggleAction("BUFFERING")).toBe("PLAY");
    expect(playbackToggleAction("READYCHECK")).toBe("PLAY");

    expect(playbackControlsEnabled("PLAYING")).toBe(true);
    expect(playbackControlsEnabled("PAUSED")).toBe(true);
  });
});

describe("AUD-16 — the sync indicator never claims to be syncing an ended room", () => {
  it("shows nothing at ENDED rather than 'Syncing'", () => {
    expect(syncIndicatorFor(ROOM_STATE_ENDED)).toBe("NONE");
  });

  /**
   * Negative control: the old indicator was a two-branch
   * `roomState === "PLAYING" ? In sync : Syncing`, so ENDED landed in the
   * "Syncing" branch. Reproduce that and show the new value differs.
   */
  it("negative control: the old indicator read 'Syncing' at ENDED", () => {
    const oldExpression = (roomState: string): "IN_SYNC" | "SYNCING" =>
      roomState === "PLAYING" ? "IN_SYNC" : "SYNCING";

    expect(oldExpression(ROOM_STATE_ENDED)).toBe("SYNCING"); // the defect
    expect(syncIndicatorFor(ROOM_STATE_ENDED)).not.toBe(oldExpression(ROOM_STATE_ENDED));
  });

  it("negative control: live states keep their real indicator", () => {
    expect(syncIndicatorFor("PLAYING")).toBe("IN_SYNC");
    expect(syncIndicatorFor("BUFFERING")).toBe("SYNCING");
    expect(syncIndicatorFor("RECONNECTING")).toBe("SYNCING");
  });
});
