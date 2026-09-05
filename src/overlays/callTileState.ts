export type CallTilePosition = {
  x: number;
  y: number;
};

export type CallTileSessionState = {
  position: CallTilePosition;
  isMinimized: boolean;
  isHidden: boolean;
};

export type RemoteCallPresentation = {
  showVideo: boolean;
  showMutedIndicator: boolean;
  statusLabel: string;
};

const tileMargin = 12;

export function createCallTileSessionState(): CallTileSessionState {
  const viewportWidth = typeof window === "undefined" ? 1280 : window.innerWidth;
  return {
    position: { x: Math.max(tileMargin, viewportWidth - 320), y: 32 },
    isMinimized: false,
    isHidden: false,
  };
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
