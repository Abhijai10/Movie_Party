const reactions = ["😂", "❤️", "😮", "🔥", "😭", "👏"] as const;

type ReactionTrayProps = {
  warning: string;
  onSendReaction: (reaction: string) => void;
};

/**
 * §Reactions: the tray renders ONLY while open — the dock emoji button
 * owns the toggle and the parent auto-hides it 5s after opening, so the
 * movie surface never carries a permanent reaction palette.
 */
export function ReactionTray({ warning, onSendReaction }: ReactionTrayProps) {
  return (
    <div className="reaction-tray" aria-label="Reactions" data-testid="reaction-tray">
      {reactions.map((reaction) => (
        <button
          key={reaction}
          type="button"
          aria-label={`Send ${reaction} reaction`}
          onClick={() => {
            onSendReaction(reaction);
          }}
        >
          {reaction}
        </button>
      ))}
      {warning ? <span role="status">{warning}</span> : null}
    </div>
  );
}

export function FloatingReactions({
  reactions: floatingReactions,
}: {
  reactions: Array<{ id: string; sender: string; reaction: string }>;
}) {
  return (
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
  );
}
