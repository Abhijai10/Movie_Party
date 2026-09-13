import { describe, expect, it } from "vitest";
import {
  displayNameFor,
  fileNameFromPath,
  listRecentMedia,
  rememberMedia,
} from "./recentMedia";

type MemoryStorage = { getItem: (key: string) => string | null; setItem: (key: string, value: string) => void };

function memoryStorage(): MemoryStorage {
  const store = new Map<string, string>();
  return {
    getItem: (key) => store.get(key) ?? null,
    setItem: (key, value) => {
      store.set(key, value);
    },
  };
}

describe("fileNameFromPath", () => {
  it("extracts the filename from unix and windows paths", () => {
    expect(fileNameFromPath("/Users/you/Movies/Inception.mkv")).toBe("Inception.mkv");
    expect(fileNameFromPath("C:\\Movies\\Dune.mp4")).toBe("Dune.mp4");
    expect(fileNameFromPath("  plain-name.mkv ")).toBe("plain-name.mkv");
  });
});

describe("rememberMedia / listRecentMedia", () => {
  it("records picks newest-first, deduped by path", () => {
    const storage = memoryStorage();
    rememberMedia(storage, "/m/a.mkv");
    rememberMedia(storage, "/m/b.mkv");
    const afterRePick = rememberMedia(storage, "/m/a.mkv");
    expect(afterRePick.map((entry) => entry.path)).toEqual(["/m/a.mkv", "/m/b.mkv"]);
    expect(listRecentMedia(storage)).toHaveLength(2);
  });

  it("uses the provided display name and caps the list", () => {
    const storage = memoryStorage();
    for (let i = 0; i < 12; i += 1) {
      rememberMedia(storage, `/m/movie-${String(i)}.mkv`, `Movie ${String(i)}`);
    }
    const listed = listRecentMedia(storage);
    expect(listed).toHaveLength(8);
    expect(listed[0]?.name).toBe("Movie 11");
  });

  it("ignores blank paths and corrupt storage", () => {
    const storage = memoryStorage();
    rememberMedia(storage, "   ");
    expect(listRecentMedia(storage)).toHaveLength(0);
    const corrupt = {
      getItem: () => "{not json",
      setItem: () => undefined,
    };
    expect(listRecentMedia(corrupt)).toHaveLength(0);
  });

  it("tolerates null storage", () => {
    rememberMedia(null, "/m/x.mkv");
    expect(listRecentMedia(null)).toHaveLength(0);
  });
});

describe("displayNameFor", () => {
  it("resolves remembered names and falls back for real ids", () => {
    const recent = rememberMedia(memoryStorage(), "/m/big-buck-bunny.mkv", "Big Buck Bunny");
    expect(displayNameFor("/m/big-buck-bunny.mkv", recent, (id) => id)).toBe("Big Buck Bunny");
    expect(displayNameFor("real-media-id-123", recent, (id) => `media ${id}`)).toBe(
      "media real-media-id-123",
    );
  });
});
