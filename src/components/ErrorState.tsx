type ErrorStateProps = {
  message: string | null;
  tone?: "error" | "info";
};

export function ErrorState({ message, tone = "error" }: ErrorStateProps) {
  if (!message) {
    return null;
  }

  return (
    <p className={`message-state message-state-${tone}`} role={tone === "error" ? "alert" : "status"}>
      {message}
    </p>
  );
}
