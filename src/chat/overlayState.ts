export type ChatOverlayVisibility = {
  manualOpen: boolean;
  transientOpen: boolean;
  /**
   * The history panel has auto-hidden (5 s, see ChatHistoryCard) while
   * the compose bar may still be up. It resets the next time chat is
   * opened, so the panel reappears alongside the composer.
   */
  historyDismissed: boolean;
};

export const closedChatOverlay: ChatOverlayVisibility = {
  manualOpen: false,
  transientOpen: false,
  historyDismissed: false,
};

export function openChatManually(): ChatOverlayVisibility {
  return { manualOpen: true, transientOpen: false, historyDismissed: false };
}

export function openChatPreview(): ChatOverlayVisibility {
  return { manualOpen: false, transientOpen: true, historyDismissed: false };
}

export function closeChatPreview(current: ChatOverlayVisibility): ChatOverlayVisibility {
  return { ...current, transientOpen: false };
}

/**
 * The history panel's 5 s auto-hide: only the panel steps aside — the
 * compose bar (manualOpen) keeps its state, so typing continues.
 */
export function dismissChatHistory(current: ChatOverlayVisibility): ChatOverlayVisibility {
  return { ...current, historyDismissed: true };
}

export function isChatOverlayOpen(current: ChatOverlayVisibility): boolean {
  return current.manualOpen || current.transientOpen;
}

/**
 * Semantics of an incoming chat message (MASTER_PRD social layer).
 *
 * - While the overlay is already open, nothing changes (the message is
 *   visible; no unread flag).
 * - While it is hidden, the message reveals a transient preview — the
 *   "incoming message can reveal chat" rule — so the user sees it arrive.
 * - While social UI is suppressed (Ghost Mode or Privacy Mode), nothing is
 *   revealed; the caller keeps an unread indicator instead, surfaced when
 *   the modes end.
 */
export function applyChatArrival(
  current: ChatOverlayVisibility,
  isSocialHidden: boolean,
): ChatOverlayVisibility {
  if (isSocialHidden) {
    return current;
  }
  if (isChatOverlayOpen(current)) {
    return current;
  }
  return openChatPreview();
}

/**
 * Whether the chat toggle affordance (Enter / "c" / the chat button) may
 * act. Ghost and Privacy Modes hide all social UI; the toggle must not
 * reveal it while they are active.
 */
export function canToggleChat(isSocialHidden: boolean): boolean {
  return !isSocialHidden;
}
