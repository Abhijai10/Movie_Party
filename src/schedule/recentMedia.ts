/**
 * Recently picked local movies for the self-contained Schedule flow.
 *
 * The Schedule screen lets the user pick a movie file directly (no Create
 * Party step first). A schedule created that way references the file by
 * its absolute path until the party replaces it with the real media id —
 * the same reference Home's Upcoming cards then display. This module
 * persists the readable filenames so those cards never show a raw path.
 *
 * Storage: localStorage only, capped, best-effort — never blocks.
 */

export type RecentMedia = {
  path: string;
  name: string;
  at: number;
};

const RECENT_MEDIA_KEY = "mp_recent_media_v1";
const RECENT_MEDIA_CAP = 8;

type StorageLike = Pick<Storage, "getItem" | "setItem"> | null;

/** The browser localStorage, or null outside a DOM / when blocked. */
export function localStorageOrNull(): StorageLike {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

function isValid(entry: unknown): entry is RecentMedia {
  if (typeof entry !== "object" || entry === null) return false;
  const candidate = entry as Partial<RecentMedia>;
  return (
    typeof candidate.path === "string" &&
    candidate.path.trim().length > 0 &&
    typeof candidate.name === "string" &&
    candidate.name.trim().length > 0 &&
    (candidate.at == null || typeof candidate.at === "number")
  );
}

/** The display filename for a path ("/x/y/movie.mkv" → "movie.mkv"). */
export function fileNameFromPath(path: string): string {
  const trimmed = path.trim();
  const parts = trimmed.split(/[\\/]/);
  return parts.at(-1) ?? trimmed;
}

export function listRecentMedia(storage: StorageLike): RecentMedia[] {
  if (storage == null) return [];
  try {
    const raw = storage.getItem(RECENT_MEDIA_KEY);
    if (raw == null) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isValid).map((entry) => ({
      path: entry.path,
      name: entry.name,
      at: typeof entry.at === "number" ? entry.at : 0,
    }));
  } catch {
    return [];
  }
}

/** Record a pick: dedupes by path, newest first, capped at 8 entries. */
export function rememberMedia(
  storage: StorageLike,
  path: string,
  name?: string,
): RecentMedia[] {
  const cleanPath = path.trim();
  const cleanName = (name ?? fileNameFromPath(path)).trim();
  if (cleanPath.length === 0 || cleanName.length === 0) {
    return listRecentMedia(storage);
  }
  const next = listRecentMedia(storage).filter((entry) => entry.path !== cleanPath);
  next.unshift({ path: cleanPath, name: cleanName, at: Date.now() });
  const capped = next.slice(0, RECENT_MEDIA_CAP);
  if (storage != null) {
    try {
      storage.setItem(RECENT_MEDIA_KEY, JSON.stringify(capped));
    } catch {
      /* storage full — the session still works, just not persisted */
    }
  }
  return capped;
}

/** Readable display name for a media reference (path or real id). */
export function displayNameFor(
  mediaRef: string,
  recent: RecentMedia[],
  fallback: (mediaId: string) => string,
): string {
  const hit = recent.find((entry) => entry.path === mediaRef);
  return hit != null ? hit.name : fallback(mediaRef);
}
