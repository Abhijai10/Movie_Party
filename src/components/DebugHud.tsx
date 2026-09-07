import type { AppSnapshot } from "../backend/appRuntime";

/**
 * UI_UX_SPEC §63 — Debug HUD. NOT visible to normal users: dev-gated via
 * the `?debug` query (localhost dev preview) — never a user setting. Shows
 * live sync internals so a developer can see drift/buffer/camera tier
 * without a debugger.
 */
type DebugHudProps = {
  snapshot: AppSnapshot;
};

function ms(value: number | null | undefined): string {
  if (value == null) return "—";
  return `${String(Math.round(value))} ms`;
}

function formatClock(valueMs: number): string {
  if (!Number.isFinite(valueMs) || valueMs <= 0) return "00:00:00";
  const totalSeconds = Math.floor(valueMs / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const frac = Math.floor(valueMs % 1000);
  const pad = (n: number, width = 2) => String(n).padStart(width, "0");
  return `${pad(hours)}:${pad(minutes)}:${pad(seconds)}.${pad(frac, 3)}`;
}

export function DebugHud({ snapshot }: DebugHudProps) {
  const rows: Array<[string, string]> = [
    ["Room", snapshot.sync.roomState],
    ["Media", snapshot.media != null ? "LOCAL" : snapshot.provider.providerId ?? "—"],
    ["Position", formatClock(snapshot.sync.positionMs)],
    ["RTT", ms(snapshot.network.rttMs)],
    ["Path", snapshot.network.path],
    ["Goodput", `${(snapshot.network.goodputBps / 1_000_000).toFixed(1)} Mbps`],
    [
      "Guest Buf",
      `${String(Math.round(snapshot.buffer.guestBufferAheadMs / 1000))} sec`,
    ],
    [
      "Camera",
      `${String(snapshot.call.camera.width)}p${String(snapshot.call.camera.fps)} / ${String(
        Math.round(snapshot.call.camera.targetBitrateBps / 1000),
      )} kbps`,
    ],
    ["Call", snapshot.call.status],
    ["Strict", snapshot.sync.strictSyncPaused ? "PAUSED" : "active"],
  ];

  return (
    <aside
      className="fixed bottom-4 right-4 z-[70] rounded-lg bg-black/80 border border-white/15 p-3 font-mono-mp text-[11px] leading-relaxed text-emerald-200/90 pointer-events-none"
      data-testid="debug-hud"
      aria-hidden="true"
    >
      {rows.map(([label, value]) => (
        <div key={label} className="flex gap-3 justify-between min-w-[220px]">
          <span className="text-white/50">{label.padEnd(10, " ")}</span>
          <span>{value}</span>
        </div>
      ))}
    </aside>
  );
}
