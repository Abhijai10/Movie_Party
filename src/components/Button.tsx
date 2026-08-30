import type { ButtonHTMLAttributes, ReactNode } from "react";

type ButtonVariant = "primary" | "secondary" | "danger" | "ghost";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  children: ReactNode;
  variant?: ButtonVariant;
  isLoading?: boolean;
};

export function Button({
  children,
  className,
  variant = "secondary",
  isLoading = false,
  disabled,
  ...props
}: ButtonProps) {
  const classes = ["mp-button", `mp-button-${variant}`, className].filter(Boolean).join(" ");

  return (
    <button className={classes} type="button" disabled={disabled || isLoading} {...props}>
      {isLoading ? "Working..." : children}
    </button>
  );
}
