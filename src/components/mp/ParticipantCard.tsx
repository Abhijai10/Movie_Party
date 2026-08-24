import { StatusIndicator } from "./StatusIndicator";

type ParticipantCardProps = {
  name?: string;
  role: string;
  ready?: boolean;
  isYou?: boolean;
  testId?: string;
};

export function ParticipantCard({
  name,
  role,
  ready = false,
  isYou = false,
  testId,
}: ParticipantCardProps) {
  const initials =
    name
      ?.split(" ")
      .map((s) => s[0])
      .slice(0, 2)
      .join("")
      .toUpperCase() ?? "";

  return (
    <div
      className="relative flex items-center gap-5 p-5 rounded-2xl overflow-hidden"
      style={{
        background: "linear-gradient(135deg, rgba(20,15,32,0.9) 0%, rgba(11,7,20,0.9) 100%)",
        border: "1px solid rgba(159,122,234,0.15)",
        boxShadow: "inset 0 1px 0 rgba(255,255,255,0.04)",
      }}
      data-testid={testId}
    >
      {ready && (
        <div
          className="absolute -left-8 -top-8 w-32 h-32 rounded-full pointer-events-none"
          style={{
            background: "radial-gradient(circle, rgba(52,211,153,0.28) 0%, transparent 70%)",
            filter: "blur(14px)",
          }}
        />
      )}

      <div
        className="relative w-14 h-14 rounded-full flex items-center justify-center font-serif-display text-xl text-white shrink-0"
        style={{
          background: "linear-gradient(135deg, #4A2B85, #201142)",
          border: "1px solid rgba(159,122,234,0.4)",
          boxShadow: "0 6px 24px -8px rgba(107,70,193,0.6)",
        }}
      >
        {initials || "?"}
      </div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-white text-[15px] font-medium truncate">
            {name || "Waiting..."}
          </span>
          {isYou && (
            <span className="text-[9px] tracking-[0.22em] uppercase text-white/50 px-1.5 py-0.5 rounded-full bg-white/5">
              You
            </span>
          )}
        </div>
        <div className="text-[11px] tracking-[0.22em] uppercase text-white/45 mt-1">{role}</div>
      </div>

      <StatusIndicator state={ready ? "ready" : "waiting"} />
    </div>
  );
}

export default ParticipantCard;
