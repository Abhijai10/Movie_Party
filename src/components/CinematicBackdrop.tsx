type CinematicBackdropProps = {
  label?: string;
};

export function CinematicBackdrop({
  label = "Cinematic ambient background",
}: CinematicBackdropProps) {
  return (
    <div className="cinematic-backdrop" aria-label={label} aria-hidden="true">
      <div className="cinematic-backdrop-layer cinematic-backdrop-layer-a" />
      <div className="cinematic-backdrop-layer cinematic-backdrop-layer-b" />
    </div>
  );
}
