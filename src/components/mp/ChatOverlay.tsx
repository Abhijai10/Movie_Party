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
 * `min(560px, 70vw)`, invoked on Enter — not a big panel.
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
        initial={{ opacity: 0, y: 16 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: 16 }}
        transition={{ duration: 0.25, ease: "easeOut" }}
        onSubmit={onSubmit}
        className="absolute left-1/2 -translate-x-1/2 bottom-[132px] z-40 w-[min(560px,70vw)] flex items-center gap-2 rounded-full bg-black/60 backdrop-blur-xl border border-white/10 px-4 py-2 shadow-2xl"
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
          className="flex-1 bg-transparent text-sm text-white placeholder:text-white/40 focus:outline-none"
          data-testid="chat-input"
        />
        <button
          type="submit"
          className="w-9 h-9 rounded-full bg-[#6B46C1] hover:bg-[#553592] transition flex items-center justify-center disabled:opacity-40"
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

export function ChatHistoryCard({ snapshot, onClose }: ChatHistoryCardProps) {
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [snapshot.chat.length]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  return (
    <AnimatePresence>
      <motion.div
        key="chat-history"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.25, ease: "easeOut" }}
        className="fixed inset-0 z-40 flex items-center justify-center p-6"
        data-testid="chat-history"
        role="dialog"
        aria-label="Chat history"
      >
        <button
          type="button"
          className="absolute inset-0 bg-black/45 backdrop-blur-[2px] cursor-default"
          onClick={onClose}
          aria-label="Close chat history"
          data-testid="chat-history-backdrop"
        />
        <div className="relative w-[min(620px,75vw)] h-[min(560px,70vh)] rounded-2xl bg-[#0D0B14]/92 backdrop-blur-xl border border-white/10 shadow-2xl flex flex-col overflow-hidden">
          <header className="px-5 py-4 flex items-center justify-between border-b border-white/5">
            <div className="flex items-center gap-2.5">
              <MessageCircle className="w-4 h-4 text-white/70" strokeWidth={1.6} />
              <span className="text-[11px] tracking-[0.28em] uppercase text-white/80">
                Whisper Row
              </span>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="p-1.5 rounded-full hover:bg-white/10 transition"
              data-testid="chat-close-btn"
              aria-label="Close chat history"
            >
              <X className="w-4 h-4 text-white/70" />
            </button>
          </header>
          <div
            ref={listRef}
            className="flex-1 overflow-y-auto no-scrollbar px-5 py-4 space-y-3"
          >
            {snapshot.chat.length === 0 ? (
              <div className="h-full flex flex-col items-center justify-center text-center opacity-70">
                <div className="w-10 h-10 rounded-full border border-white/10 flex items-center justify-center mb-3">
                  <MessageCircle className="w-4 h-4 text-white/50" strokeWidth={1.4} />
                </div>
                <p className="font-serif-display text-lg text-white/80">No messages yet</p>
                <p className="text-[11px] tracking-[0.2em] uppercase text-white/40 mt-1.5">
                  Say something during the show
                </p>
              </div>
            ) : (
              snapshot.chat.map((message) => (
                <div key={message.id} className="flex flex-col">
                  <span className="text-[10px] tracking-[0.2em] uppercase text-white/40 mb-1">
                    {message.sender}
                  </span>
                  <div className="max-w-[80%] px-3.5 py-2 rounded-2xl text-sm bg-white/6 text-white/90 rounded-bl-sm">
                    {message.body}
                  </div>
                </div>
              ))
            )}
          </div>
          <footer className="px-5 py-3 border-t border-white/5 text-[11px] tracking-[0.2em] uppercase text-white/40">
            Press Esc to return to the movie
          </footer>
        </div>
      </motion.div>
    </AnimatePresence>
  );
}
