/**
 * real cross-device call session.
 *
 * Replaces the local loopback self-test as the production path. Each
 * device runs ONE RTCPeerConnection; signaling travels over the existing
 * QUIC relay (submit_call_signal → ServerEvent::CallSignal broadcast →
 * snapshot.callSignals).
 *
 * Design constraints (from the batch plan + PROTOCOL_SPEC):
 * - Host is the offerer (host authority, §15); guest answers.
 * - NON-TRICKLE ICE: both sides wait for icegatheringcomplete and embed
 *   candidates in the SDP. The guest→host signal path is a fire-and-forget
 *   tokio::spawn over QUIC, so candidate ordering is not guaranteed —
 *   embedding removes the ordering hazard entirely. ICE messages are still
 *   relayed for later TURN/trickle support (§52 lists ICE).
 * - Applied signals are tracked by snapshot index (a monotonic cursor over
 *   callSignals), NOT createdHostTimeUs (submit uses host_time_us, receive
 *   uses quic::monotonic_us — mixed clock domains) and NOT string equality
 *   (a genuine renegotiation can repeat an OFFER tag).
 * - track.enabled stays local-only (PRD §41): remote toggles arrive via the
 *   participant snapshot, never negotiated.
 * - Mode downgrade (VIDEO_VOICE → VOICE_ONLY → OFF) and privacy mode tear
 *   the session down and require explicit re-enable (no silent fallbacks,
 * §29).
 */

import {
  acquireRealCallMedia,
  cameraToggleAction,
  createMoviePartyPeerConnection,
  stopCallStream,
  type CallMode,
  type CallConnectionStatus,
} from "./webrtc";

export type CallRole = "HOST" | "GUEST";

/**
 * the runtime-recommended camera tier as it crosses the snapshot
 * boundary. Mirrors `call::CameraState` (camelCase serde) from the Rust
 * ladder; `tier` orders A < B < C < D (D = disabled).
 */
export type CameraTierState = {
  enabled: boolean;
  tier: "A" | "B" | "C" | "D";
  width: number;
  height: number;
  fps: number;
  targetBitrateBps: number;
};

/**
 * Pure tier-→sender-parameter mapping. Given the snapshot's
 * camera state and a live video sender's current parameters, produce the
 * next parameters to set. Encoder caps only: no SDP renegotiation.
 *
 * - maxBitrate = targetBitrateBps (0 for a disabled tier is clamped to
 *   1 kbps — 0 is treated by some stacks as "unlimited").
 * - maxFramerate = fps.
 */
export function senderParametersForTier(
  camera: CameraTierState,
  current: RTCRtpSendParameters,
): RTCRtpSendParameters {
  const encodings: RTCRtpEncodingParameters[] = current.encodings.map((encoding) => ({
    ...encoding,
    maxBitrate: camera.targetBitrateBps > 0 ? camera.targetBitrateBps : 1_000,
    maxFramerate: camera.fps > 0 ? camera.fps : 1,
  }));
  return { ...current, encodings };
}

/**
 * Pure tier-→track-constraint mapping. Downscale the captured
 * resolution to the tier's frame (aspect preserved via width/height pair).
 * An exact frame is requested with ideal (browser may pick nearest).
 */
export function trackConstraintsForTier(camera: CameraTierState): MediaTrackConstraints {
  return {
    width: { ideal: Math.max(camera.width, 1) },
    height: { ideal: Math.max(camera.height, 1) },
    frameRate: { ideal: Math.max(camera.fps, 1) },
  };
}


/** Snapshot callSignals entries (signalType is an unvalidated string). */
export type IncomingCallSignal = {
  signalType: string;
  data: string;
};

/** A signal after shape validation — safe to hand to the session. */
export type WellFormedCallSignal = {
  signalType: "OFFER" | "ANSWER" | "ICE" | "RENEGOTIATE";
  data: string;
};

export type MoviePartyCallSignalType = WellFormedCallSignal["signalType"];

/**
 * Pure cursor-based deduplication for the incoming signal stream: returns
 * the not-yet-applied signals (in order) and the advanced cursor.
 * Duplicate delivery (guest self-echo via host broadcast, or a re-broadcast
 * after reconnect) can never double-apply a signal because the cursor only
 * moves forward and is derived from the snapshot array index, which the
 * host appends monotonically.
 *
 * Malformed entries are filtered OUT of `pending` but still consumed by
 * the cursor — the host already validated them when appending; a malformed
 * entry reaching the view means the transport/ledger accepted it and the
 * view's job is to skip, not crash (§67).
 *
 * The returned batch is COALESCED per role before application. A snapshot
 * delta can legitimately contain a stale OFFER published before a
 * RENEGOTIATE marker plus a fresh OFFER after it (session restart race);
 * applying both would have the guest answer twice and poison the ledger
 * (MP-CALL-004 at the host). submit_call_signal also appends every
 * locally-published signal back into the publisher's own snapshot, so
 * each side sees its own publications and must drop them by role:
 *
 * - GUEST: keeps only the LAST OFFER at/after the last RENEGOTIATE marker
 *   plus the ICE following it. Drops ANSWER and all markers (the guest
 *   never applies them; its own marker is the self-echo).
 * - HOST: keeps ANSWER and ICE. Drops OFFER (the host is the offerer; a
 *   guest OFFER is a protocol violation the session arm also guards).
 *   Keeps the LAST marker to poke a re-offer — UNLESS it is the host's
 *   own self-echo (matching `ownMarkerId`) or an ANSWER follows it (the
 *   exchange it requested is already progressing; re-poking would publish
 *   an offer nobody waits for).
 */
export function nextPendingSignals(
  signals: ReadonlyArray<IncomingCallSignal>,
  cursor: number,
  role: CallRole,
  options?: { ownMarkerId?: string },
): { pending: WellFormedCallSignal[]; nextCursor: number } {
  if (signals.length <= cursor) {
    return { pending: [], nextCursor: cursor };
  }
  const batch = signals
    .slice(cursor)
    .filter(isWellFormedSignal)
    .map((signal) => signal as WellFormedCallSignal);
  const lastMarkerIndex = batch.reduce(
    (last, signal, index) => (signal.signalType === "RENEGOTIATE" ? index : last),
    -1,
  );
  if (role === "GUEST") {
    // Find the last OFFER at/after the last marker; drop everything the
    // peer's fresh exchange has already superseded (earlier offers,
    // stale ICE predating the marker, earlier markers, self-echoed
    // ANSWERs and markers).
    let lastOfferIndex = -1;
    for (let index = lastMarkerIndex + 1; index < batch.length; index += 1) {
      if (batch[index]?.signalType === "OFFER") {
        lastOfferIndex = index;
      }
    }
    const pending: WellFormedCallSignal[] = [];
    if (lastOfferIndex >= 0) {
      const offer = batch[lastOfferIndex] as WellFormedCallSignal;
      // ICE after the kept offer belongs to the same exchange (non-trickle
      // embeds them, but the relay path keeps them valid).
      const ice = batch.filter(
        (signal, index) => index > lastOfferIndex && signal.signalType === "ICE",
      );
      pending.push(offer, ...ice);
    }
    return { pending, nextCursor: signals.length };
  }
  // HOST: keep ANSWER/ICE; a peer marker pokes a re-offer via the
  // session's RENEGOTIATE arm. Coalesce markers to the LAST one and drop
  // it when an ANSWER follows (progressing exchange) or when it is the
  // host's own self-echo (submit_call_signal appends locally-published
  // signals back into the local snapshot; applying it would re-poke into
  // an offer that is already pending — MP-CALL-002 at the own submit
  // ledger).
  let hostMarkerIndex = -1;
  for (let index = batch.length - 1; index >= 0; index -= 1) {
    if (batch[index]?.signalType === "RENEGOTIATE") {
      hostMarkerIndex = index;
      break;
    }
  }
  const answerAfterMarker =
    hostMarkerIndex >= 0 &&
    batch.some(
      (signal, index) => index > hostMarkerIndex && signal.signalType === "ANSWER",
    );
  const markerIsSelfEcho =
    hostMarkerIndex >= 0 &&
    options?.ownMarkerId !== undefined &&
    parseMarkerId(batch[hostMarkerIndex] as WellFormedCallSignal) === options.ownMarkerId;
  const keepMarker = hostMarkerIndex >= 0 && !answerAfterMarker && !markerIsSelfEcho;
  return {
    pending: batch.filter(
      (signal, index) =>
        signal.signalType === "ANSWER" ||
        signal.signalType === "ICE" ||
        (keepMarker && signal.signalType === "RENEGOTIATE" && index === hostMarkerIndex),
    ),
    nextCursor: signals.length,
  };
}

/** Extracts the session-unique id from a RENEGOTIATE marker payload. */
export function parseMarkerId(signal: WellFormedCallSignal): string | null {
  try {
    const parsed = JSON.parse(signal.data) as { id?: unknown };
    return typeof parsed.id === "string" ? parsed.id : null;
  } catch {
    return null;
  }
}

export function isWellFormedSignal(signal: IncomingCallSignal): boolean {
  return (
    typeof signal.signalType === "string" &&
    SIGNAL_TYPES.has(signal.signalType) &&
    typeof signal.data === "string" &&
    signal.data.length > 0 &&
    signal.data.length <= 64 * 1024
  );
}

const SIGNAL_TYPES: ReadonlySet<string> = new Set(["OFFER", "ANSWER", "ICE", "RENEGOTIATE"]);

/**
 * A live cross-device call session on this device. One instance per call
 * lifecycle; the view creates it when the call turns on and tears it down
 * on mode/privacy change.
 */
export type LiveCallSession = {
  /** Role of this device in the room (offerer or answerer). */
  role: CallRole;
  /** Session-unique marker id used in this session's RENEGOTIATE marker. */
  markerId: string;
  /** Live local media (camera/mic). Owner: this session. */
  localStream: MediaStream;
  /** Remote media assembled from ontrack events. */
  remoteStream: MediaStream;
  /** The single peer connection. */
  peerConnection: RTCPeerConnection;
  /** True when getUserMedia returned real devices (false = degraded). */
  usedRealMedia: boolean;
  /** MP-CALL error code if media acquisition degraded. */
  mediaErrorCode: string | null;
  /**
   * (PRD §41): apply an adaptive-camera-tier state to
   * the LIVE sender. Movie-first: the runtime ladder decides the tier;
   * this maps it onto WebRTC sender parameters + track constraints.
   *
   * - maxBitrate/maxFramerate on the video sender (encoder-level; no SDP
   *   renegotiation is required, so the non-trickle session keeps flowing
   *   without a new exchange).
   * - applyConstraints on the video track downscales the captured
   *   resolution; failure is non-fatal (the sender caps still hold).
   * - A camera-disabled tier (D) only freezes the local track (the peer
   *   sees frozen video; track.enabled semantics per PRD §41 "freeze"
   *   step).
   *
   * Returns false when there is no video sender/track to constrain (e.g.
   * VOICE_ONLY) — the ladder state still flows via the snapshot.
   */
  applyCameraTier: (tier: CameraTierState) => Promise<boolean>;
  /** Apply a peer-originated signal (cursor-guarded by the view). */
  applyRemoteSignal: (signal: WellFormedCallSignal) => Promise<void>;
  /**
   * Local-only media toggle — never renegotiates (PRD §41). Sets
   * track.enabled per kind; a disabled-at-start track that is later
   * enabled already flows (it was acquired and added — enabled=false
   * only mutes it).
   */
  setLocalTracksEnabled: (cameraEnabled: boolean, microphoneEnabled: boolean) => void;
  /** Tear down media + connection. Idempotent. */
  close: () => void;
  /** Wire status. */
  status: () => CallConnectionStatus;
};

export type CallSessionEvents = {
  onSignal: (signal: WellFormedCallSignal) => Promise<void> | void;
  onStatusChange: (status: CallConnectionStatus) => void;
};

/**
 * Start the real cross-device session for this device.
 *
 * - HOST: acquires media, adds tracks, creates the offer, waits for ICE
 *   gathering, publishes OFFER. Applies the guest ANSWER when it arrives
 *   via applyRemoteSignal.
 * - GUEST: acquires media, waits for the host OFFER via applyRemoteSignal,
 *   answers, waits for gathering, publishes ANSWER.
 */
export async function startRealCallSession(
  role: CallRole,
  mode: CallMode,
  cameraEnabled: boolean,
  microphoneEnabled: boolean,
  events: CallSessionEvents,
): Promise<LiveCallSession> {
  const peerConnection = createMoviePartyPeerConnection();
  if (!peerConnection) {
    throw new CallSessionError("MP-CALL-010", "WebRTC is unavailable in this runtime");
  }

  const media = await acquireRealCallMedia(mode, cameraEnabled, microphoneEnabled);
  const localStream = media.stream;
  const remoteStream = new MediaStream();
  let closed = false;

  /**
   * MP-03: the acquired camera/mic tracks are owned by THIS function until the
   * `LiveCallSession` below is returned to the caller. If startup throws in
   * between, the caller never receives a handle and so could never release the
   * devices — the OS camera indicator would stay lit with no call. The failure
   * paths therefore release them here, through the same implementation the
   * success path's `close()` uses, so ownership is explicit and
   * exception-safe instead of duplicated.
   */
  const releaseAcquiredMedia = () => {
    peerConnection.ontrack = null;
    stopCallStream(localStream);
    try {
      peerConnection.close();
    } catch {
      // close() on an already-closed connection is best-effort teardown.
    }
  };

  const statusOf = (): CallConnectionStatus => {
    if (closed) {
      return "ended";
    }
    switch (peerConnection.connectionState) {
      case "connected":
        return "connected";
      case "failed":
        return "degraded";
      case "disconnected":
        return "reconnecting";
      case "closed":
        return "ended";
      case "new":
      case "connecting":
      default:
        return "connecting";
    }
  };

  const reportStatus = () => {
    events.onStatusChange(statusOf());
  };

  // ── Host-side recovery re-offer wiring ──────────────────────────────────
  // A one-sided peer restart (privacy toggle exit, view remount) tears down
  // the ANSWERER's peer connection while the offerer's session persists.
  // Two detectors cover it: (1) the wire going failed/disconnected past a
  // grace window — the offerer re-offers on the same peer connection;
  // (2) a RENEGOTIATE poke from the restarted answerer when the wire still
  // LOOKS healthy to the offerer (see applySignal's RENEGOTIATE arm).
  // The ledger sees a legal offer→answer pair each time (offer_pending &&
  // answer_seen permits a new offer; the new answer then validates). A
  // backoff keeps a flapping wire from spamming offers.
  let disconnectGraceTimer = 0;
  peerConnection.addEventListener("connectionstatechange", () => {
    reportStatus();
    if (role !== "HOST" || closed) {
      return;
    }
    if (peerConnection.connectionState === "failed") {
      void reofferForDeadWire().catch(() => undefined);
      return;
    }
    if (peerConnection.connectionState === "disconnected") {
      window.clearTimeout(disconnectGraceTimer);
      disconnectGraceTimer = window.setTimeout(() => {
        if (peerConnection.connectionState === "connected") {
          return;
        }
        void reofferForDeadWire().catch(() => undefined);
      }, DISCONNECT_REOFFER_GRACE_MS);
    }
  });
  peerConnection.addEventListener("iceconnectionstatechange", reportStatus);

  peerConnection.ontrack = (event) => {
    for (const stream of event.streams) {
      for (const track of stream.getTracks()) {
        if (!remoteStream.getTracks().some((existing) => existing.id === track.id)) {
          remoteStream.addTrack(track);
        }
      }
    }
    if (
      event.streams.length === 0 &&
      !remoteStream.getTracks().some((existing) => existing.id === event.track.id)
    ) {
      remoteStream.addTrack(event.track);
    }
  };

  // F29: remember the video sender so the camera toggle can swap tracks
  // without renegotiating — a same-kind `replaceTrack` needs no re-offer.
  let videoSender: RTCRtpSender | null = null;
  try {
    for (const track of localStream.getTracks()) {
      const sender = peerConnection.addTrack(track, localStream);
      if (track.kind === "video") {
        videoSender = sender;
      }
    }
  } catch (error) {
    // MP-03: the devices are already open but no session will be returned —
    // release them before propagating the failure.
    closed = true;
    releaseAcquiredMedia();
    throw error;
  }

  // The wire-driven re-offer path (distinct from the RENEGOTIATE poke in
  // applySignal): a one-sided peer restart often surfaces as failed/
  // disconnected ICE before the marker arrives. The backoff guards a
  // flapping wire from spamming offers; recovery from a suppressed
  // re-offer is the RENEGOTIATE poke (which bypasses this backoff).
  let lastReofferAtMs = 0;

  const publishOffer = async (options?: { iceRestart?: boolean }) => {
    const offer = await peerConnection.createOffer(
      options?.iceRestart === true ? { iceRestart: true } : undefined,
    );
    await peerConnection.setLocalDescription(offer);
    await waitForNonTrickleIce(peerConnection, SESSION_ICE_TIMEOUT_MS);
    await events.onSignal({
      signalType: "OFFER",
      data: JSON.stringify(peerConnection.localDescription ?? offer),
    });
  };

  const reofferForDeadWire = async () => {
    if (closed || role !== "HOST" || peerConnection.connectionState === "connected") {
      return;
    }
    const now = Date.now();
    if (now - lastReofferAtMs < REOFFER_BACKOFF_MS) {
      return;
    }
    lastReofferAtMs = now;
    // iceRestart: a plain createOffer reuses the stale candidate pairs
    // that already failed — recovery must re-run ICE to have any effect.
    await publishOffer({ iceRestart: true });
  };

  const applySignal = async (signal: WellFormedCallSignal) => {
    if (closed) {
      return;
    }
    switch (signal.signalType) {
      case "OFFER": {
        if (role !== "GUEST") {
          // Only the guest applies offers (the host is the offerer;
          // honoring an OFFER at the host would be a protocol violation).
          return;
        }
        const offer = parseSessionDescription(signal.data, "OFFER");
        await peerConnection.setRemoteDescription(offer);
        const answer = await peerConnection.createAnswer();
        await peerConnection.setLocalDescription(answer);
        await waitForNonTrickleIce(peerConnection, SESSION_ICE_TIMEOUT_MS);
        await events.onSignal({
          signalType: "ANSWER",
          data: JSON.stringify(peerConnection.localDescription ?? answer),
        });
        return;
      }
      case "ANSWER": {
        if (role !== "HOST") {
          // Only the host applies answers.
          return;
        }
        if (peerConnection.signalingState !== "have-local-offer") {
          // Glare artifact: this ANSWER belongs to an exchange that a
          // previously-applied answer already completed (near-simultaneous
          // session starts can produce offer/answer races). Both answers
          // carry the same guest ICE ufrag (the guest never ICE-restarts),
          // so the applied one has the wire up and this one is redundant.
          // Applying it at stable would throw InvalidStateError; skipping
          // keeps the live exchange intact.
          return;
        }
        const answer = parseSessionDescription(signal.data, "ANSWER");
        await peerConnection.setRemoteDescription(answer);
        return;
      }
      case "ICE": {
        const candidate = parseIceCandidate(signal.data);
        try {
          await peerConnection.addIceCandidate(candidate);
        } catch {
          // Late/duplicate ICE after a renegotiation can legitimately
          // fail; the non-trickle design makes this a no-op, not fatal.
        }
        return;
      }
      case "RENEGOTIATE": {
        // Peer-restart poke: the peer's session was recreated (mode
        // change, privacy exit, view remount) and its answerer needs a
        // fresh exchange. Only meaningful at the offerer (host); the
        // answerer ignores it. Ledger validity is already guaranteed —
        // both ledgers were conditionally reset when the marker was
        // validated (publisher's at submit, receiver's on arrival) — so
        // this is a legal fresh offer. Never suppress the poke: the
        // restarted peer's cursor pinned PAST every earlier offer, so
        // ignoring or backing off would deadlock the recovery. Backoff is
        // reserved for the wire-driven path (reofferForDeadWire).
        if (role !== "HOST") {
          return;
        }
        await publishOffer();
        return;
      }
      default:
        return;
    }
  };

  // ── Session identity ────────────────────────────────────────────────────
  // Generated BEFORE the session object so both the kickoff marker and the
  // nextPendingSignals own-marker drop can reference it (see
  // generateMarkerId for the identity scheme). Both sides publish the
  // marker at kickoff; only the PEER's marker ever gets applied.
  const markerId = generateMarkerId();

  // Serialize all remote-signal application: two cursor-effect batches can
  // arrive back-to-back (marker poke lands, then the OFFER it triggered in
  // a later snapshot), and interleaved setRemoteDescription/createAnswer
  // calls would race inside the peer connection. The queue also delays
  // application past close() — a queued signal for a closed session is
  // dropped, not applied to a dead connection.
  let applyQueueTail: Promise<void> = Promise.resolve();
  const applyRemoteSignal = (signal: WellFormedCallSignal): Promise<void> => {
    const run = async () => {
      if (closed) {
        return;
      }
      await applySignal(signal);
    };
    applyQueueTail = applyQueueTail.then(run, run);
    return applyQueueTail;
  };

  /**
   * F29: the user's camera toggle is a privacy control, so turning it off must
   * RELEASE the capture device rather than merely mute a live track.
   * `track.enabled = false` keeps the device open, so the OS camera indicator
   * stays lit while the UI reports the camera as off — the two disagreed.
   *
   * Turning it back on re-acquires the video device and swaps it through the
   * existing sender (a same-kind `replaceTrack` needs no renegotiation). If the
   * device is unavailable or permission is refused, the camera stays off rather
   * than silently claiming otherwise.
   *
   * The degradation ladder (`applyCameraTier`) deliberately keeps its
   * documented freeze semantics: that path is a temporary quality step, not a
   * user privacy choice, so it must not hard-end the track.
   */
  const applyLocalCameraToggle = async (enabled: boolean): Promise<void> => {
    if (closed) {
      return;
    }
    const liveTracks = localStream
      .getVideoTracks()
      .filter((track) => track.readyState === "live");

    switch (cameraToggleAction(liveTracks.length > 0, enabled)) {
      case "enable-existing":
        for (const track of liveTracks) {
          track.enabled = true;
        }
        return;

      case "acquire":
        try {
          const devices = typeof navigator === "undefined" ? undefined : navigator.mediaDevices;
          if (!devices?.getUserMedia) {
            return;
          }
          const fresh = await devices.getUserMedia({ video: true });
          const freshVideo = fresh.getVideoTracks();
          const track = freshVideo[0];
          // `track === undefined` covers a device that yielded no video; the
          // connection check covers the call ending while the permission prompt
          // was still open. Either way, release instead of leaking a camera.
          if (track === undefined || peerConnection.connectionState === "closed") {
            for (const opened of fresh.getTracks()) {
              opened.stop();
            }
            return;
          }
          localStream.addTrack(track);
          if (videoSender) {
            await videoSender.replaceTrack(track);
          } else {
            videoSender = peerConnection.addTrack(track, localStream);
          }
        } catch {
          // Denied or unavailable — the camera stays off, honestly.
        }
        return;

      case "release":
        // Off: release the device so the OS indicator goes out.
        for (const track of localStream.getVideoTracks()) {
          track.stop();
          localStream.removeTrack(track);
        }
        if (videoSender) {
          try {
            await videoSender.replaceTrack(null);
          } catch {
            // A torn-down connection has nothing to detach from.
          }
        }
        return;
    }
  };

  /**
   * Serializes camera toggles (F29).
   *
   * The toggle awaits `getUserMedia`, and the caller's effect re-runs on the
   * MICROPHONE toggle too. Without serialization, a mic toggle landing while a
   * camera acquire was in flight would find no live track yet, start a SECOND
   * acquire, and add a duplicate video track. Chaining preserves the user's
   * ordering — the last toggle issued is the one that wins.
   */
  let cameraToggleChain: Promise<void> = Promise.resolve();

  const applyLocalCameraEnabled = (enabled: boolean): Promise<void> => {
    cameraToggleChain = cameraToggleChain
      .then(() => applyLocalCameraToggle(enabled))
      .catch(() => undefined);
    return cameraToggleChain;
  };

  const session: LiveCallSession = {
    role,
    markerId,
    localStream,
    remoteStream,
    peerConnection,
    usedRealMedia: media.usedRealMedia,
    mediaErrorCode: media.errorCode,
    applyRemoteSignal,
    setLocalTracksEnabled(cameraEnabled: boolean, microphoneEnabled: boolean) {
      for (const track of localStream.getAudioTracks()) {
        track.enabled = microphoneEnabled;
      }
      // F29: the camera half is async (it may have to release or re-acquire the
      // device). Audio stays a synchronous enabled flip.
      void applyLocalCameraEnabled(cameraEnabled);
    },
    async applyCameraTier(camera: CameraTierState) {
      if (closed) {
        return false;
      }
      const videoTracks = localStream.getVideoTracks();
      if (videoTracks.length === 0) {
        return false;
      }
      // Freeze semantics for a disabled tier (PRD §41 "freeze" step):
      // the track keeps flowing frozen frames rather than a hard end,
      // matching the ladder's temporary-off intent.
      for (const track of videoTracks) {
        track.enabled = camera.enabled;
      }
      if (!camera.enabled) {
        return true;
      }
      // Sender caps: encoder-level bitrate/framerate, no renegotiation.
      const sender = peerConnection
        .getSenders()
        .find((candidate) => candidate.track?.kind === "video");
      if (sender) {
        const parameters = sender.getParameters();
        const next = senderParametersForTier(camera, parameters);
        try {
          await sender.setParameters(next);
        } catch {
          // Non-fatal: some stacks reject dynamic maxFramerate; the
          // resolution constraint below still downgrades the feed.
        }
      }
      // Track constraints: downscale the captured resolution.
      for (const track of videoTracks) {
        try {
          await track.applyConstraints(trackConstraintsForTier(camera));
        } catch {
          // Non-fatal: the sender caps above still bound the encoder.
        }
      }
      return true;
    },
    close() {
      if (closed) {
        return;
      }
      closed = true;
      window.clearTimeout(disconnectGraceTimer);
      releaseAcquiredMedia();
      events.onStatusChange("ended");
    },
    status: statusOf,
  };

  // ── Kickoff ─────────────────────────────────────────────────────────────
  // BOTH sides publish the RENEGOTIATE marker with a session-unique id,
  // then the host offers. The marker resets both ledgers (sender's at
  // submit, receiver's on arrival — conditional on a never-completed
  // exchange), which (a) unblocks this side's fresh exchange regardless
  // of what the previous session left pending (host remount after an
  // unanswered offer would otherwise trip MP-CALL-002 on its own submit
  // ledger), and (b) pokes a still-live peer whose wire LOOKS connected,
  // so its state-driven re-offer cannot fire on its own. Self-echo is
  // handled by identity: submit_call_signal appends every locally-
  // published signal back into the local snapshot, so each side later
  // sees its own marker — the host drops it by markerId (see
  // nextPendingSignals), and the guest drops all markers by role (its
  // RENEGOTIATE arm never runs).
  try {
    await events.onSignal({
      signalType: "RENEGOTIATE",
      data: JSON.stringify({ request: "renegotiate", v: 1, id: markerId }),
    });
    if (role === "HOST") {
      await publishOffer();
    }
  } catch (error) {
    // MP-03: startup failed after the camera/mic were already acquired. The
    // caller has no session handle, so nothing downstream can release the
    // devices — release them here, then rethrow so the caller still sees the
    // real failure (and can retry with the devices free).
    closed = true;
    window.clearTimeout(disconnectGraceTimer);
    releaseAcquiredMedia();
    throw error;
  }

  reportStatus();
  return session;
}

export class CallSessionError extends Error {
  readonly errorCode: string;

  constructor(errorCode: string, message: string) {
    super(message);
    this.errorCode = errorCode;
    this.name = "CallSessionError";
  }
}

const SESSION_ICE_TIMEOUT_MS = 5_000;
/** Minimum spacing between host recovery re-offers. */
const REOFFER_BACKOFF_MS = 4_000;
/** disconnected→re-offer grace; brief blips must not renegotiate. */
const DISCONNECT_REOFFER_GRACE_MS = 2_500;

/**
 * Session-unique marker id (monotonic counter + random suffix — unique
 * across devices without coordination). Identifies which session a
 * RENEGOTIATE marker came from so each side can drop its OWN self-
 * echoed marker while honoring the peer's.
 */
function generateMarkerId(): string {
  markerIdCounter += 1;
  return `s${String(markerIdCounter)}-${Math.random().toString(36).slice(2, 8)}`;
}
let markerIdCounter = 0;

function parseSessionDescription(data: string, expectedType: string): RTCSessionDescription {
  let parsed: unknown;
  try {
    parsed = JSON.parse(data);
  } catch {
    throw new CallSessionError("MP-CALL-001", `malformed ${expectedType} payload`);
  }
  if (typeof parsed !== "object" || parsed === null) {
    throw new CallSessionError("MP-CALL-001", `malformed ${expectedType} payload`);
  }
  const type = (parsed as { type?: unknown }).type;
  const sdp = (parsed as { sdp?: unknown }).sdp;
  if (
    !isSdpType(type) ||
    (expectedType === "OFFER" && type !== "offer") ||
    (expectedType === "ANSWER" && type !== "answer") ||
    typeof sdp !== "string" ||
    sdp.length === 0
  ) {
    throw new CallSessionError("MP-CALL-001", `malformed ${expectedType} payload`);
  }
  return new RTCSessionDescription({ type, sdp });
}

function isSdpType(value: unknown): value is RTCSdpType {
  return value === "offer" || value === "answer" || value === "pranswer" || value === "rollback";
}

function parseIceCandidate(data: string): RTCIceCandidateInit {
  let parsed: unknown;
  try {
    parsed = JSON.parse(data);
  } catch {
    throw new CallSessionError("MP-CALL-005", "malformed ICE payload");
  }
  if (typeof parsed !== "object" || parsed === null) {
    throw new CallSessionError("MP-CALL-005", "malformed ICE payload");
  }
  const candidate = (parsed as { candidate?: unknown }).candidate;
  if (typeof candidate !== "string" && typeof candidate !== "undefined") {
    throw new CallSessionError("MP-CALL-005", "malformed ICE payload");
  }
  return parsed;
}

async function waitForNonTrickleIce(
  peerConnection: RTCPeerConnection,
  timeoutMs: number,
): Promise<void> {
  if (peerConnection.iceGatheringState === "complete") {
    return;
  }
  await new Promise<void>((resolve) => {
    let settled = false;
    const finish = () => {
      if (settled) {
        return;
      }
      settled = true;
      peerConnection.removeEventListener("icegatheringstatechange", check);
      window.clearTimeout(timer);
      resolve();
    };
    const check = () => {
      if (peerConnection.iceGatheringState === "complete") {
        finish();
      }
    };
    const timer = window.setTimeout(finish, timeoutMs);
    peerConnection.addEventListener("icegatheringstatechange", check, { once: false });
  });
}
