import type { CompanionStatus } from "../backend/appRuntime";

/**
 * The companion is the one provider path that touches a browser the user also
 * uses for everything else, so the UI has to say what is on, what is off and why
 * — and never imply the extension is installed when it is not.
 */

export type CompanionStep = "load" | "start" | "pair" | "chooseTab" | "done";

export const CAPABILITY_LABELS: Record<string, string> = {
  identify_tab: "Recognise the provider tab you chose",
  report_content: "Report the title you selected",
  report_playback: "Report the playback position",
  request_navigation: "Open a title you approve",
  control_playback: "Play, pause or seek from Movie Party",
};

export function companionStep(status: CompanionStatus | null): CompanionStep {
  if (!status) return "load";
  if (!status.listening) return "start";
  if (!status.paired) return "pair";
  return "chooseTab";
}

export function companionHeadline(status: CompanionStatus | null): string {
  switch (companionStep(status)) {
    case "load":
      return "Checking the companion…";
    case "start":
      return "Turn the companion on to pair a browser.";
    case "pair":
      return "Pair your browser with this code.";
    case "chooseTab":
      return "Paired. Open the extension and choose a provider tab.";
    case "done":
      return "The companion is ready.";
  }
}

export function companionSubline(status: CompanionStatus | null): string {
  if (!status?.listening) {
    return "Nothing runs until you switch this on. Managed Chrome stays the default provider path.";
  }
  if (!status.paired) {
    return "The code expires after five attempts and is replaced when you unlink the browser.";
  }
  return "Movie Party reads only the tab you choose, and only on a provider page.";
}

/** Refused capabilities are shown with their reason instead of hidden. */
export function companionCapabilityLines(status: CompanionStatus): string[] {
  const on = status.granted.map((name) => `${CAPABILITY_LABELS[name] ?? name}: allowed`);
  const off = status.refused.map(
    (record) =>
      `${CAPABILITY_LABELS[record.capability] ?? record.capability}: off — ${record.reason}`,
  );
  return [...on, ...off];
}

/**
 * The pairing code is only ever produced by Movie Party, and only while the
 * listener is running, so the UI must not offer a stale code as if it worked.
 */
export function usablePairingCode(status: CompanionStatus): string | null {
  return status.listening ? status.pairingCode : null;
}
