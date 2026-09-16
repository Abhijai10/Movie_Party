import { describe, expect, it } from "vitest";
import {
  droppedPathErrorMessage,
  isSupportedMediaFile,
  mediaFileNameFromPath,
  usableDroppedPath,
} from "./createPartySource";

describe("createPartySource — drag/drop selection logic (F3)", () => {
  describe("isSupportedMediaFile", () => {
    it("accepts the extensions libmpv is handed", () => {
      for (const name of ["a.mp4", "b.MKV", "c.mov", "d.webm"]) {
        expect(isSupportedMediaFile(name)).toBe(true);
      }
    });

    it("rejects everything else, including look-alikes", () => {
      for (const name of [
        "notes.txt",
        "clip.mp4.exe",
        "archive.zip",
        "movie",
        "movie.mp4.txt",
      ]) {
        expect(isSupportedMediaFile(name)).toBe(false);
      }
    });

    it("ignores surrounding whitespace", () => {
      expect(isSupportedMediaFile("  film.mkv  ")).toBe(true);
    });
  });

  describe("mediaFileNameFromPath", () => {
    it("returns the last segment for either separator", () => {
      expect(mediaFileNameFromPath("/Users/me/Movies/Dune.mkv")).toBe("Dune.mkv");
      expect(mediaFileNameFromPath("C:\\Movies\\Dune.mkv")).toBe("Dune.mkv");
      expect(mediaFileNameFromPath("Dune.mkv")).toBe("Dune.mkv");
    });

    it("returns an empty string for empty input", () => {
      expect(mediaFileNameFromPath("")).toBe("");
      expect(mediaFileNameFromPath("   ")).toBe("");
    });
  });

  describe("usableDroppedPath", () => {
    it("returns a playable path", () => {
      expect(usableDroppedPath(["/Users/me/Movies/Dune.mkv"])).toBe(
        "/Users/me/Movies/Dune.mkv",
      );
    });

    it("skips unusable entries and takes the first playable one", () => {
      expect(
        usableDroppedPath(["/tmp/notes.txt", "/tmp/Dune.mov", "/tmp/other.mp4"]),
      ).toBe("/tmp/Dune.mov");
    });

    it("returns null when nothing can be played", () => {
      expect(usableDroppedPath([])).toBeNull();
      expect(usableDroppedPath(["/tmp/notes.txt"])).toBeNull();
      expect(usableDroppedPath(["/tmp/a.zip", "/tmp/b.rar"])).toBeNull();
    });

    it("returns null for a path with no file component", () => {
      expect(usableDroppedPath(["/Users/me/Movies/"])).toBeNull();
    });
  });

  describe("droppedPathErrorMessage", () => {
    it("explains an empty drop", () => {
      expect(droppedPathErrorMessage([])).toMatch(/did not include a file/i);
    });

    it("names the offending file and the supported extensions", () => {
      const message = droppedPathErrorMessage(["/tmp/notes.txt"]);
      expect(message).toContain("notes.txt");
      expect(message).toContain("MP4, MKV, MOV or WebM");
    });
  });

  it("a dropped path is exactly what the backend receives", () => {
    // The regression: the old code stored a browser `File` (which has no
    // path) and still rendered "Selected", so create() sent mediaPath: null.
    // A usable selection is now a non-empty path by construction.
    const dropped = usableDroppedPath(["/Users/me/Movies/Dune.mkv"]);
    expect(dropped).not.toBeNull();
    expect(dropped?.trim().length).toBeGreaterThan(0);
  });
});
