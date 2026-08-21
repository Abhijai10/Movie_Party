import type { ReactNode } from "react";

type GlassPanelProps = {
  children: ReactNode;
  className?: string;
  labelledBy?: string;
};

export function GlassPanel({ children, className = "setup-panel", labelledBy }: GlassPanelProps) {
  return (
    <section className={className} aria-labelledby={labelledBy}>
      {children}
    </section>
  );
}
