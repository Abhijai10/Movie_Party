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
