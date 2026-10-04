import type { AppMetadataInfo } from "../backend/appRuntime";

/**
 * The Settings → Diagnostics metadata rows, as a pure mapping the UI can be
 * tested on.
 *
 * ## The defect this exists to prevent
 *
 * The "App version" row rendered `info.appName`, because `app_metadata()`
 * carried no version field and the view reached for the nearest string. The row
 * therefore showed the application *name* ("Movie Party") under the label
 * "App version" — which silently invalidated every manual beta step that asked a
 * tester to "confirm the version shown in Settings".
 *
 * ## Why this is a pure function
 *
 * The repo has no DOM test environment (`vitest` runs in `node`; there is no
 * `@testing-library/react` or `jsdom`), so a component cannot be rendered in a
 * test. Every UI decision of this kind is extracted into a pure function and
 * tested directly — see `cinemaEndState.ts`, `reconnectLatchAfter`,
 * `cinemaDockInteractionClass`. This follows that pattern.
 */

/** What the Settings rows show before the backend answers. */
export type MetadataDisplay = {
  appVersion: string;
  protocol: string;
};

/** The placeholder shown until (or unless) the backend answers. */
export const UNKNOWN_METADATA: MetadataDisplay = {
  appVersion: "—",
  protocol: "—",
};

/**
 * Map the backend's app metadata onto the two Settings rows.
 *
 * `appVersion` is taken from the metadata's own version field — never from
 * `appName`. A null answer (the command failed) falls back to the placeholder
 * rather than inventing a version.
 */
export function metadataDisplay(info: AppMetadataInfo | null): MetadataDisplay {
  if (info == null) {
    return UNKNOWN_METADATA;
  }
  return {
    appVersion: info.appVersion,
    protocol: `V${String(info.protocolMajor)}.${String(info.protocolMinor)}`,
  };
}
