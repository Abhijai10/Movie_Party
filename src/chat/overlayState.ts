export type ChatOverlayVisibility = {
  manualOpen: boolean;
  transientOpen: boolean;
};

export const closedChatOverlay: ChatOverlayVisibility = {
  manualOpen: false,
  transientOpen: false,
};

export function openChatManually(): ChatOverlayVisibility {
  return { manualOpen: true, transientOpen: false };
}

export function openChatPreview(): ChatOverlayVisibility {
  return { manualOpen: false, transientOpen: true };
}

export function closeChatPreview(current: ChatOverlayVisibility): ChatOverlayVisibility {
  return { ...current, transientOpen: false };
}

export function isChatOverlayOpen(current: ChatOverlayVisibility): boolean {
  return current.manualOpen || current.transientOpen;
}
