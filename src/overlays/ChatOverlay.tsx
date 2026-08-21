import type { AppSnapshot } from "../backend/appRuntime";
import type { KeyboardEvent as ReactKeyboardEvent, SyntheticEvent } from "react";

type ChatOverlayProps = {
  snapshot: AppSnapshot;
  draft: string;
  isComposing: boolean;
  isHistoryOpen: boolean;
  isDraftTooLong: boolean;
  onDraftChange: (value: string) => void;
  onSubmit: (event: SyntheticEvent<HTMLFormElement>) => void;
  onCloseCompose: () => void;
  onCloseHistory: () => void;
};

export function ChatOverlay({
  snapshot,
  draft,
  isComposing,
  isHistoryOpen,
  isDraftTooLong,
  onDraftChange,
  onSubmit,
  onCloseCompose,
  onCloseHistory,
}: ChatOverlayProps) {
  const visibleMessages = snapshot.chat.slice(-3);

  return (
    <>
      <article className="chat-bubble" aria-label="Recent chat message">
        {visibleMessages.map((message) => (
          <p key={message.id}>
            <strong>{message.sender}</strong>
            <span>{message.body}</span>
          </p>
        ))}
      </article>
      {isComposing ? (
        <form className="chat-compose" aria-label="Chat compose" onSubmit={onSubmit}>
          <input
            autoFocus
            value={draft}
            aria-invalid={isDraftTooLong}
            onChange={(event) => {
              onDraftChange(event.target.value);
            }}
            onKeyDown={(event: ReactKeyboardEvent<HTMLInputElement>) => {
              if (event.key === "Escape") {
                onCloseCompose();
              }
            }}
            placeholder="Type message..."
          />
          <button type="submit" disabled={draft.trim().length === 0 || isDraftTooLong}>
            Send
          </button>
          <small>{isDraftTooLong ? "Message is over 2000 bytes." : "Enter sends. Esc closes."}</small>
        </form>
      ) : null}
      {isHistoryOpen ? (
        <section className="chat-history" aria-labelledby="chat-history-title">
          <div className="history-header">
            <h2 id="chat-history-title">Chat</h2>
            <button type="button" onClick={onCloseHistory}>
              Close
            </button>
          </div>
          <ol>
            {snapshot.chat.map((message) => (
              <li key={message.id}>
                <strong>{message.sender}</strong>
                <span>{message.body}</span>
              </li>
            ))}
          </ol>
        </section>
      ) : null}
    </>
  );
}
