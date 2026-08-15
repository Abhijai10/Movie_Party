import type { KeyboardEvent as ReactKeyboardEvent, SyntheticEvent } from "react";
import { useEffect, useMemo, useState } from "react";

type CinemaModeProps = {
  onLeave: () => void;
};

type ChatMessage = {
  id: string;
  sender: string;
  body: string;
};

type FloatingReaction = {
  id: string;
  sender: string;
  reaction: string;
};

const initialMessages: ChatMessage[] = [
  { id: "chat-1", sender: "Rahul", body: "BRO WHAT" },
  { id: "chat-2", sender: "Abhijai", body: "Paused so we stay together." },
  { id: "chat-3", sender: "Rahul", body: "Ready when you are." },
];

const reactions = ["😂", "❤️", "😮", "🔥", "😭", "👏"] as const;
const maxReactionEvents = 5;
const reactionWindowMs = 3_000;
const chatBodyLimitBytes = 2_000;

export function CinemaMode({ onLeave }: CinemaModeProps) {
  const [messages, setMessages] = useState<ChatMessage[]>(initialMessages);
  const [draft, setDraft] = useState("");
  const [isComposing, setIsComposing] = useState(false);
  const [isHistoryOpen, setIsHistoryOpen] = useState(false);
  const [floatingReactions, setFloatingReactions] = useState<FloatingReaction[]>([
    { id: "reaction-seed", sender: "Rahul", reaction: "😮" },
  ]);
  const [reactionEvents, setReactionEvents] = useState<number[]>([]);
  const [reactionWarning, setReactionWarning] = useState("");
  const visibleMessages = messages.slice(-3);
  const encodedDraftLength = useMemo(() => new TextEncoder().encode(draft).length, [draft]);
  const isDraftTooLong = encodedDraftLength > chatBodyLimitBytes;

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      const isTextInput =
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement;

      if (event.key === "Escape") {
        setIsComposing(false);
        setIsHistoryOpen(false);
        return;
      }

      if (isTextInput) {
        return;
      }

      if (event.key === "Enter") {
        event.preventDefault();
        setIsComposing(true);
      }

      if (event.key.toLowerCase() === "c") {
        event.preventDefault();
        setIsHistoryOpen((current) => !current);
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, []);

  const sendMessage = (event: SyntheticEvent<HTMLFormElement>) => {
    event.preventDefault();

    if (draft.trim().length === 0 || isDraftTooLong) {
      return;
    }

    setMessages((current) => [
      ...current,
      {
        id: `chat-${String(Date.now())}`,
        sender: "Abhijai",
        body: draft.trim(),
      },
    ]);
    setDraft("");
    setIsComposing(false);
  };

  const handleDraftKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      setIsComposing(false);
    }
  };

  const sendReaction = (reaction: string) => {
    const now = Date.now();
    const recentEvents = reactionEvents.filter((eventTime) => now - eventTime < reactionWindowMs);

    if (recentEvents.length >= maxReactionEvents) {
      setReactionWarning("Reaction limit reached");
      return;
    }

    setReactionWarning("");
    setReactionEvents([...recentEvents, now]);

    const nowText = String(now);
    const floatingReaction = {
      id: `reaction-${nowText}-${reaction}`,
      sender: "Abhijai",
      reaction,
    };

    setFloatingReactions((current) => [...current.slice(-3), floatingReaction]);
    window.setTimeout(() => {
      setFloatingReactions((current) =>
        current.filter((candidate) => candidate.id !== floatingReaction.id),
      );
    }, 2_400);
  };

  return (
    <main className="cinema-shell" aria-label="Cinema mode">
      <section className="movie-surface" aria-label="Movie">
        <div className="movie-frame">
          <div className="movie-light" />
          <span>Interstellar</span>
        </div>
        <article className="camera-card" aria-label="Rahul camera">
          <strong>Rahul</strong>
          <span>Camera on - Mic muted</span>
        </article>
        <article className="chat-bubble" aria-label="Recent chat message">
          {visibleMessages.map((message) => (
            <p key={message.id}>
              <strong>{message.sender}</strong>
              <span>{message.body}</span>
            </p>
          ))}
        </article>
        <div className="reaction-float-layer" aria-live="polite">
          {floatingReactions.map((reaction) => (
            <span
              key={reaction.id}
              className="floating-reaction"
              aria-label={`${reaction.sender} reacted`}
            >
              {reaction.reaction}
            </span>
          ))}
        </div>
        <section className="buffer-overlay" aria-live="polite">
          <h1>Paused to keep you together</h1>
          <p>Rahul is buffering</p>
          <div className="meter">
            <span style={{ width: "64%" }} />
          </div>
          <small>Camera and chat are still available.</small>
        </section>
        <section className="reconnect-overlay" aria-live="polite">
          <h2>Rahul disconnected.</h2>
          <p>The movie has been paused. Reconnecting...</p>
        </section>
        {isComposing ? (
          <form className="chat-compose" aria-label="Chat compose" onSubmit={sendMessage}>
            <input
              autoFocus
              value={draft}
              aria-invalid={isDraftTooLong}
              onChange={(event) => {
                setDraft(event.target.value);
              }}
              onKeyDown={handleDraftKeyDown}
              placeholder="Type message..."
            />
            <button type="submit" disabled={draft.trim().length === 0 || isDraftTooLong}>
              Send
            </button>
            <small>
              {isDraftTooLong ? "Message is over 2000 bytes." : "Enter sends. Esc closes."}
            </small>
          </form>
        ) : null}
        {isHistoryOpen ? (
          <section className="chat-history" aria-labelledby="chat-history-title">
            <div className="history-header">
              <h2 id="chat-history-title">Chat</h2>
              <button
                type="button"
                onClick={() => {
                  setIsHistoryOpen(false);
                }}
              >
                Close
              </button>
            </div>
            <ol>
              {messages.map((message) => (
                <li key={message.id}>
                  <strong>{message.sender}</strong>
                  <span>{message.body}</span>
                </li>
              ))}
            </ol>
          </section>
        ) : null}
        <div className="reaction-tray" aria-label="Reactions">
          {reactions.map((reaction) => (
            <button
              key={reaction}
              type="button"
              aria-label={`Send ${reaction} reaction`}
              onClick={() => {
                sendReaction(reaction);
              }}
            >
              {reaction}
            </button>
          ))}
          {reactionWarning ? <span role="status">{reactionWarning}</span> : null}
        </div>
        <nav className="control-dock" aria-label="Cinema controls">
          <button type="button" aria-label="Back 10 seconds">
            -10
          </button>
          <button type="button" aria-label="Pause for both participants">
            Pause
          </button>
          <button type="button" aria-label="Forward 10 seconds">
            +10
          </button>
          <button type="button" aria-label="Mute microphone">
            Mic
          </button>
          <button type="button" aria-label="Toggle camera">
            Camera
          </button>
          <button
            type="button"
            aria-label="Open chat"
            onClick={() => {
              setIsComposing(true);
            }}
          >
            Chat
          </button>
          <button
            type="button"
            aria-label="Send reaction"
            onClick={() => {
              sendReaction("👏");
            }}
          >
            React
          </button>
          <button type="button" onClick={onLeave} aria-label="Open party menu">
            More
          </button>
        </nav>
      </section>
    </main>
  );
}
