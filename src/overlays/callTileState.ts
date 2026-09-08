export type CallTilePosition = {
  x: number;
  y: number;
};

export type CallTileSessionState = {
  position: CallTilePosition;
  isMinimized: boolean;
  isHidden: boolean;
  /**
   * UI_UX_SPEC §28: card width in px — default 220, user-resizable within
   * the 120–360 clamp. The minimized circle (§29) ignores this value.
   */
  sizePx: number;
};

/** §28: default card width. */
export const CAMERA_CARD_DEFAULT_PX = 220;
/** §28: resizable clamp. */
export const CAMERA_CARD_MIN_PX = 120;
export const CAMERA_CARD_MAX_PX = 360;
/** §28: "Persist location locally" — the localStorage key. */
const CAMERA_CARD_POSITION_KEY = "mp_camera_card_position";

export type RemoteCallPresentation = {
  showVideo: boolean;
  showMutedIndicator: boolean;
  statusLabel: string;
};

const tileMargin = 12;

export function createCallTileSessionState(): CallTileSessionState {
  const viewportWidth = typeof window === "undefined" ? 1280 : window.innerWidth;
  // §28: top-right 24px margin default; a persisted location wins.
  const fallback = { x: viewportWidth - CAMERA_CARD_DEFAULT_PX - 24, y: 24 };
  return {
    position: loadCallTilePosition(fallback),
    isMinimized: false,
    isHidden: false,
    sizePx: CAMERA_CARD_DEFAULT_PX,
  };
}

/**
 * §28 "Persist location locally": the card position survives reloads via
 * localStorage. Corrupt or out-of-bounds values are ignored (the clamp
 * still runs on every drag), so a stale entry can never strand the card
 * off-screen.
 */
export function loadCallTilePosition(
  fallback: CallTilePosition,
  storage: Pick<Storage, "getItem"> | null = typeof window === "undefined"
    ? null
    : window.localStorage,
): CallTilePosition {
  if (storage == null) {
    return fallback;
  }
  try {
    const raw = storage.getItem(CAMERA_CARD_POSITION_KEY);
    if (raw == null) {
      return fallback;
    }
    const parsed = JSON.parse(raw) as { x?: unknown; y?: unknown };
    if (
      typeof parsed.x === "number" &&
      Number.isFinite(parsed.x) &&
      typeof parsed.y === "number" &&
      Number.isFinite(parsed.y)
    ) {
      return { x: parsed.x, y: parsed.y };
    }
  } catch {
    // Corrupt storage is not a reason to fail; fall back to the default.
  }
  return fallback;
}

/**
 * §28: persist the card location. Called on drag end.
 */
export function saveCallTilePosition(
  position: CallTilePosition,
  storage: Pick<Storage, "setItem"> | null = typeof window === "undefined"
    ? null
    : window.localStorage,
): void {
  if (storage == null) {
    return;
  }
  try {
    storage.setItem(CAMERA_CARD_POSITION_KEY, JSON.stringify(position));
  } catch {
    // Private-mode storage failures are non-fatal; the position simply
    // does not persist.
  }
}

/** §28: clamp a user resize to the 120–360 spec range. */
export function clampCameraCardSize(sizePx: number): number {
  if (!Number.isFinite(sizePx)) {
    return CAMERA_CARD_DEFAULT_PX;
  }
  return Math.min(CAMERA_CARD_MAX_PX, Math.max(CAMERA_CARD_MIN_PX, Math.round(sizePx)));
}

export function clampCallTilePosition(
  position: CallTilePosition,
  viewport: { width: number; height: number },
  tile: { width: number; height: number },
): CallTilePosition {
  return {
    x: Math.min(Math.max(tileMargin, position.x), Math.max(tileMargin, viewport.width - tile.width - tileMargin)),
    y: Math.min(Math.max(tileMargin, position.y), Math.max(tileMargin, viewport.height - tile.height - tileMargin)),
  };
}

export function deriveRemoteCallPresentation(
  remoteCameraEnabled: boolean,
  remoteMicrophoneEnabled: boolean,
  remoteConnected: boolean,
): RemoteCallPresentation {
  return {
    showVideo: remoteCameraEnabled && remoteConnected,
    showMutedIndicator: !remoteMicrophoneEnabled,
    statusLabel: remoteConnected ? "Present" : "Away",
  };
}

/**
 * Close the call tile locally. Hiding is a purely local presentation
 * choice — the call itself (tracks, session, peer connection) is never
 * torn down by this action. Hanging up is a separate, explicit action.
 */
export function closeCallTileLocally(current: CallTileSessionState): CallTileSessionState {
  return { ...current, isHidden: true };
}

/**
 * Restore a locally-hidden tile. Position and minimized state are the
 * user's own prior choices and come back unchanged.
 */
export function showCallTile(current: CallTileSessionState): CallTileSessionState {
  return { ...current, isHidden: false };
}

/**
 * Minimize keeps the video/avatar stage visible and draggable while the
 * header, labels, and controls are compacted away; restoring returns the
 * full tile.
 */
export function minimizeCallTile(current: CallTileSessionState): CallTileSessionState {
  return { ...current, isMinimized: true };
}

export function restoreCallTile(current: CallTileSessionState): CallTileSessionState {
  return { ...current, isMinimized: false };
}

/**
 * Whether the drag surface may start a drag. Interactive controls inside
 * the tile (buttons, inputs) must never initiate dragging.
 */
export function canInitiateDragFrom(target: Element | null): boolean {
  if (target == null) {
    return false;
  }
  const nonDraggable = "button, input, textarea, select, a, [data-call-tile-control]";
  return !(target instanceof Element && target.closest(nonDraggable) != null);
}
