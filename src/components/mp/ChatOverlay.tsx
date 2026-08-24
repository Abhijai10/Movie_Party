import { AnimatePresence, motion } from "framer-motion";
import { MessageCircle, Send, X } from "lucide-react";
import {
  useRef,
  useEffect,
  type KeyboardEvent as ReactKeyboardEvent,
  type SyntheticEvent,
} from "react";
import type { AppSnapshot } from "../../backend/appRuntime";

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
  const listRef = useRef<HTMLDivElement>(null);
  const open = isComposing || isHistoryOpen;

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [snapshot.chat.length, open]);

  return (
    <AnimatePresence>
      {open && (
        <motion.aside
          key="chat"
          initial={{ opacity: 0, x: 40 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: 40 }}
          transition={{ duration: 0.3, ease: "easeOut" }}
          className="absolute right-6 top-6 bottom-28 w-[340px] z-40 flex flex-col rounded-2xl overflow-hidden"
          style={{
            background: "rgba(9, 7, 15, 0.72)",
            backdropFilter: "blur(24px) saturate(140%)",
            WebkitBackdropFilter: "blur(24px) saturate(140%)",
            border: "1px solid rgba(159,122,234,0.18)",
            boxShadow: "0 30px 80px -20px rgba(0,0,0,0.7)",
          }}
          data-testid="chat-overlay"
        >
          <header className="px-5 py-4 flex items-center justify-between border-b border-white/5">
            <div className="flex items-center gap-2.5">
              <MessageCircle className="w-4 h-4 text-white/70" strokeWidth={1.6} />
              <span className="text-[11px] tracking-[0.28em] uppercase text-white/80">
                Whisper Row
              </span>
            </div>
            <button
              type="button"
              onClick={() => {
                onCloseCompose();
                onCloseHistory();
              }}
              className="p-1.5 rounded-full hover:bg-white/10 transition"
              data-testid="chat-close-btn"
              aria-label="Close chat"
            >
              <X className="w-4 h-4 text-white/70" />
            </button>
          </header>

          <div ref={listRef} className="flex-1 overflow-y-auto no-scrollbar px-5 py-4 space-y-3">
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

          {isComposing ? (
            <form
              onSubmit={onSubmit}
              className="p-3 border-t border-white/5 flex items-center gap-2"
            >
              <input
                autoFocus
                type="text"
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
                placeholder="Type a message..."
                className="flex-1 bg-white/5 border border-white/10 rounded-full px-4 py-2.5 text-sm text-white placeholder:text-white/40 focus:outline-none focus:border-[#9F7AEA]/50 transition"
                data-testid="chat-input"
              />
              <button
                type="submit"
                className="w-10 h-10 rounded-full bg-[#6B46C1] hover:bg-[#553592] transition flex items-center justify-center disabled:opacity-40"
                disabled={draft.trim().length === 0 || isDraftTooLong}
                data-testid="chat-send-btn"
                aria-label="Send"
              >
                <Send className="w-4 h-4 text-white" strokeWidth={1.8} />
              </button>
              <small className="sr-only">
                {isDraftTooLong ? "Message is over 2000 bytes." : "Enter sends. Esc closes."}
              </small>
            </form>
          ) : null}
        </motion.aside>
      )}
    </AnimatePresence>
  );
}

export default ChatOverlay;
