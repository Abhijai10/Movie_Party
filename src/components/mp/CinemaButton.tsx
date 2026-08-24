import { forwardRef, type ReactNode } from "react";
import { motion, type HTMLMotionProps } from "framer-motion";
import type { LucideIcon } from "lucide-react";

type CinemaButtonProps = Omit<HTMLMotionProps<"button">, "ref"> & {
  children: ReactNode;
  variant?: "primary" | "ghost" | "neutral";
  className?: string;
  icon?: LucideIcon;
  iconPos?: "left" | "right";
};

export const CinemaButton = forwardRef<HTMLButtonElement, CinemaButtonProps>(function CinemaButton(
  { children, variant = "primary", className = "", icon: Icon, iconPos = "right", ...props },
  ref,
) {
  const base =
    "inline-flex h-14 max-w-full shrink-0 items-center justify-center gap-3 rounded-full px-8 sm:px-10 text-sm tracking-[0.16em] uppercase font-medium relative select-none whitespace-nowrap overflow-hidden disabled:opacity-40 disabled:cursor-not-allowed";
  const styles =
    variant === "primary"
      ? "cinema-btn-primary text-white"
      : variant === "ghost"
        ? "cinema-btn-ghost text-white/90"
        : "bg-white/5 border border-white/10 text-white/90 hover:bg-white/10 transition";

  return (
    <motion.button
      ref={ref}
      whileTap={{ scale: 0.97 }}
      className={`${base} ${styles} ${className}`}
      type="button"
      {...props}
    >
      {Icon && iconPos === "left" && <Icon className="w-4 h-4 shrink-0" strokeWidth={1.6} />}
      <span className="min-w-0 truncate">{children}</span>
      {Icon && iconPos === "right" && <Icon className="w-4 h-4 shrink-0" strokeWidth={1.6} />}
    </motion.button>
  );
});

export default CinemaButton;
