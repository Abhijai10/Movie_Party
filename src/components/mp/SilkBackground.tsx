type SilkBackgroundProps = {
  variant?: "default" | "dim" | "calm" | "warm";
};

export function SilkBackground({ variant = "default" }: SilkBackgroundProps) {
  return (
    <div className={`silk-bg ${variant}`} data-testid="silk-background" aria-hidden="true">
      <div className="silk-grain" />
      <div className="silk-vignette" />
    </div>
  );
}

export default SilkBackground;
