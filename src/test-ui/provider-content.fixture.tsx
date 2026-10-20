// Test-only IPC fixture. Never imported by the production entry point.
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { useState } from "react";
import { CreatePartyView } from "../views/CreatePartyView";
import { type ProviderCapability } from "../backend/appRuntime";
import type { ContentChooserSnapshot, ContentTab } from "../providers/contentChooser";
import "../styles.css";

const capability: ProviderCapability = {
  id: "youtube",
  displayName: "YouTube",
  supportLevel: "SUPPORTED",
  titleResolution: "DIRECT_URL",
  syncAvailable: true,
  sharedAvailable: false,
  sharedReason: "Off",
  verification: "EXTERNAL_VERIFICATION_PENDING",
};
const tabs: ContentTab[] = ["first", "second"].map((targetId, index) => ({
  targetId,
  title: `Fixture video ${String(index + 1)}`,
  identity: {
    providerId: "youtube",
    contentId: index ? "abcdefghijk" : "dQw4w9WgXcQ",
    allowedUrl: `https://www.youtube.com/watch?v=${index ? "abcdefghijk" : "dQw4w9WgXcQ"}`,
    playlistId: null,
  },
  thumbnailUrl: null,
  documentId: "fixture-document",
  focused: index === 1,
  visible: index === 1,
  playbackReady: true,
  issue: null,
  selectionKey: targetId,
}));
mockWindows("main");
let state: ContentChooserSnapshot = {
  browserReady: false,
  tabs: [],
  suggestedTargetId: null,
  selected: null,
  confirmedKey: null,
  contentReady: false,
  issue: null,
};
let fail = false;
let calls = 0;
let releaseCreation: (() => void) | null = null;
mockIPC((command, payload) => {
  if (command === "get_provider_capabilities") return [capability];
  if (command === "get_provider_content") {
    calls++;
    if (fail) throw new Error("Fixture: managed browser closed. Reopen it.");
    return state;
  }
  if (command === "browse_provider_content") {
    if (payload && "input" in payload && String(payload.input).startsWith("javascript:"))
      throw new Error("Only provider HTTPS title or video URLs are supported.");
    return state;
  }
  if (command === "confirm_provider_content") {
    if (!payload || !("targetId" in payload) || !("selectionKey" in payload))
      throw new Error("Missing confirmation");
    const selected = state.tabs.find(
      (t) => t.targetId === payload.targetId && t.selectionKey === payload.selectionKey,
    );
    if (!selected) throw new Error("Fixture: changed tab");
    state = {
      ...state,
      selected,
      confirmedKey: selected.selectionKey,
      contentReady: selected.playbackReady,
    };
    return state;
  }
  throw new Error(`Unexpected fixture IPC: ${command}`);
});
function Fixture() {
  const [creating, setCreating] = useState(false);
  const [result, setResult] = useState("No party created");
  const capabilities = [capability];
  return (
    <>
      <aside
        style={{
          padding: 12,
          background: "#181322",
          color: "white",
          position: "fixed",
          zIndex: 100,
          bottom: 0,
          right: 0,
          maxWidth: 440,
        }}
      >
        <strong>Isolated mocked IPC fixture</strong>
        <p>{result}</p>
        <button
          onClick={() => {
            fail = true;
          }}
        >
          Simulate browser crash
        </button>{" "}
        <button
          onClick={() => {
            state = {
              ...state,
              tabs: [],
              selected: null,
              confirmedKey: null,
              contentReady: false,
              issue: "Selected tab closed. Select again.",
            };
          }}
        >
          Close selected tab
        </button>{" "}
        <button
          onClick={() => {
            releaseCreation?.();
          }}
        >
          Finish creation
        </button>
      </aside>
      <CreatePartyView
        isCreating={creating}
        error={null}
        providerCapabilities={capabilities}
        provider={{ providerId: "youtube", url: null, state: "", readiness: "NOT_STARTED" }}
        onBack={() => {
          setResult("Back requested");
        }}
        onOpenProvider={() => {
          fail = false;
          state = { ...state, browserReady: true, tabs, suggestedTargetId: "second" };
          return Promise.resolve(true);
        }}
        onCheckProviderStatus={() => Promise.resolve(true)}
        onNavigateProviderTitle={() => Promise.resolve(true)}
        onCreateParty={(request) => {
          setCreating(true);
          const before = calls;
          setResult(`Creating ${request.source}; no overlapping checks expected`);
          return new Promise((resolve) => {
            releaseCreation = () => {
              setResult(
                `Created ${request.source}; checks while creating: ${String(calls - before)}`,
              );
              setCreating(false);
              resolve(true);
            };
          });
        }}
      />
    </>
  );
}
const root = document.getElementById("root");
if (!root) throw new Error("Missing fixture root");
createRoot(root).render(<Fixture />);
