/**
 * TMDB trending feed — Home hero decoration.
 *
 * Design rules (all enforced):
 *  - NEVER blocks the app: every failure resolves to the bundled fallback
 *    feature list, and the network call has a hard timeout + AbortController.
 *  - NEVER hard-codes a secret: the user's own pasted TMDB key (Settings →
 *    General) lives ONLY in localStorage, and the shared build-time default
 *    comes from .env / a CI secret — nothing sensitive is committed; the
 *    invite link and the database never carry either.
 *  - Local cache: the parsed feed is cached in localStorage with a
 *    timestamp; a fresh fetch runs at most once per TTL.
 *  - Attribution: the hero renders the required TMDB attribution line;
 *    this module only supplies the data.
 */

export type TmdbFeature = {
  title: string;
  year: string;
  tagline: string;
  posterUrl: string | null;
  backdropUrl: string | null;
  /** The gradient pair used behind/instead of poster art. */
  from: string;
  to: string;
  accent: string;
};

/** Bundled fallback — the shipped hero wall, the offline answer. */
export const FALLBACK_FEATURES: TmdbFeature[] = [
  {
    title: "Inception",
    year: "2010",
    tagline: "A dream within a dream.",
    posterUrl: null,
    backdropUrl: null,
    from: "#1E1B4B",
    to: "#4C1D95",
    accent: "#A78BFA",
  },
  {
    title: "Interstellar",
    year: "2014",
    tagline: "Go further than anyone before.",
    posterUrl: null,
    backdropUrl: null,
    from: "#082F49",
    to: "#0E7490",
    accent: "#67E8F9",
  },
  {
    title: "La La Land",
    year: "2016",
    tagline: "Here's to the fools who dream.",
    posterUrl: null,
    backdropUrl: null,
    from: "#7C2D12",
    to: "#B45309",
    accent: "#FDBA74",
  },
  {
    title: "Spirited Away",
    year: "2001",
    tagline: "The tunnel leads somewhere new.",
    posterUrl: null,
    backdropUrl: null,
    from: "#064E3B",
    to: "#0F766E",
    accent: "#6EE7B7",
  },
  {
    title: "Dune",
    year: "2021",
    tagline: "Beyond fear, destiny awaits.",
    posterUrl: null,
    backdropUrl: null,
    from: "#78350F",
    to: "#A16207",
    accent: "#FCD34D",
  },
  {
    title: "The Grand Budapest",
    year: "2014",
    tagline: "A perfect holiday, mostly.",
    posterUrl: null,
    backdropUrl: null,
    from: "#831843",
    to: "#BE185D",
    accent: "#F9A8D4",
  },
];

/** The accent/gradient palette recycled for TMDB items without art. */
const ACCENTS: Array<{ from: string; to: string; accent: string }> = [
  { from: "#1E1B4B", to: "#4C1D95", accent: "#A78BFA" },
  { from: "#082F49", to: "#0E7490", accent: "#67E8F9" },
  { from: "#7C2D12", to: "#B45309", accent: "#FDBA74" },
  { from: "#064E3B", to: "#0F766E", accent: "#6EE7B7" },
  { from: "#78350F", to: "#A16207", accent: "#FCD34D" },
  { from: "#831843", to: "#BE185D", accent: "#F9A8D4" },
];

/** The user's own TMDB credential — localStorage only, never the repo. */
export const TMDB_TOKEN_STORAGE_KEY = "mp_tmdb_token";

/**
 * The build-time default credential — the SHARED key for the two-person app.
 *
 * Provided via `VITE_TMDB_TOKEN` in `.env` (local builds) or the GitHub
 * Actions secret of the same name (release builds); Vite statically
 * inlines the value at build time, so every install — yours and your
 * friend's — carries it without either of you pasting anything. Read at
 * call time (not a module const) so tests can stub the env per-case; in
 * a production build the call site is still replaced by the inlined
 * literal. It is deliberately NOT committed: a key in the repo would
 * leak into any fork or archive of the source. Resolution priority per
 * device:
 *   1. A key pasted in Settings → General (localStorage) — always wins,
 *      so if the shared key dies it can be swapped per device without a
 *      new build.
 *   2. This bundled default (build-time .env / CI secret).
 *   3. Nothing → the bundled gradient wall, zero network.
 */
export function bundledTmdbToken(): string {
  return import.meta.env.VITE_TMDB_TOKEN ?? "";
}
const CACHE_KEY = "mp_tmdb_trending_v1";
/** Outcome of the last live fetch attempt — surfaces in Settings so a
 *  configured-but-unreachable TMDB (common: ISP DNS/packet blocking in
 *  some regions) is visible instead of silently showing gradients. */
const STATUS_KEY = "mp_tmdb_status_v1";
const TRENDING_PATH = "https://api.themoviedb.org/3/trending/movie/week?language=en-US";
const POSTER_BASE = "https://image.tmdb.org/t/p/w500";
const BACKDROP_BASE = "https://image.tmdb.org/t/p/w780";

/** Cache TTL — at most one fetch per hour per session. */
export const CACHE_TTL_MS = 60 * 60 * 1000;
/** Hard per-request timeout — never let the hero wait on the network. */
const FETCH_TIMEOUT_MS = 6000;
/** Live refresh cadence while the Home screen stays open. */
export const LIVE_REFRESH_MS = 30 * 60 * 1000;

/** Minimal storage surface (browser localStorage; injectable in tests). */
export type TmdbStorage = {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
};

/** Resolves the browser localStorage, or null outside a DOM. */
export function browserStorage(): TmdbStorage | null {
  try {
    const storage = typeof window === "undefined" ? null : window.localStorage;
    return storage != null
      ? {
          getItem: (key) => {
            return storage.getItem(key);
          },
          setItem: (key, value) => {
            storage.setItem(key, value);
          },
          removeItem: (key) => {
            storage.removeItem(key);
          },
        }
      : null;
  } catch {
    // Accessing localStorage can itself throw (privacy mode).
    return null;
  }
}

export function getTmdbToken(storage: TmdbStorage | null): string {
  if (storage == null) return "";
  try {
    const raw = storage.getItem(TMDB_TOKEN_STORAGE_KEY);
    return raw != null ? raw.trim() : "";
  } catch {
    return "";
  }
}

/**
 * The credential actually used for the next fetch: the user's pasted key
 * (Settings → General) if present, else the build-time shared default.
 * A per-device paste always wins, so a retired shared key can be
 * replaced without shipping a new build.
 */
export function resolveTmdbToken(storage: TmdbStorage | null): string {
  const pasted = getTmdbToken(storage);
  return pasted.length > 0 ? pasted : bundledTmdbToken();
}

export function setTmdbToken(storage: TmdbStorage | null, token: string): void {
  if (storage == null) return;
  const trimmed = token.trim();
  try {
    if (trimmed.length === 0) {
      storage.removeItem(TMDB_TOKEN_STORAGE_KEY);
    } else {
      storage.setItem(TMDB_TOKEN_STORAGE_KEY, trimmed);
    }
  } catch {
    /* storage unavailable — the session simply stays on the fallback wall */
  }
}

function paletteFor(index: number): { from: string; to: string; accent: string } {
  return ACCENTS[index % ACCENTS.length] ?? {
    from: "#1E1B4B",
    to: "#0C0A20",
    accent: "#8B5CF6",
  };
}

function yearFrom(date?: string | null): string {
  if (!date) return "—";
  const year = date.slice(0, 4);
  return /^\d{4}$/.test(year) ? year : "—";
}

type TmdbItem = {
  title?: unknown;
  release_date?: unknown;
  overview?: unknown;
  poster_path?: unknown;
  backdrop_path?: unknown;
};

/** Map a TMDB trending payload to the hero feature shape. Testable. */
export function mapTrendingPayload(payload: unknown): TmdbFeature[] | null {
  if (typeof payload !== "object" || payload === null) return null;
  const results = (payload as { results?: unknown }).results;
  if (!Array.isArray(results)) return null;

  const features: TmdbFeature[] = [];
  for (const raw of results) {
    if (typeof raw !== "object" || raw === null) continue;
    const item = raw as TmdbItem;
    const title = typeof item.title === "string" ? item.title.trim() : "";
    if (title.length === 0 || title.length > 80) continue;
    const posterPath = typeof item.poster_path === "string" ? item.poster_path : null;
    const backdropPath = typeof item.backdrop_path === "string" ? item.backdrop_path : null;
    const overview =
      typeof item.overview === "string" && item.overview.trim().length > 0
        ? item.overview.trim()
        : "";
    const palette = paletteFor(features.length);
    features.push({
      title,
      year: yearFrom(typeof item.release_date === "string" ? item.release_date : null),
      tagline: overview.length > 90 ? `${overview.slice(0, 87).trimEnd()}…` : overview,
      posterUrl: posterPath ? `${POSTER_BASE}${posterPath}` : null,
      backdropUrl: backdropPath
        ? `${BACKDROP_BASE}${backdropPath}`
        : posterPath
          ? `${POSTER_BASE}${posterPath}`
          : null,
      from: palette.from,
      to: palette.to,
      accent: palette.accent,
    });
    if (features.length >= FALLBACK_FEATURES.length) break;
  }
  return features.length > 0 ? features : null;
}

/**
 * Outcome of one trending fetch (F42).
 *
 * The three failure kinds are distinct on purpose: `http` is "TMDB answered,
 * but not usably", `parse` is "TMDB answered 200 with a body we could not
 * read", and a thrown error is `network` ("we never reached TMDB"). Previously
 * a malformed body propagated as a throw and was recorded as a NETWORK
 * failure, which mislabelled the diagnostic and left the declared `parse` kind
 * unreachable.
 */
type TrendingFetch =
  | { outcome: "ok"; features: TmdbFeature[] }
  | { outcome: "http" }
  | { outcome: "parse" };

async function fetchTrending(
  token: string,
  fetchImpl: typeof fetch,
  abort: AbortSignal,
): Promise<TrendingFetch> {
  // v4 read token (JWT) → Bearer header; classic v3 key → api_key param.
  const isBearer = token.startsWith("eyJ");
  const url = isBearer ? TRENDING_PATH : `${TRENDING_PATH}&api_key=${encodeURIComponent(token)}`;
  const headers: Record<string, string> = { accept: "application/json" };
  if (isBearer) {
    headers.Authorization = `Bearer ${token}`;
  }
  const response = await fetchImpl(url, { headers, signal: abort });
  if (!response.ok) return { outcome: "http" };
  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    // A 200 whose body is not JSON is a parse failure, not a network one.
    return { outcome: "parse" };
  }
  const features = mapTrendingPayload(payload);
  // A body that parses but yields nothing usable is the "unusable answer" case
  // the http branch already describes; only an unreadable body is "parse".
  return features != null && features.length > 0
    ? { outcome: "ok", features }
    : { outcome: "http" };
}

type CachedFeed = { at: number; features: TmdbFeature[] };

function readCache(storage: TmdbStorage | null): CachedFeed | null {
  if (storage == null) return null;
  try {
    const raw = storage.getItem(CACHE_KEY);
    if (raw == null) return null;
    const parsed = JSON.parse(raw) as Partial<CachedFeed>;
    if (
      typeof parsed.at === "number" &&
      Number.isFinite(parsed.at) &&
      Array.isArray(parsed.features) &&
      parsed.features.length > 0 &&
      parsed.features.every((feature) => {
        const record = feature as Partial<TmdbFeature> | null | undefined;
        return (
          typeof record?.title === "string" &&
          typeof record.from === "string" &&
          (record.posterUrl == null || typeof record.posterUrl === "string")
        );
      })
    ) {
      return { at: parsed.at, features: parsed.features };
    }
  } catch {
    /* corrupt cache — ignore */
  }
  return null;
}

/**
 * The feed used by the hero. Delivers synchronously (cached feed or the
 * bundled fallback), then upgrades to a fresh TMDB fetch when the token is
 * configured and the cache is stale. Never rejects, never throws, never
 * blocks: no token → no network at all.
 *
 * Returns a stop function (cancels the in-flight fetch).
 */
export function loadTmdbTrending(
  onTrending: (features: TmdbFeature[]) => void,
  deps?: { storage?: TmdbStorage | null; fetchImpl?: typeof fetch; now?: () => number },
): () => void {
  const storage = deps?.storage !== undefined ? deps.storage : browserStorage();
  const fetchImpl = deps?.fetchImpl ?? (typeof fetch === "function" ? fetch : undefined);
  const now = deps?.now ?? ((): number => new Date().getTime());
  const token = resolveTmdbToken(storage);

  const deliver = (features: TmdbFeature[]): void => {
    if (features.length > 0) {
      onTrending(features);
    }
  };

  // No credential configured → the shipped wall, no network at all.
  if (token.length === 0) {
    const cached = readCache(storage);
    deliver(cached?.features ?? FALLBACK_FEATURES);
    return () => undefined;
  }

  const cached = readCache(storage);
  if (cached != null && now() - cached.at < CACHE_TTL_MS) {
    // Fresh enough — deliver the cache, no fetch this round.
    deliver(cached.features);
    return () => undefined;
  }
  if (cached != null) {
    deliver(cached.features); // stale cache shows while refreshing
  } else {
    deliver(FALLBACK_FEATURES);
  }

  if (fetchImpl == null) {
    return () => undefined;
  }

  let cancelled = false;
  const controller = new AbortController();
  const timeout = setTimeout(() => {
    controller.abort();
  }, FETCH_TIMEOUT_MS);
  // One immediate retry on a network-level failure: flaky middleboxes
  // (seen in the field: ISP packet filters resetting TMDB connections)
  // often clear on the second attempt, and the hero degrades gracefully
  // anyway, so a single cheap retry is a good trade.
  const attemptWithRetry = async (): Promise<TrendingFetch> => {
    try {
      return await fetchTrending(token, fetchImpl, controller.signal);
    } catch (error) {
      if (cancelled || controller.signal.aborted) throw error;
      return await fetchTrending(token, fetchImpl, controller.signal);
    }
  };

  void attemptWithRetry()
    .then((result) => {
      clearTimeout(timeout);
      if (cancelled) return;
      if (result.outcome !== "ok") {
        // Reached TMDB but the answer was unusable — reported as the kind it
        // actually was: "http" for a non-200 or an empty payload, "parse" for
        // a 200 we could not read (F42).
        writeTmdbStatus(storage, {
          lastSuccessAtMs: readTmdbStatus(storage)?.lastSuccessAtMs ?? null,
          lastAttemptAtMs: now(),
          lastError: result.outcome,
        });
        return;
      }
      deliver(result.features);
      writeTmdbStatus(storage, {
        lastSuccessAtMs: now(),
        lastAttemptAtMs: now(),
        lastError: null,
      });
      if (storage != null) {
        try {
          const entry: CachedFeed = { at: now(), features: result.features };
          storage.setItem(CACHE_KEY, JSON.stringify(entry));
        } catch {
          /* storage full — the session still shows the live wall */
        }
      }
    })
    .catch(() => {
      clearTimeout(timeout);
      /* offline / rejected: the cached or bundled wall is already shown */
      writeTmdbStatus(storage, {
        lastSuccessAtMs: readTmdbStatus(storage)?.lastSuccessAtMs ?? null,
        lastAttemptAtMs: now(),
        lastError: "network",
      });
    });

  return () => {
    cancelled = true;
    clearTimeout(timeout);
    controller.abort();
  };
}

/** Cache age in ms (testable; null when absent/corrupt). */
export function cacheAgeMs(storage: TmdbStorage | null, now: () => number = () => new Date().getTime()): number | null {
  const cached = readCache(storage);
  return cached == null ? null : now() - cached.at;
}

export type TmdbFeedStatus = {
  /** Unix ms of the last COMPLETED live fetch (null = never succeeded). */
  lastSuccessAtMs: number | null;
  /** Unix ms of the last FAILED live fetch attempt (null = never tried). */
  lastAttemptAtMs: number | null;
  /** The failed attempt's rough cause, when known. */
  lastError: "network" | "http" | "parse" | null;
};

/** Read the persisted last-attempt outcome (Settings diagnostics). */
export function readTmdbStatus(
  storage: TmdbStorage | null = browserStorage(),
): TmdbFeedStatus | null {
  if (storage == null) return null;
  try {
    const raw = storage.getItem(STATUS_KEY);
    if (raw == null) return null;
    const parsed = JSON.parse(raw) as Partial<TmdbFeedStatus>;
    if (typeof parsed.lastAttemptAtMs !== "number") return null;
    return {
      lastSuccessAtMs: typeof parsed.lastSuccessAtMs === "number" ? parsed.lastSuccessAtMs : null,
      lastAttemptAtMs: parsed.lastAttemptAtMs,
      lastError:
        parsed.lastError === "network" || parsed.lastError === "http" || parsed.lastError === "parse"
          ? parsed.lastError
          : null,
    };
  } catch {
    return null;
  }
}

/** Record a fetch attempt's outcome (best-effort; storage errors ignored). */
function writeTmdbStatus(
  storage: TmdbStorage | null,
  status: TmdbFeedStatus,
): void {
  if (storage == null) return;
  try {
    storage.setItem(STATUS_KEY, JSON.stringify(status));
  } catch {
    /* ignore */
  }
}

/** Clear the cached feed (used when the token changes/clears). */
export function clearTmdbCache(storage: TmdbStorage | null): void {
  if (storage == null) return;
  try {
    storage.removeItem(CACHE_KEY);
    storage.removeItem(STATUS_KEY);
  } catch {
    /* ignore */
  }
}
