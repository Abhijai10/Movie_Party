import { describe, expect, it } from "vitest";
import { metadataDisplay, UNKNOWN_METADATA } from "./settingsMetadata";
import type { AppMetadataInfo } from "../backend/appRuntime";

/**
 * Regression guard for the Settings "App version" defect: the row used to
 * render `info.appName`, so it displayed the application *name* under a label
 * that promises a version. These tests fail if that mapping ever comes back.
 */

const REAL: AppMetadataInfo = {
  appName: "Movie Party",
  appVersion: "0.9.9",
  protocolMajor: 1,
  protocolMinor: 0,
};

describe("settings metadata rows", () => {
  it("shows the application version, not the application name", () => {
    const display = metadataDisplay(REAL);

    expect(display.appVersion).toBe("0.9.9");
    expect(display.appVersion).not.toBe(REAL.appName);
  });

  it("formats the protocol version from its two numeric parts", () => {
    expect(metadataDisplay(REAL).protocol).toBe("V1.0");
    expect(
      metadataDisplay({ ...REAL, protocolMajor: 2, protocolMinor: 3 }).protocol,
    ).toBe("V2.3");
  });

  it("falls back to the placeholder when the backend did not answer", () => {
    expect(metadataDisplay(null)).toEqual(UNKNOWN_METADATA);
    expect(metadataDisplay(null).appVersion).toBe("—");
  });

  it("never renders an empty version row", () => {
    expect(metadataDisplay(REAL).appVersion.length).toBeGreaterThan(0);
    expect(metadataDisplay(null).appVersion.length).toBeGreaterThan(0);
  });
});
