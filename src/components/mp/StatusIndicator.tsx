type StatusIndicatorProps = {
  state?: "ready" | "waiting" | "live" | "sync" | "idle";
  label?: string;
  className?: string;
  showLabel?: boolean;
};

const map: Record<string, { color: string; pulse: boolean; label: string }> = {
  ready: { color: "#34D399", pulse: false, label: "Ready" },
  waiting: { color: "#9F7AEA", pulse: true, label: "Waiting" },
  live: { color: "#EF4444", pulse: true, label: "Live" },
  sync: { color: "#34D399", pulse: false, label: "In sync" },
  idle: { color: "#686475", pulse: false, label: "Idle" },
};

export function StatusIndicator({
  state = "ready",
  label,
  className = "",
  showLabel = true,
}: StatusIndicatorProps) {
  const idleCfg = { color: "#686475", pulse: false, label: "Idle" };
  const cfg = map[state] ?? idleCfg;
  return (
    <div
      className={`inline-flex items-center gap-2.5 ${className}`}
      data-testid={`status-${state}`}
    >
      <span
        className={`status-dot inline-block w-2 h-2 rounded-full ${cfg.pulse ? "animate-pulse" : ""}`}
        style={{ background: cfg.color, color: cfg.color }}
      />
      {showLabel && (
        <span className="text-[11px] tracking-[0.22em] uppercase text-white/70">
          {label ?? cfg.label}
        </span>
      )}
    </div>
  );
}

export default StatusIndicator;
