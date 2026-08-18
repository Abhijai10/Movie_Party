export type CallMode = "VIDEO_VOICE" | "VOICE_ONLY" | "OFF";

export type MovePartyCallSignalType = "OFFER" | "ANSWER" | "ICE";

export type MovePartyCallSignal = {
  signalType: MovePartyCallSignalType;
  data: string;
};

export type CallMediaIntent = {
  audio: boolean;
  video: boolean | MediaTrackConstraints;
};

export type LocalCallLoopbackResult = {
  connected: boolean;
  localTrackKinds: string[];
  remoteTrackKinds: string[];
  signals: MovePartyCallSignal[];
  usedSyntheticMedia: boolean;
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
): Promise<LocalCallLoopbackResult> {
  const host = createMovePartyPeerConnection();
  const guest = createMovePartyPeerConnection();

  if (!host || !guest) {
    return {
      connected: false,
      localTrackKinds: [],
      remoteTrackKinds: [],
      signals: [],
      usedSyntheticMedia: false,
    };
  }

  const signals: MovePartyCallSignal[] = [];
  const remoteTrackKinds: string[] = [];
  const { stream, usedSyntheticMedia } = createSyntheticCallStream(
    mode,
    cameraEnabled,
    microphoneEnabled,
  );
  const localTrackKinds = stream.getTracks().map((track) => track.kind);

  const publishSignal = async (signal: MovePartyCallSignal) => {
    signals.push(signal);
    await onSignal?.(signal);
  };

  host.onicecandidate = (event) => {
    const candidate = event.candidate;
    if (candidate) {
      void publishSignal({
        signalType: "ICE",
        data: JSON.stringify(candidate.toJSON()),
      }).then(() => guest.addIceCandidate(candidate));
    }
  };
  guest.onicecandidate = (event) => {
    const candidate = event.candidate;
    if (candidate) {
      void publishSignal({
        signalType: "ICE",
        data: JSON.stringify(candidate.toJSON()),
      }).then(() => host.addIceCandidate(candidate));
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

  await waitForConnected(host, guest);
  await waitForIceGatheringComplete(host, guest);

  const connected =
    host.connectionState === "connected" ||
    guest.connectionState === "connected" ||
    remoteTrackKinds.length > 0;

  closeLoopback(host, guest, stream);

  return {
    connected,
    localTrackKinds,
    remoteTrackKinds,
    signals,
    usedSyntheticMedia,
  };
}

export function applyPrivacyModeToStream(stream: MediaStream): void {
  for (const track of stream.getTracks()) {
    track.enabled = false;
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
): Promise<{ stream: MediaStream; usedRealMedia: boolean }> {
  if (mode === "OFF") {
    return { stream: new MediaStream(), usedRealMedia: false };
  }

  const constraints: MediaStreamConstraints = {
    audio:
      microphoneEnabled
        ? { echoCancellation: true, noiseSuppression: true }
        : false,
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
      return { stream: new MediaStream(), usedRealMedia: false };
    }
    const stream = await devices.getUserMedia(constraints);
    return { stream, usedRealMedia: true };
  } catch {
    return { stream: new MediaStream(), usedRealMedia: false };
  }
}

async function waitForConnected(host: RTCPeerConnection, guest: RTCPeerConnection): Promise<void> {
  if (host.connectionState === "connected" || guest.connectionState === "connected") {
    return;
  }

  await new Promise<void>((resolve) => {
    const timeout = window.setTimeout(resolve, 2_000);
    const check = () => {
      if (host.connectionState === "connected" || guest.connectionState === "connected") {
        window.clearTimeout(timeout);
        resolve();
      }
    };
    host.addEventListener("connectionstatechange", check, { once: false });
    guest.addEventListener("connectionstatechange", check, { once: false });
  });
}

async function waitForIceGatheringComplete(
  host: RTCPeerConnection,
  guest: RTCPeerConnection,
): Promise<void> {
  await Promise.all([waitForIceComplete(host), waitForIceComplete(guest)]);
}

async function waitForIceComplete(connection: RTCPeerConnection): Promise<void> {
  if (connection.iceGatheringState === "complete") {
    return;
  }

  await new Promise<void>((resolve) => {
    const timeout = window.setTimeout(resolve, 1_000);
    const check = () => {
      if (connection.iceGatheringState === "complete") {
        window.clearTimeout(timeout);
        resolve();
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
  for (const track of stream.getTracks()) {
    track.stop();
  }
  host.close();
  guest.close();
}
