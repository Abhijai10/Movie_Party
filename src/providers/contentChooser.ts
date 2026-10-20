import { invoke } from "@tauri-apps/api/core";

export type ContentIdentity = {
  providerId: string;
  contentId: string;
  allowedUrl: string;
  playlistId: string | null;
};
export type ContentTab = {
  targetId: string;
  title: string;
  identity: ContentIdentity | null;
  thumbnailUrl: string | null;
  documentId: string;
  focused: boolean;
  visible: boolean;
  playbackReady: boolean;
  issue: string | null;
  selectionKey: string;
};
export type ContentChooserSnapshot = {
  browserReady: boolean;
  tabs: ContentTab[];
  suggestedTargetId: string | null;
  selected: ContentTab | null;
  confirmedKey: string | null;
  contentReady: boolean;
  issue: string | null;
};
export const getProviderContent = (providerId: string) =>
  invoke<ContentChooserSnapshot>("get_provider_content", { providerId });
export const browseProviderContent = (providerId: string, input: string, direct: boolean) =>
  invoke<ContentChooserSnapshot>("browse_provider_content", { providerId, input, direct });
export const confirmProviderContent = (
  providerId: string,
  targetId: string,
  selectionKey: string,
) =>
  invoke<ContentChooserSnapshot>("confirm_provider_content", {
    providerId,
    targetId,
    selectionKey,
  });
export async function validateProviderContent(
  providerId: string,
  selectionKey: string,
): Promise<void> {
  await invoke("validate_provider_content", { providerId, selectionKey });
}

export function canCreateSelectedContent(
  snapshot: ContentChooserSnapshot | null,
  preview: ContentTab | null,
  fresh: boolean,
): boolean {
  return Boolean(
    fresh &&
    snapshot?.browserReady &&
    snapshot.contentReady &&
    preview?.playbackReady &&
    preview.identity &&
    preview.selectionKey === snapshot.confirmedKey &&
    preview.targetId === snapshot.selected?.targetId,
  );
}

/** One CDP operation at a time, including user actions queued behind polling. */
export function createContentQueue() {
  let tail: Promise<unknown> = Promise.resolve();
  return <T>(work: () => Promise<T>): Promise<T> => {
    const result = tail.then(work);
    tail = result.catch(() => undefined);
    return result;
  };
}
