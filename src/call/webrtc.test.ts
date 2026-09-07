import { describe, expect, it } from "vitest";
import { buildCallMediaIntent } from "./webrtc";

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
