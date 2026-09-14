import { describe, expect, it, vi } from "vitest";
import {
  BUNDLED_TMDB_TOKEN,
  FALLBACK_FEATURES,
  cacheAgeMs,
  clearTmdbCache,
  getTmdbToken,
  loadTmdbTrending,
  mapTrendingPayload,
  readTmdbStatus,
  resolveTmdbToken,
  setTmdbToken,
  type TmdbStorage,
} from "./tmdbFeed";

function memoryStorage(initial: Record<string, string> = {}): TmdbStorage {
  const store = new Map<string, string>(Object.entries(initial));
  return {
    getItem: (key) => store.get(key) ?? null,
    setItem: (key, value) => {
      store.set(key, value);
    },
    removeItem: (key) => {
      store.delete(key);
    },
  };
}

const tokenStorage = (token: string): TmdbStorage =>
  memoryStorage(token.length > 0 ? { mp_tmdb_token: token } : {});

const jsonOk = (body: unknown): Response =>
  new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });

describe("mapTrendingPayload", () => {
  it("maps a valid trending payload to hero features", () => {
    const features = mapTrendingPayload({
      results: [
        {
          title: "Dune: Part Two",
          release_date: "2024-02-27",
          overview: "A mythic journey of explosive revenge.",
          poster_path: "/1i5N4ykrfE7nVaE5y0ZrrE9O5V5.jpg",
          backdrop_path: "/xOMo8BRK7PfcNv9sXK6p1IYmU2e.jpg",
        },
        { title: "   ", overview: "dropped — blank title" },
      ],
    });
    expect(features).not.toBeNull();
    expect(features).toHaveLength(1);
    expect(features?.[0]?.title).toBe("Dune: Part Two");
    expect(features?.[0]?.year).toBe("2024");
    expect(features?.[0]?.posterUrl).toContain("https://image.tmdb.org/t/p/w500");
    expect(features?.[0]?.backdropUrl).toContain("https://image.tmdb.org/t/p/w780");
  });

  it("returns null for shapes the hero cannot use", () => {
    expect(mapTrendingPayload(null)).toBeNull();
    expect(mapTrendingPayload({})).toBeNull();
    expect(mapTrendingPayload({ results: "nope" })).toBeNull();
    expect(mapTrendingPayload({ results: [{ overview: "no title" }] })).toBeNull();
  });

  it("keeps long overviews to a readable tagline length", () => {
    const long = "word ".repeat(60).trim();
    const features = mapTrendingPayload({
      results: [{ title: "T", overview: long, release_date: "2020-01-01" }],
    });
    expect(features?.[0]?.tagline.length ?? 0).toBeLessThanOrEqual(90);
    expect(features?.[0]?.tagline.endsWith("…")).toBe(true);
  });

  it("caps the wall at the bundled feature count", () => {
    const many = Array.from({ length: 20 }, (_, i) => ({ title: `Movie ${String(i)}` }));
    const features = mapTrendingPayload({ results: many });
    expect(features).toHaveLength(FALLBACK_FEATURES.length);
  });

  it("marks unknown years honestly", () => {
    const features = mapTrendingPayload({ results: [{ title: "T" }] });
    expect(features?.[0]?.year).toBe("—");
    expect(features?.[0]?.tagline).toBe("");
  });
});

describe("token storage", () => {
  it("saves, reads, and clears the token", () => {
    const storage = memoryStorage();
    setTmdbToken(storage, "  abc123  ");
    expect(getTmdbToken(storage)).toBe("abc123");
    setTmdbToken(storage, "");
    expect(getTmdbToken(storage)).toBe("");
  });

  it("tolerates absent storage", () => {
    setTmdbToken(null, "x");
    expect(getTmdbToken(null)).toBe("");
  });
});

describe("shared build-time default (resolveTmdbToken)", () => {
  it("uses the pasted key when present, the bundled default otherwise", () => {
    // Pasted key (Settings → General) always beats the shared build-time key.
    const pasted = memoryStorage({ mp_tmdb_token: "  my-own-key  " });
    expect(resolveTmdbToken(pasted)).toBe("my-own-key");

    // No pasted key → the build-time default from .env / the CI secret.
    expect(resolveTmdbToken(memoryStorage())).toBe(BUNDLED_TMDB_TOKEN);

    // Unreachable/absent storage still resolves to the bundled default
    // when one ships with the build (empty in the test env, which is the
    // no-secret-committed guarantee).
    expect(resolveTmdbToken(null)).toBe(BUNDLED_TMDB_TOKEN);
  });

  it("falls back to the shipped wall only when neither exists", () => {
    // In the test environment no VITE_TMDB_TOKEN is set, so with no
    // pasted key the feed must deliver the bundled fallback wall without
    // any network call — the same guarantee a build WITHOUT the secret
    // keeps at runtime.
    const seen: string[][] = [];
    loadTmdbTrending((features) => seen.push(features.map((f) => f.title)), {
      storage: memoryStorage(),
      fetchImpl: (() => {
        throw new Error("network must not be touched without a credential");
      }) as unknown as typeof fetch,
    });
    expect(seen[0]).toEqual(FALLBACK_FEATURES.map((f) => f.title));
  });
});

describe("loadTmdbTrending", () => {
  const flush = () => new Promise((resolve) => setTimeout(resolve, 5));

  it("delivers the bundled fallback wall without a token and never fetches", async () => {
    const fetchImpl = vi.fn();
    const seen: string[] = [];
    loadTmdbTrending((features) => seen.push(features[0]?.title ?? ""), {
      storage: memoryStorage(),
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    expect(seen).toEqual([FALLBACK_FEATURES[0]?.title]);
    await flush();
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("uses a fresh cache without re-fetching", async () => {
    const fetchImpl = vi.fn();
    const storage = tokenStorage("tok");
    storage.setItem(
      "mp_tmdb_trending_v1",
      JSON.stringify({ at: Date.now(), features: [{ title: "Cached Hit", year: "2024", tagline: "", posterUrl: null, backdropUrl: null, from: "#000", to: "#111", accent: "#fff" }] }),
    );
    const seen: string[] = [];
    loadTmdbTrending((features) => seen.push(features[0]?.title ?? ""), {
      storage,
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    expect(seen).toEqual(["Cached Hit"]);
    await flush();
    expect(fetchImpl).not.toHaveBeenCalled();
    expect(cacheAgeMs(storage)).toBeLessThan(1000);
  });

  it("shows the bundled wall while fetching, then upgrades on success and caches", async () => {
    const fetchImpl = vi.fn().mockResolvedValue(jsonOk({ results: [{ title: "Live Trend" }] }));
    const storage = tokenStorage("tok");
    const seen: string[] = [];
    loadTmdbTrending((features) => seen.push(features[0]?.title ?? ""), {
      storage,
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    expect(seen).toEqual([FALLBACK_FEATURES[0]?.title]);
    await vi.waitFor(() => {
      expect(seen.at(-1)).toBe("Live Trend");
    });
    // the live wall is cached for the next session
    expect(storage.getItem("mp_tmdb_trending_v1")).toContain("Live Trend");
  });

  it("keeps the last good wall when the network fails", async () => {
    const fetchImpl = vi.fn().mockRejectedValue(new Error("offline"));
    const storage = tokenStorage("tok");
    const seen: string[] = [];
    loadTmdbTrending((features) => seen.push(features[0]?.title ?? ""), {
      storage,
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    expect(seen).toEqual([FALLBACK_FEATURES[0]?.title]);
    await vi.waitFor(() => {
      expect(fetchImpl).toHaveBeenCalledTimes(1);
    });
    await flush();
    expect(seen).toEqual([FALLBACK_FEATURES[0]?.title]); // unchanged, no crash
  });

  it("keeps the last good wall when TMDB answers non-200", async () => {
    const fetchImpl = vi.fn().mockResolvedValue(new Response("nope", { status: 401 }));
    const storage = tokenStorage("tok");
    const seen: string[] = [];
    loadTmdbTrending((features) => seen.push(features[0]?.title ?? ""), {
      storage,
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    await vi.waitFor(() => {
      expect(fetchImpl).toHaveBeenCalledTimes(1);
    });
    await flush();
    expect(seen).toEqual([FALLBACK_FEATURES[0]?.title]);
  });

  it("sends the key as api_key for v3 keys and Bearer for v4 tokens", async () => {
    const fetchImpl = vi.fn().mockResolvedValue(jsonOk({ results: [{ title: "X" }] }));
    loadTmdbTrending(() => undefined, {
      storage: tokenStorage("plainv3key"),
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    await vi.waitFor(() => {
      expect(fetchImpl).toHaveBeenCalledTimes(1);
    });
    expect(String((fetchImpl.mock.calls[0] as unknown[])[0])).toContain("api_key=plainv3key");
    expect((fetchImpl.mock.calls[0] as unknown[])[1]).not.toHaveProperty("headers.Authorization");

    const fetchBearer = vi.fn().mockResolvedValue(jsonOk({ results: [{ title: "X" }] }));
    loadTmdbTrending(() => undefined, {
      storage: tokenStorage("eyJfake.jwt.token"),
      fetchImpl: fetchBearer as unknown as typeof fetch,
    });
    await vi.waitFor(() => {
      expect(fetchBearer).toHaveBeenCalledTimes(1);
    });
    const options = fetchBearer.mock.calls[0]?.[1] as RequestInit;
    expect(String((fetchBearer.mock.calls[0] as unknown[])[0])).not.toContain("api_key");
    expect((options.headers as Record<string, string>)["Authorization"]).toBe(
      "Bearer eyJfake.jwt.token",
    );
  });

  it("a stale cache still shows while a refresh runs", async () => {
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(jsonOk({ results: [{ title: "Fresh Now" }] }));
    const storage = tokenStorage("tok");
    const staleAt = Date.now() - 2 * 60 * 60 * 1000; // 2h old
    storage.setItem(
      "mp_tmdb_trending_v1",
      JSON.stringify({ at: staleAt, features: [{ title: "Stale Wall", year: "2020", tagline: "", posterUrl: null, backdropUrl: null, from: "#000", to: "#111", accent: "#fff" }] }),
    );
    const seen: string[] = [];
    loadTmdbTrending((features) => seen.push(features[0]?.title ?? ""), {
      storage,
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    expect(seen[0]).toBe("Stale Wall"); // stale wall shows immediately
    await vi.waitFor(() => {
      expect(seen.at(-1)).toBe("Fresh Now");
    });
  });

  it("clearTmdbCache removes the cached feed and cacheAgeMs handles corrupt input", () => {
    const storage = memoryStorage();
    storage.setItem("mp_tmdb_trending_v1", "{oops");
    expect(cacheAgeMs(storage)).toBeNull();
    clearTmdbCache(storage);
    expect(storage.getItem("mp_tmdb_trending_v1")).toBeNull();
  });

  it("records a successful fetch in the status diagnostics", async () => {
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(jsonOk({ results: [{ title: "Live Winner" }] }));
    const storage = tokenStorage("tok");
    loadTmdbTrending(() => undefined, { storage, fetchImpl: fetchImpl as unknown as typeof fetch });
    await vi.waitFor(() => {
      const status = readTmdbStatus(storage);
      expect(status).not.toBeNull();
      expect(status?.lastError).toBeNull();
      expect(status?.lastSuccessAtMs).not.toBeNull();
      expect(status?.lastAttemptAtMs).toBe(status?.lastSuccessAtMs);
    });
    clearTmdbCache(storage);
    expect(readTmdbStatus(storage)).toBeNull();
  });

  it("records a network failure (unreachable TMDB) in the status diagnostics", async () => {
    const fetchImpl = vi.fn().mockRejectedValueOnce(new TypeError("Connection reset"));
    const storage = tokenStorage("tok");
    loadTmdbTrending(() => undefined, { storage, fetchImpl: fetchImpl as unknown as typeof fetch });
    await vi.waitFor(() => {
      const status = readTmdbStatus(storage);
      expect(status).not.toBeNull();
      expect(status?.lastError).toBe("network");
      expect(status?.lastSuccessAtMs).toBeNull();
    });
  });

  it("records an unusable response (non-200) in the status diagnostics", async () => {
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(new Response("nope", { status: 401 }));
    const storage = tokenStorage("tok");
    loadTmdbTrending(() => undefined, { storage, fetchImpl: fetchImpl as unknown as typeof fetch });
    await vi.waitFor(() => {
      const status = readTmdbStatus(storage);
      expect(status).not.toBeNull();
      expect(status?.lastError).toBe("http");
      expect(status?.lastSuccessAtMs).toBeNull();
    });
  });

  it("readTmdbStatus tolerates corrupt/absent records", () => {
    const storage = memoryStorage();
    expect(readTmdbStatus(storage)).toBeNull();
    storage.setItem("mp_tmdb_status_v1", "{oops");
    expect(readTmdbStatus(storage)).toBeNull();
    storage.setItem("mp_tmdb_status_v1", JSON.stringify({ lastAttemptAtMs: 5, lastError: "weird" }));
    expect(readTmdbStatus(storage)?.lastError).toBeNull();
    expect(readTmdbStatus(null)).toBeNull();
  });
});
