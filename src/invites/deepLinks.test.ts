import { describe, expect, it } from "vitest";
import { parseMovePartyInvite, previewInviteRoomCode } from "./deepLinks";

const roomCode = "ABCDEFGHIJKLMNOPQRSTUV";
const descriptor = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-";
const invite = `moveparty://join/${roomCode}#${descriptor}`;

describe("parseMovePartyInvite", () => {
  it("accepts a complete Move Party join invite", () => {
    expect(parseMovePartyInvite(` ${invite} `)).toEqual({
      ok: true,
      invite,
      roomCode,
    });
  });

  it("normalizes URL-encoded room codes", () => {
    expect(parseMovePartyInvite(`moveparty://join/${encodeURIComponent(roomCode)}#${descriptor}`))
      .toEqual({
        ok: true,
        invite,
        roomCode,
      });
  });

  it("rejects missing room codes", () => {
    expect(parseMovePartyInvite(`moveparty://join/#${descriptor}`)).toMatchObject({
      ok: false,
    });
  });

  it("rejects incomplete invite links without descriptors", () => {
    expect(parseMovePartyInvite(`moveparty://join/${roomCode}`)).toEqual({
      ok: false,
      message: "That invite link is incomplete. Ask the host to copy the full invite again.",
    });
  });

  it("rejects unsupported actions and query parameters", () => {
    expect(parseMovePartyInvite(`moveparty://open/${roomCode}#${descriptor}`)).toMatchObject({
      ok: false,
    });
    expect(
      parseMovePartyInvite(`moveparty://join/${roomCode}?unexpected=true#${descriptor}`),
    ).toMatchObject({
      ok: false,
    });
  });

  it("rejects whitespace and hidden characters", () => {
    expect(parseMovePartyInvite(`moveparty://join/${roomCode} #${descriptor}`)).toMatchObject({
      ok: false,
    });
  });
});

describe("previewInviteRoomCode", () => {
  it("shows the room-code prefix for valid links", () => {
    expect(previewInviteRoomCode(invite)).toBe("A B C D E F");
  });
});
