import { describe, expect, it } from "vitest";
import { buildCallMediaIntent, cameraToggleAction } from "./webrtc";

describe("buildCallMediaIntent", () => {
  it("requests both track kinds for VIDEO_VOICE (mode drives acquisition, not the enabled state)", () => {
    // PRD §41: toggles are local track.enabled flips. A camera enabled
    // mid-session must already have a track to enable — re-acquiring
    // would need a renegotiation — so VIDEO_VOICE acquires video even
    // when the camera starts disabled.
    const intent = buildCallMediaIntent("VIDEO_VOICE");

    expect(intent.audio).toBe(true);
    expect(intent.video).toEqual({
      width: { ideal: 640, max: 640 },
      height: { ideal: 360, max: 360 },
      frameRate: { ideal: 15, max: 20 },
    });
  });

  it("supports voice only and off modes", () => {
    expect(buildCallMediaIntent("VOICE_ONLY")).toEqual({
      audio: true,
      video: false,
    });
    expect(buildCallMediaIntent("OFF")).toEqual({
      audio: false,
      video: false,
    });
  });
});

describe("F29 — the camera toggle must change hardware state, not just mute", () => {
  it("RELEASES the device when the camera is turned off", () => {
    // `track.enabled = false` keeps the device open, so the OS camera
    // indicator stays lit while the UI claims the camera is off.
    expect(cameraToggleAction(true, false)).toBe("release");
    expect(cameraToggleAction(false, false)).toBe("release");
  });

  it("re-acquires when the camera is turned on with no live track", () => {
    expect(cameraToggleAction(false, true)).toBe("acquire");
  });

  it("only re-enables a track that is still live", () => {
    expect(cameraToggleAction(true, true)).toBe("enable-existing");
  });

  it("never reports an 'off' toggle as a mere mute", () => {
    // The regression: off used to resolve to a track.enabled flip.
    expect(cameraToggleAction(true, false)).not.toBe("enable-existing");
  });
});
