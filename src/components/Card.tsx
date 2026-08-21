import type { ReactNode } from "react";

type CardProps = {
  children: ReactNode;
  className?: string;
};

export function Card({ children, className }: CardProps) {
  return <article className={["mp-card", className].filter(Boolean).join(" ")}>{children}</article>;
}
