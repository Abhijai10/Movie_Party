export type CallMode = "VIDEO_VOICE" | "VOICE_ONLY" | "OFF";

export type MovePartyCallSignalType = "OFFER" | "ANSWER" | "ICE";

export type MovePartyCallSignal = {
  signalType: MovePartyCallSignalType;
  data: string;
};

export type CallConnectionStatus =
  | "connecting"
  | "connected"
  | "degraded"
  | "reconnecting"
  | "unavailable"
  | "ended";

export type CallMediaIntent = {
  audio: boolean;
  video: boolean | MediaTrackConstraints;
};

export type LocalCallLoopbackResult = {
  connected: boolean;
  status: CallConnectionStatus;
  localTrackKinds: string[];
  remoteTrackKinds: string[];
  signals: MovePartyCallSignal[];
  usedRealMedia: boolean;
  errorCode: string | null;
};

export function buildCallMediaIntent(mode: CallMode, cameraEnabled: boolean): CallMediaIntent {
  if (mode === "OFF") {
    return {
      audio: false,
      video: false,
    };
  }

  return {
    audio: true,
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

export function createSyntheticCallStream(
  mode: CallMode,
  cameraEnabled: boolean,
  microphoneEnabled: boolean,
): { stream: MediaStream; usedSyntheticMedia: boolean } {
  const stream = new MediaStream();

  if (mode === "OFF") {
    return { stream, usedSyntheticMedia: true };
  }

  const audioTrack = createSyntheticAudioTrack();
  if (audioTrack) {
    audioTrack.enabled = microphoneEnabled;
    stream.addTrack(audioTrack);
  }

  if (mode === "VIDEO_VOICE" && cameraEnabled) {
    const videoTrack = createSyntheticVideoTrack();
    if (videoTrack) {
      stream.addTrack(videoTrack);
    }
  }

  return { stream, usedSyntheticMedia: true };
}

export async function runLocalPeerConnectionLoopback(
  mode: CallMode,
  cameraEnabled: boolean,
  microphoneEnabled: boolean,
  onSignal?: (signal: MovePartyCallSignal) => Promise<void> | void,
  abortSignal?: AbortSignal,
): Promise<LocalCallLoopbackResult> {
  const host = createMovePartyPeerConnection();
  const guest = createMovePartyPeerConnection();

  if (!host || !guest) {
    return {
      connected: false,
      status: "unavailable",
      localTrackKinds: [],
      remoteTrackKinds: [],
      signals: [],
      usedRealMedia: false,
      errorCode: "MP-CALL-010",
    };
  }

  const signals: MovePartyCallSignal[] = [];
  const remoteTrackKinds: string[] = [];
  let stream = new MediaStream();
  let usedRealMedia = false;

  const publishSignal = async (signal: MovePartyCallSignal) => {
    if (abortSignal?.aborted) {
      return;
    }
    signals.push(signal);
    await onSignal?.(signal);
  };

  try {
    if (abortSignal?.aborted || mode === "OFF") {
      return {
        connected: false,
        status: "ended",
        localTrackKinds: [],
        remoteTrackKinds: [],
        signals,
        usedRealMedia: false,
        errorCode: null,
      };
    }

    const media = await acquireRealCallMedia(mode, cameraEnabled, microphoneEnabled);
    stream = media.stream;
    usedRealMedia = media.usedRealMedia;
    const localTrackKinds = stream.getTracks().map((track) => track.kind);
    if (!usedRealMedia) {
      return {
        connected: false,
        status: media.errorCode === "MP-CALL-012" ? "degraded" : "unavailable",
        localTrackKinds,
        remoteTrackKinds,
        signals,
        usedRealMedia,
        errorCode: media.errorCode,
      };
    }

    host.onicecandidate = (event) => {
      const candidate = event.candidate;
      if (candidate && !abortSignal?.aborted) {
        void publishSignal({
          signalType: "ICE",
          data: JSON.stringify(candidate.toJSON()),
        })
          .then(() => guest.addIceCandidate(candidate))
          .catch(() => undefined);
      }
    };
    guest.onicecandidate = (event) => {
      const candidate = event.candidate;
      if (candidate && !abortSignal?.aborted) {
        void publishSignal({
          signalType: "ICE",
          data: JSON.stringify(candidate.toJSON()),
        })
          .then(() => host.addIceCandidate(candidate))
          .catch(() => undefined);
      }
    };
    guest.ontrack = (event) => {
      remoteTrackKinds.push(event.track.kind);
    };

    for (const track of stream.getTracks()) {
      host.addTrack(track, stream);
    }

    const offer = await host.createOffer();
    await host.setLocalDescription(offer);
    await publishSignal({
      signalType: "OFFER",
      data: JSON.stringify(offer),
    });
    await guest.setRemoteDescription(offer);

    const answer = await guest.createAnswer();
    await guest.setLocalDescription(answer);
    await publishSignal({
      signalType: "ANSWER",
      data: JSON.stringify(answer),
    });
    await host.setRemoteDescription(answer);

    await waitForConnected(host, guest, abortSignal);
    await waitForIceGatheringComplete(host, guest, abortSignal);

    const connected =
      host.connectionState === "connected" ||
      guest.connectionState === "connected" ||
      remoteTrackKinds.length > 0;

    return {
      connected,
      status: connected ? "connected" : "connecting",
      localTrackKinds,
      remoteTrackKinds,
      signals,
      usedRealMedia,
      errorCode: connected ? null : "MP-CALL-014",
    };
  } catch {
    return {
      connected: false,
      status: abortSignal?.aborted ? "ended" : "unavailable",
      localTrackKinds: stream.getTracks().map((track) => track.kind),
      remoteTrackKinds,
      signals,
      usedRealMedia,
      errorCode: abortSignal?.aborted ? null : "MP-CALL-015",
    };
  } finally {
    closeLoopback(host, guest, stream);
  }
}

export function applyPrivacyModeToStream(stream: MediaStream): void {
  for (const track of stream.getTracks()) {
    track.enabled = false;
  }
}

export function stopCallStream(stream: MediaStream): void {
  for (const track of stream.getTracks()) {
    track.stop();
  }
}

function createSyntheticAudioTrack(): MediaStreamTrack | null {
  if (typeof AudioContext === "undefined") {
    return null;
  }

  const context = new AudioContext();
  const oscillator = context.createOscillator();
  const destination = context.createMediaStreamDestination();
  oscillator.connect(destination);
  oscillator.start();
  return destination.stream.getAudioTracks()[0] ?? null;
}

function createSyntheticVideoTrack(): MediaStreamTrack | null {
  const canvas = document.createElement("canvas");
  canvas.width = 640;
  canvas.height = 360;
  const context = canvas.getContext("2d");
  if (!context || typeof canvas.captureStream !== "function") {
    return null;
  }

  context.fillStyle = "#111827";
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.fillStyle = "#f8fafc";
  context.font = "24px sans-serif";
  context.fillText("Move Party call test", 32, 64);
  return canvas.captureStream(15).getVideoTracks()[0] ?? null;
}

export async function acquireRealCallMedia(
  mode: CallMode,
  cameraEnabled: boolean,
  microphoneEnabled: boolean,
): Promise<{ stream: MediaStream; usedRealMedia: boolean; errorCode: string | null }> {
  if (mode === "OFF") {
    return { stream: new MediaStream(), usedRealMedia: false, errorCode: null };
  }

  const constraints: MediaStreamConstraints = {
    audio: { echoCancellation: true, noiseSuppression: true },
    video:
      mode === "VIDEO_VOICE" && cameraEnabled
        ? {
            width: { ideal: 640, max: 640 },
            height: { ideal: 360, max: 360 },
            frameRate: { ideal: 15, max: 20 },
          }
        : false,
  };

  try {
    const devices = typeof navigator !== "undefined" ? navigator.mediaDevices : undefined;
    if (!devices?.getUserMedia) {
      return { stream: new MediaStream(), usedRealMedia: false, errorCode: "MP-CALL-010" };
    }
    const stream = await devices.getUserMedia(constraints);
    for (const track of stream.getAudioTracks()) {
      track.enabled = microphoneEnabled;
    }
    return { stream, usedRealMedia: true, errorCode: null };
  } catch (error) {
    const name = error instanceof DOMException ? error.name : "";
    const errorCode =
      name === "NotAllowedError" || name === "SecurityError" ? "MP-CALL-011" : "MP-CALL-012";
    return { stream: new MediaStream(), usedRealMedia: false, errorCode };
  }
}

async function waitForConnected(
  host: RTCPeerConnection,
  guest: RTCPeerConnection,
  abortSignal?: AbortSignal,
): Promise<void> {
  if (host.connectionState === "connected" || guest.connectionState === "connected") {
    return;
  }

  await new Promise<void>((resolve) => {
    let settled = false;
    let check: () => void;
    const cleanup = () => {
      host.removeEventListener("connectionstatechange", check);
      guest.removeEventListener("connectionstatechange", check);
    };
    const done = () => {
      if (settled) {
        return;
      }
      settled = true;
      window.clearTimeout(timeout);
      cleanup();
      resolve();
    };
    const timeout = window.setTimeout(done, 2_000);
    check = () => {
      if (
        abortSignal?.aborted ||
        host.connectionState === "connected" ||
        guest.connectionState === "connected"
      ) {
        done();
      }
    };
    host.addEventListener("connectionstatechange", check, { once: false });
    guest.addEventListener("connectionstatechange", check, { once: false });
  });
}

async function waitForIceGatheringComplete(
  host: RTCPeerConnection,
  guest: RTCPeerConnection,
  abortSignal?: AbortSignal,
): Promise<void> {
  await Promise.all([
    waitForIceComplete(host, abortSignal),
    waitForIceComplete(guest, abortSignal),
  ]);
}

async function waitForIceComplete(
  connection: RTCPeerConnection,
  abortSignal?: AbortSignal,
): Promise<void> {
  if (connection.iceGatheringState === "complete") {
    return;
  }

  await new Promise<void>((resolve) => {
    let settled = false;
    let check: () => void;
    const cleanup = () => {
      connection.removeEventListener("icegatheringstatechange", check);
    };
    const done = () => {
      if (settled) {
        return;
      }
      settled = true;
      window.clearTimeout(timeout);
      cleanup();
      resolve();
    };
    const timeout = window.setTimeout(done, 1_000);
    check = () => {
      if (abortSignal?.aborted || connection.iceGatheringState === "complete") {
        done();
      }
    };
    connection.addEventListener("icegatheringstatechange", check, { once: false });
  });
}

function closeLoopback(
  host: RTCPeerConnection,
  guest: RTCPeerConnection,
  stream: MediaStream,
): void {
  host.onicecandidate = null;
  guest.onicecandidate = null;
  guest.ontrack = null;
  stopCallStream(stream);
  host.close();
  guest.close();
}
