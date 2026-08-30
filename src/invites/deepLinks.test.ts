import { describe, expect, it } from "vitest";
import { parseMoviePartyInvite, previewInviteRoomCode } from "./deepLinks";

const roomCode = "ABCDEFGHIJKLMNOPQRSTUV";
const descriptor = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-";
const invite = `movieparty://join/${roomCode}#${descriptor}`;

describe("parseMoviePartyInvite", () => {
  it("accepts a complete Movie Party join invite", () => {
    expect(parseMoviePartyInvite(` ${invite} `)).toEqual({
      ok: true,
      invite,
      roomCode,
    });
  });

  it("normalizes URL-encoded room codes", () => {
    expect(parseMoviePartyInvite(`movieparty://join/${encodeURIComponent(roomCode)}#${descriptor}`))
      .toEqual({
        ok: true,
        invite,
        roomCode,
      });
  });

  it("keeps duplicate platform deliveries idempotent at the parser boundary", () => {
    expect(parseMoviePartyInvite(invite)).toEqual(parseMoviePartyInvite(invite));
  });

  it("rejects missing room codes", () => {
    expect(parseMoviePartyInvite(`movieparty://join/#${descriptor}`)).toMatchObject({
      ok: false,
    });
  });

  it("rejects incomplete invite links without descriptors", () => {
    expect(parseMoviePartyInvite(`movieparty://join/${roomCode}`)).toEqual({
      ok: false,
      message: "That invite link is incomplete. Ask the host to copy the full invite again.",
    });
  });

  it("rejects unsupported actions and query parameters", () => {
    expect(parseMoviePartyInvite(`movieparty://open/${roomCode}#${descriptor}`)).toMatchObject({
      ok: false,
    });
    expect(
      parseMoviePartyInvite(`movieparty://join/${roomCode}?unexpected=true#${descriptor}`),
    ).toMatchObject({
      ok: false,
    });
  });

  it("rejects whitespace and hidden characters", () => {
    expect(parseMoviePartyInvite(`movieparty://join/${roomCode} #${descriptor}`)).toMatchObject({
      ok: false,
    });
  });
});

describe("previewInviteRoomCode", () => {
  it("shows the room-code prefix for valid links", () => {
    expect(previewInviteRoomCode(invite)).toBe("A B C D E F");
  });
});
