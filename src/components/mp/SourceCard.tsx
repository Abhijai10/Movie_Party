import { motion } from "framer-motion";

type SourceCardProps = {
  icon: React.ComponentType<{ className?: string; strokeWidth?: number }>;
  title: string;
  description: string;
  active: boolean;
  onClick: () => void;
  testId?: string;
};

export function SourceCard({
  icon: Icon,
  title,
  description,
  active,
  onClick,
  testId,
}: SourceCardProps) {
  return (
    <motion.button
      type="button"
      onClick={onClick}
      whileHover={{ y: -4 }}
      whileTap={{ scale: 0.99 }}
      transition={{ type: "spring", stiffness: 300, damping: 24 }}
      data-testid={testId}
      className={`group relative text-left rounded-2xl p-6 h-[210px] flex flex-col justify-between overflow-hidden transition-colors ${
        active
          ? "border border-[#9F7AEA]/60 bg-[#150F24]"
          : "border border-white/8 bg-[#0D0B14]/70 hover:border-[#9F7AEA]/40"
      }`}
      style={{
        backdropFilter: "blur(14px)",
        boxShadow: active
          ? "0 20px 60px -20px rgba(159,122,234,0.5), inset 0 1px 0 rgba(255,255,255,0.05)"
          : "inset 0 1px 0 rgba(255,255,255,0.04)",
      }}
    >
      <div
        className="absolute -top-16 -right-16 w-40 h-40 rounded-full opacity-0 group-hover:opacity-100 transition-opacity duration-500 pointer-events-none"
        style={{
          background: "radial-gradient(circle, rgba(159,122,234,0.45) 0%, transparent 70%)",
          filter: "blur(20px)",
        }}
      />

      <div
        className="relative w-12 h-12 rounded-xl flex items-center justify-center"
        style={{
          background: active
            ? "linear-gradient(135deg, #6B46C1, #3D2168)"
            : "linear-gradient(135deg, rgba(107,70,193,0.25), rgba(61,33,104,0.15))",
          border: "1px solid rgba(159,122,234,0.25)",
        }}
      >
        <Icon className="w-5 h-5 text-white" strokeWidth={1.6} />
      </div>

      <div>
        <h3 className="font-serif-display text-2xl text-white leading-tight">{title}</h3>
        <p className="mt-2 text-white/55 text-sm leading-relaxed">{description}</p>
      </div>

      {active && (
        <div className="absolute top-4 right-4 w-2 h-2 rounded-full bg-[#34D399] status-dot" />
      )}
    </motion.button>
  );
}

export default SourceCard;
