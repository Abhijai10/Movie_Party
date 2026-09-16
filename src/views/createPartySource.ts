/**
 * Pure selection logic for the Create Party "local movie" source (F3).
 *
 * A dropped file only counts as a selection when Movie Party can actually
 * open it — which means a real filesystem path. The browser `File` object an
 * HTML5 drag-drop hands to a handler carries no path (by design), so the
 * native Tauri drag-drop event is the source of truth.
 *
 * Keeping the rule here means the UI can never render "Selected" for a
 * source the backend would receive as `null`, which is exactly how the
 * drag-and-drop bug produced an empty cinema room.
 */

/** Extensions Movie Party can hand to libmpv. */
export const MEDIA_FILE_EXTENSION_PATTERN = /\.(mp4|mkv|mov|webm)$/i;

/** Human-readable list of the extensions above, for error copy. */
export const SUPPORTED_MEDIA_EXTENSIONS_LABEL = "MP4, MKV, MOV or WebM";

/** True when `value` names a file Movie Party can play. */
export function isSupportedMediaFile(value: string): boolean {
  return MEDIA_FILE_EXTENSION_PATTERN.test(value.trim());
}

/** Final path segment, for display. Handles both path separators. */
export function mediaFileNameFromPath(value: string): string {
  const trimmed = value.trim();
  if (trimmed.length === 0) {
    return "";
  }
  return trimmed.split(/[\\/]/).at(-1) ?? "";
}

/**
 * The first dropped path Movie Party can open, or `null` when the drop cannot
 * be used. `null` is the signal to surface a real error rather than pretend a
 * movie was selected.
 */
export function usableDroppedPath(paths: readonly string[]): string | null {
  return paths.find((candidate) => isSupportedMediaFile(candidate)) ?? null;
}

/** Honest message for a drop Movie Party cannot act on. */
export function droppedPathErrorMessage(paths: readonly string[]): string {
  if (paths.length === 0) {
    return "That drop did not include a file Movie Party can open.";
  }
  return `Movie Party can play ${SUPPORTED_MEDIA_EXTENSIONS_LABEL} files. “${mediaFileNameFromPath(
    paths[0] ?? "",
  )}” is not one of them.`;
}
