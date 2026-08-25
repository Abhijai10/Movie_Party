const MOVE_PARTY_SCHEME = "moveparty:";
const JOIN_ACTION = "join";
const MAX_INVITE_LENGTH = 4096;
const BASE64URL_PATTERN = /^[A-Za-z0-9_-]+$/;

export type InviteParseResult =
  | {
      ok: true;
      invite: string;
      roomCode: string;
    }
  | {
      ok: false;
      message: string;
    };

export function parseMovePartyInvite(input: string): InviteParseResult {
  const value = input.trim();

  if (!value) {
    return { ok: false, message: "Enter a Move Party invite link." };
  }

  if (value.length > MAX_INVITE_LENGTH) {
    return { ok: false, message: "That invite link is too long." };
  }

  if (/[\u0000-\u001f\u007f]|\s/.test(value)) {
    return { ok: false, message: "Invite links cannot contain spaces or hidden characters." };
  }

  if (!value.toLowerCase().startsWith("moveparty://")) {
    return { ok: false, message: "Paste a Move Party invite link that starts with moveparty://." };
  }

  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return { ok: false, message: "That invite link is malformed." };
  }

  if (url.protocol.toLowerCase() !== MOVE_PARTY_SCHEME) {
    return { ok: false, message: "That invite link uses an unsupported scheme." };
  }

  if (url.hostname.toLowerCase() !== JOIN_ACTION) {
    return { ok: false, message: "That Move Party link is not a join invite." };
  }

  if (url.search) {
    return { ok: false, message: "Move Party invite links cannot include query parameters." };
  }

  const encodedRoomCode = url.pathname.replace(/^\/+/, "");
  if (!encodedRoomCode) {
    return { ok: false, message: "That invite link is missing a room code." };
  }

  if (encodedRoomCode.includes("/")) {
    return { ok: false, message: "That invite link has an invalid room code." };
  }

  let roomCode: string;
  try {
    roomCode = decodeURIComponent(encodedRoomCode);
  } catch {
    return { ok: false, message: "That invite room code is not valid URL text." };
  }

  if (!roomCode || !BASE64URL_PATTERN.test(roomCode)) {
    return { ok: false, message: "That invite room code is not valid." };
  }

  const descriptor = url.hash.slice(1);
  if (!descriptor) {
    return {
      ok: false,
      message: "That invite link is incomplete. Ask the host to copy the full invite again.",
    };
  }

  if (!BASE64URL_PATTERN.test(descriptor)) {
    return { ok: false, message: "That invite descriptor is not valid." };
  }

  return {
    ok: true,
    invite: `moveparty://join/${roomCode}#${descriptor}`,
    roomCode,
  };
}

export function previewInviteRoomCode(input: string): string {
  const parsed = parseMovePartyInvite(input);
  if (parsed.ok) {
    return parsed.roomCode.slice(0, 6).padEnd(6, "•").split("").join(" ");
  }

  const value = input.trim();
  if (!value) {
    return "— — — — — —";
  }

  return value.slice(0, 6).padEnd(6, "•").split("").join(" ");
}
