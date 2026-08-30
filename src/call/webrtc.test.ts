import { describe, expect, it } from "vitest";
import { buildCallMediaIntent } from "./webrtc";

describe("buildCallMediaIntent", () => {
  it("keeps an audio track requested while the microphone starts muted", () => {
    const intent = buildCallMediaIntent("VIDEO_VOICE", true);

    expect(intent.audio).toBe(true);
    expect(intent.video).toEqual({
      width: { ideal: 640, max: 640 },
      height: { ideal: 360, max: 360 },
      frameRate: { ideal: 15, max: 20 },
    });
  });

  it("supports voice only and off modes", () => {
    expect(buildCallMediaIntent("VOICE_ONLY", true)).toEqual({
      audio: true,
      video: false,
    });
    expect(buildCallMediaIntent("OFF", true)).toEqual({
      audio: false,
      video: false,
    });
  });
});
