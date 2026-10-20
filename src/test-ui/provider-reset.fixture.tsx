// Isolated manual browser fixture. Uses Tauri's IPC mock: no real profiles,
// authentication, browser launch or deletion can be reached from this page.
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { ProviderSessionReset } from "../views/ProviderSessionReset";
import "../styles.css";

let complete: (() => void) | undefined;
let fail: (() => void) | undefined;
let calls = 0;
mockIPC((command, payload) => {
  if (
    command !== "reset_provider_session" ||
    payload == null ||
    !("providerId" in payload) ||
    !("confirmed" in payload) ||
    payload.providerId !== "youtube" ||
    payload.confirmed !== true
  ) {
    throw new Error("Unexpected test IPC request");
  }
  const counter = document.getElementById("calls");
  if (!counter) throw new Error("Test counter is missing");
  counter.textContent = `Reset requests: ${String(++calls)}`;
  return new Promise((resolve, reject) => {
    complete = () => {
      resolve({ screen: "SETTINGS" });
    };
    fail = () => {
      reject(new Error("MP-PROVIDER-005 managed profile is busy"));
    };
  });
});
const root = document.getElementById("root");
if (!root) throw new Error("Test root is missing");
createRoot(root).render(
  <main className="min-h-screen bg-[#0a0a0a] p-8 text-white">
    <h1 className="text-xl">Provider reset — isolated UI test</h1>
    <p>Mocked IPC only. No real profile data is connected.</p>
    <p id="calls">Reset requests: 0</p>
    <div className="my-6 max-w-xl rounded-xl border border-white/10 p-5">
      <h2>YouTube</h2>
      <ProviderSessionReset providerId="youtube" name="YouTube" />
    </div>
    <button type="button" onClick={() => complete?.()}>
      Complete mocked reset
    </button>
    <button type="button" className="ml-4" onClick={() => fail?.()}>
      Fail mocked reset
    </button>
  </main>,
);
