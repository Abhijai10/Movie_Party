const MOVIE_PARTY_SCHEME = "movieparty:";
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

export function parseMoviePartyInvite(input: string): InviteParseResult {
  const value = input.trim();

  if (!value) {
    return { ok: false, message: "Enter a Movie Party invite link." };
  }

  if (value.length > MAX_INVITE_LENGTH) {
    return { ok: false, message: "That invite link is too long." };
  }

  if (/[\u0000-\u001f\u007f]|\s/.test(value)) {
    return { ok: false, message: "Invite links cannot contain spaces or hidden characters." };
  }

  if (!value.toLowerCase().startsWith("movieparty://")) {
    return { ok: false, message: "Paste a Movie Party invite link that starts with movieparty://." };
  }

  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return { ok: false, message: "That invite link is malformed." };
  }

  if (url.protocol.toLowerCase() !== MOVIE_PARTY_SCHEME) {
    return { ok: false, message: "That invite link uses an unsupported scheme." };
  }

  if (url.hostname.toLowerCase() !== JOIN_ACTION) {
    return { ok: false, message: "That Movie Party link is not a join invite." };
  }

  if (url.search) {
    return { ok: false, message: "Movie Party invite links cannot include query parameters." };
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
    invite: `movieparty://join/${roomCode}#${descriptor}`,
    roomCode,
  };
}

export function previewInviteRoomCode(input: string): string {
  const parsed = parseMoviePartyInvite(input);
  if (parsed.ok) {
    return parsed.roomCode.slice(0, 6).padEnd(6, "•").split("").join(" ");
  }

  const value = input.trim();
  if (!value) {
    return "— — — — — —";
  }

  return value.slice(0, 6).padEnd(6, "•").split("").join(" ");
}

// ── Friend invites (movieparty://friend/…) ─────────────────────────────────

export type FriendInviteParseResult =
  | { ok: true; peerKey: string; displayName: string; invite: string }
  | { ok: false; message: string };

const FRIEND_PREFIX = "movieparty://friend/";
const MAX_FRIEND_INVITE_LENGTH = 1024;

/** Encode a friend-invite payload as compact base64url JSON. */
function toBase64Url(value: string): string {
  const bytes = new TextEncoder().encode(value);
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(value: string): string | null {
  const normalized = value.replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized + "=".repeat((4 - (normalized.length % 4)) % 4);
  try {
    const binary = atob(padded);
    const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}

/** Build the link the inviter shares: their peer key + chosen name. */
export function buildFriendInviteLink(peerKey: string, displayName: string): string {
  const payload = JSON.stringify({ n: displayName, pk: peerKey });
  return `${FRIEND_PREFIX}${toBase64Url(payload)}`;
}

/** Parse a movieparty://friend/ invite (the receiving side of Add Friend). */
export function parseFriendInvite(input: string): FriendInviteParseResult {
  const value = input.trim();

  if (!value) {
    return { ok: false, message: "Paste a friend invite link." };
  }
  if (value.length > MAX_FRIEND_INVITE_LENGTH) {
    return { ok: false, message: "That friend invite link is too long." };
  }
  if (!value.toLowerCase().startsWith(FRIEND_PREFIX)) {
    return { ok: false, message: "Friend invites start with movieparty://friend/." };
  }

  const encoded = value.slice(FRIEND_PREFIX.length).split("#")[0] ?? "";
  if (!encoded) {
    return { ok: false, message: "That friend invite is missing its code." };
  }
  if (!BASE64URL_PATTERN.test(encoded)) {
    return { ok: false, message: "That friend invite code is not valid." };
  }

  const decoded = fromBase64Url(encoded);
  if (decoded == null) {
    return { ok: false, message: "That friend invite could not be read." };
  }

  let payload: { n?: unknown; pk?: unknown };
  try {
    payload = JSON.parse(decoded) as { n?: unknown; pk?: unknown };
  } catch {
    return { ok: false, message: "That friend invite is malformed." };
  }

  // Identity-only contract: the payload must carry exactly {n, pk}.
  // Extra fields (an attempted auth-key/token smuggle) invalidate the
  // link rather than being silently honored — a friend invite must
  // never be able to authenticate another person's device as anyone.
  const keys = Object.keys(payload).sort();
  if (keys.length !== 2 || !keys.includes("n") || !keys.includes("pk")) {
    return { ok: false, message: "That friend invite is not a valid identity card." };
  }

  if (typeof payload.pk !== "string" || typeof payload.n !== "string") {
    return { ok: false, message: "That friend invite is incomplete." };
  }

  const peerKey = payload.pk;
  const displayName = payload.n;
  if (!peerKey.includes(".") || peerKey.length < 4 || peerKey.length > 128) {
    return { ok: false, message: "That friend invite has an invalid device identity." };
  }
  if (displayName.length === 0 || displayName.length > 40) {
    return { ok: false, message: "That friend invite has an invalid name." };
  }

  return { ok: true, peerKey, displayName, invite: `${FRIEND_PREFIX}${encoded}` };
}
