type ErrorStateProps = {
  message: string | null;
  tone?: "error" | "info";
  onRetry?: () => void;
};

export function ErrorState({ message, tone = "error", onRetry }: ErrorStateProps) {
  if (!message) {
    return null;
  }

  if (tone === "info") {
    return <p className="message-state message-state-info" role="status">{message}</p>;
  }

  return (
    <section className="message-state message-state-error" role="alert">
      <div>
        <strong>{friendlyError(message)}</strong>
        <span>Nothing has been changed. Check the room setup and try again.</span>
      </div>
      <div className="error-actions">
        {onRetry ? <button type="button" onClick={onRetry}>Try again</button> : null}
        <details>
          <summary>Technical details</summary>
          <code>{message}</code>
        </details>
      </div>
    </section>
  );
}

function friendlyError(message: string): string {
  if (/MP-[A-Z]+-\d+/.test(message) || message.toLowerCase().includes("failed")) {
    return "Something went wrong while preparing your cinema room. Please try again.";
  }

  return message;
}
