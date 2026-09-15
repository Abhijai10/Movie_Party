import { AnimatePresence, motion } from "framer-motion";
import { MessageCircle, Send, X } from "lucide-react";
import {
  useRef,
  useEffect,
  type KeyboardEvent as ReactKeyboardEvent,
  type SyntheticEvent,
} from "react";
import type { AppSnapshot } from "../../backend/appRuntime";

/**
 * UI_UX_SPEC §34–36 via ADR-0003 (replaces the right-anchored
 * `.cinema-chat-overlay` family; that family's behavioral rules —
 * Ghost/Privacy suppression, unread badge, typing preservation — are
 * owned by CinemaView and unchanged).
 *
 * §34 CHAT COMPOSE: a bottom-center input above the control dock,
 * `min(420px, 56vw)`, invoked on Enter — not a big panel.
 *
 * §36 CHAT HISTORY: press C — a centered translucent card
 * `min(620px, 75vw) × min(560px, 70vh)`; the movie remains full size
 * behind; opening it never auto-pauses.
 */

type ChatComposeProps = {
  draft: string;
  isDraftTooLong: boolean;
  onDraftChange: (value: string) => void;
  onSubmit: (event: SyntheticEvent<HTMLFormElement>) => void;
  onClose: () => void;
};

export function ChatCompose({
  draft,
  isDraftTooLong,
  onDraftChange,
  onSubmit,
  onClose,
}: ChatComposeProps) {
  return (
    <AnimatePresence>
      <motion.form
        key="chat-compose"
        initial={{ opacity: 0, y: 16, x: "-50%" }}
        animate={{ opacity: 1, y: 0, x: "-50%" }}
        exit={{ opacity: 0, y: 16, x: "-50%" }}
        transition={{ duration: 0.25, ease: "easeOut" }}
        onSubmit={onSubmit}
        className="chat-compose-bar"
        data-testid="chat-compose"
      >
        <input
          autoFocus
          type="text"
          value={draft}
          aria-invalid={isDraftTooLong}
          aria-label="Chat message"
          onChange={(event) => {
            onDraftChange(event.target.value);
          }}
          onKeyDown={(event: ReactKeyboardEvent<HTMLInputElement>) => {
            if (event.key === "Escape") {
              onClose();
            }
          }}
          placeholder="Press Enter to send…"
          className="chat-compose-input"
          data-testid="chat-input"
        />
        <button
          type="submit"
          className="chat-send-btn"
          disabled={draft.trim().length === 0 || isDraftTooLong}
          data-testid="chat-send-btn"
          aria-label="Send message"
        >
          <Send className="w-4 h-4 text-white" strokeWidth={1.8} />
        </button>
        <small className="sr-only">
          {isDraftTooLong ? "Message is over 2000 bytes." : "Enter sends. Esc closes."}
        </small>
      </motion.form>
    </AnimatePresence>
  );
}

type ChatHistoryCardProps = {
  snapshot: AppSnapshot;
  onClose: () => void;
};

/**
 * §36 CHAT HISTORY — right-anchored side panel. It accompanies the
 * compose bar: opening chat shows the recent thread so a reply has
 * context. The panel auto-hides after 5 s, but a resting cursor pauses
 * the countdown for as long as it stays over the panel; the countdown
 * restarts when the cursor leaves. The compose bar is unaffected —
 * typing continues after the history has stepped aside.
 */
export function ChatHistoryCard({ snapshot, onClose }: ChatHistoryCardProps) {
  const listRef = useRef<HTMLDivElement>(null);
  const hoverRef = useRef(false);
  const closeTimerRef = useRef<number | null>(null);
  // onClose is a fresh closure per parent render; keep it in a ref so a
  // frequently-updating snapshot (e.g. playback ticks) can't keep
  // resetting the countdown via the effect deps.
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  // Auto-hide (#2): the panel stays 5 s, then slides away — unless the
  // cursor is resting on it, which pauses the countdown for as long as
  // it stays; leaving restarts the 5 s. The compose bar is unaffected:
  // typing continues after the history has stepped aside.
  useEffect(() => {
    if (hoverRef.current) {
      return;
    }

    closeTimerRef.current = window.setTimeout(() => {
      onCloseRef.current();
    }, 5000);
    return () => {
      if (closeTimerRef.current != null) {
        window.clearTimeout(closeTimerRef.current);
        closeTimerRef.current = null;
      }
    };
  }, [snapshot.chat.length]);

  // Scroll to the newest line whenever the thread changes.
  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [snapshot.chat.length]);

  // Esc closes the panel (and the compose via CinemaView's own listener).
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onCloseRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  return (
    <AnimatePresence>
      <motion.div
        key="chat-history"
        initial={{ opacity: 0, x: 24 }}
        animate={{ opacity: 1, x: 0 }}
        exit={{ opacity: 0, x: 24 }}
        transition={{ duration: 0.25, ease: "easeOut" }}
        className="chat-history-panel"
        data-testid="chat-history"
        role="dialog"
        aria-label="Chat history"
        onMouseEnter={() => {
          hoverRef.current = true;
          if (closeTimerRef.current != null) {
            window.clearTimeout(closeTimerRef.current);
            closeTimerRef.current = null;
          }
        }}
        onMouseLeave={() => {
          hoverRef.current = false;
          // Restart the 5 s countdown now that the cursor is off.
          if (closeTimerRef.current != null) {
            window.clearTimeout(closeTimerRef.current);
          }
          closeTimerRef.current = window.setTimeout(() => {
            onCloseRef.current();
          }, 5000);
        }}
      >
        <div className="chat-history-panel-card">
          <header>
            <div className="chp-label">
              <MessageCircle className="w-4 h-4" strokeWidth={1.6} />
              <span>Chat history</span>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="chp-close-btn"
              data-testid="chat-close-btn"
              aria-label="Close chat history"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </header>
          <div ref={listRef} className="chp-list">
            {snapshot.chat.length === 0 ? (
              <div className="chp-empty">
                <div className="chp-empty-mark">
                  <MessageCircle className="w-4 h-4" strokeWidth={1.4} />
                </div>
                <p className="chp-empty-title">No messages yet</p>
                <p className="chp-empty-hint">Say something during the show</p>
              </div>
            ) : (
              snapshot.chat.map((message) => (
                <div key={message.id} className="chp-msg">
                  <span className="chp-msg-sender">{message.sender}</span>
                  <div className="chp-msg-body">{message.body}</div>
                </div>
              ))
            )}
          </div>
        </div>
      </motion.div>
    </AnimatePresence>
  );
}
