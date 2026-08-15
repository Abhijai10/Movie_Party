export type CallMode = "VIDEO_VOICE" | "VOICE_ONLY" | "OFF";

export type CallMediaIntent = {
  audio: boolean;
  video: boolean | MediaTrackConstraints;
};

export function buildCallMediaIntent(
  mode: CallMode,
  cameraEnabled: boolean,
  microphoneEnabled: boolean,
): CallMediaIntent {
  if (mode === "OFF") {
    return {
      audio: false,
      video: false,
    };
  }

  return {
    audio: microphoneEnabled,
    video:
      mode === "VIDEO_VOICE" && cameraEnabled
        ? {
            width: { ideal: 640, max: 640 },
            height: { ideal: 360, max: 360 },
            frameRate: { ideal: 15, max: 20 },
          }
        : false,
  };
}

export function createMovePartyPeerConnection(): RTCPeerConnection | null {
  if (typeof RTCPeerConnection === "undefined") {
    return null;
  }

  return new RTCPeerConnection({
    iceServers: [],
    bundlePolicy: "max-bundle",
  });
}
