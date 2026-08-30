const reactions = ["😂", "❤️", "😮", "🔥", "😭", "👏"] as const;

type ReactionTrayProps = {
  warning: string;
  onSendReaction: (reaction: string) => void;
};

export function ReactionTray({ warning, onSendReaction }: ReactionTrayProps) {
  return (
    <div className="reaction-tray" aria-label="Reactions">
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
