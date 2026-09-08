import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useRef } from "react";
import {
  bubbleMotion,
  bubbleStackOffsetPx,
  type ChatBubbleQueue,
  pruneChatBubbles,
} from "../chat/bubbleQueue";
import { useReducedMotion } from "../hooks/useReducedMotion";

type ChatBubblesProps = {
  queue: ChatBubbleQueue;
  /** Movie height in px — drives the §66 subtitle-zone offset. */
  movieHeightPx: number;
  subtitlesActive: boolean;
  onQueueChange: (next: ChatBubbleQueue) => void;
};

/**
 * UI_UX_SPEC §35 (via ADR-0003): incoming chat messages render as
 * lower-third ephemeral bubbles — never a permanent sidebar (§27).
 * The stack rises above the subtitle-sensitive bottom-15 % zone when
 * subtitles are active, and respects reduced motion with a plain fade.
 * The retiring bubbles (displaced by the max-3 cap) get one exit pass.
 */
export function ChatBubbles({
  queue,
  movieHeightPx,
  subtitlesActive,
  onQueueChange,
}: ChatBubblesProps) {
  const prefersReducedMotion = useReducedMotion();
  const motionMode = bubbleMotion(prefersReducedMotion);
  const latestQueueRef = useRef<ChatBubbleQueue>(queue);
  const timerRef = useRef<number | null>(null);

  latestQueueRef.current = queue;

  // Prune expired bubbles on a light interval; stop when empty.
  useEffect(() => {
    if (queue.bubbles.length === 0) {
      return;
    }
    timerRef.current = window.setInterval(() => {
      onQueueChange(pruneChatBubbles(latestQueueRef.current, Date.now()));
    }, 400);
    return () => {
      if (timerRef.current != null) {
        window.clearInterval(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [queue.bubbles.length, onQueueChange]);

  const offsetPx = bubbleStackOffsetPx(movieHeightPx, subtitlesActive);

  return (
    <div
      data-testid="chat-bubbles"
      className="pointer-events-none absolute left-0 right-0 z-30 flex flex-col justify-end gap-2"
      style={{ bottom: `calc(18% + ${String(offsetPx)}px)` }}
      aria-live="polite"
      aria-label="Chat messages"
    >
      <AnimatePresence initial={false}>
        {queue.bubbles.map((bubble) => (
          <motion.div
            key={bubble.id}
            initial={motionMode === "float" ? { opacity: 0, y: 12 } : { opacity: 0 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.25, ease: "easeOut" }}
            className="mx-auto w-fit max-w-[min(560px,70vw)] px-3.5 py-2 rounded-2xl text-sm bg-black/55 backdrop-blur-md text-white/90 rounded-bl-sm border border-white/10"
          >
            <span className="text-[10px] tracking-[0.2em] uppercase text-white/40 mr-2">
              {bubble.sender}
            </span>
            {bubble.body}
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}

export default ChatBubbles;
