import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

/**
 * Batch 11 — the contracts that are not about a single pure function.
 *
 *  4. "Back to Lobby" performs the existing correct teardown/navigation path.
 *  5. The deleted `shared_pipeline.rs` has no live references.
 *  6. Provider Shared remains disabled.
 *
 * Reading files from a frontend test is established practice in this repo —
 * `src/config/cspPolicy.test.ts` reads `src-tauri/tauri.conf.json` — and it is
 * the only way to assert a *repository* invariant from either suite.
 */

/**
 * Strip line comments and block comments from source text.
 *
 * Used to tell a *live* reference apart from a documentation mention, in both
 * the Rust sources and the TSX sources this file inspects. Without it, a
 * comment that merely *names* the thing being forbidden counts as a violation —
 * which produced a false positive the first time this file ran, on the finished
 * card's own comment explaining why it does not use `onLeave`.
 *
 * Deliberately naive about `//` inside string literals: for every needle used
 * here a string literal containing one would itself be a live reference, so
 * treating it as live is the conservative direction. Block comments are removed
 * first so a `//` inside one cannot leave residue behind.
 */
function stripComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "");
}

// ── 4. the Back to Lobby navigation path ───────────────────────────────

/**
 * `backToLobby` must reach the backend `back_to_lobby` command, which is the
 * documented non-destructive retreat: it retracts readiness, clears any pending
 * countdown and returns `screen` to `LOBBY`, which unmounts Cinema and runs the
 * existing native-surface detach.
 *
 * The risk this guards is concrete and has happened before in this codebase:
 * the media-missing screen's "Back to lobby" button called `onLeave`, which
 * raises the destructive Leave / End-for-everyone confirmation behind a
 * non-destructive label (F14). The finished card must not repeat that.
 */
const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const { backToLobby, leaveParty } = await import("../backend/appRuntime");

describe("AUD-16 / 4 — Back to Lobby uses the non-destructive retreat", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({ screen: "LOBBY" });
  });

  it("invokes back_to_lobby", async () => {
    await backToLobby();

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("back_to_lobby", undefined);
  });

  it("does not take the destructive leave path", async () => {
    await backToLobby();

    expect(invokeMock).not.toHaveBeenCalledWith("leave_party", undefined);
    expect(invokeMock).not.toHaveBeenCalledWith("request_end_party", undefined);
  });

  /**
   * Negative control: the two paths are genuinely different commands, so the
   * assertions above can distinguish them. Without this, a typo'd expectation
   * would pass against any command at all.
   */
  it("negative control: leave_party is a different command", async () => {
    await leaveParty();

    expect(invokeMock).toHaveBeenCalledWith("leave_party", undefined);
    expect(invokeMock).not.toHaveBeenCalledWith("back_to_lobby", undefined);
  });
});

/**
 * The wiring the pure decisions cannot reach.
 *
 * `cinemaEndState.test.ts` proves the *decisions* are right. It cannot prove
 * CinemaView asks for them, or that the dock honours the answer — the repo has
 * no DOM test environment, and adding one for a single batch is not warranted.
 *
 * This gap is not hypothetical: while writing this batch, an edit that added
 * `playbackDisabled={...}` to CinemaView was silently lost, and the only thing
 * that caught it was eslint reporting an unused import. These assertions fail
 * on exactly that mistake.
 */
describe("AUD-16 / 3 — the dock wiring that the decisions cannot cover", () => {
  const cinemaViewSource = (): string =>
    readFileSync(resolve(__dirname, "CinemaView.tsx"), "utf8");
  const controlsSource = (): string =>
    readFileSync(resolve(__dirname, "../components/mp/CinemaControls.tsx"), "utf8");

  it("CinemaView asks the decision, rather than comparing the state inline", () => {
    expect(cinemaViewSource()).toContain(
      "playbackDisabled={!playbackControlsEnabled(snapshot.sync.roomState)}",
    );
  });

  it("the dock applies a real disabled attribute to play and both seek buttons", () => {
    const source = controlsSource();
    // One on the play button and one on each of the two seek buttons. A style
    // alone would not stop the handler; `disabled` does.
    expect(source.match(/disabled=\{playbackDisabled\}/g) ?? []).toHaveLength(3);
  });

  it("the finished card's action is backToLobby, never the destructive onLeave", () => {
    const source = cinemaViewSource();
    const start = source.indexOf("<MovieFinishedOverlay");
    expect(start).toBeGreaterThan(-1);
    // Comments are stripped: the block's own comment explains why it does NOT
    // use `onLeave`, and matching that text made this assertion fail on correct
    // code the first time it ran.
    const block = stripComments(source.slice(start, source.indexOf("/>", start)));

    expect(block).toContain("backToLobby()");
    // `onLeave` raises the Leave / End-for-everyone confirmation — the F14 bug
    // where a "Back to lobby" label sat on top of a destructive action.
    expect(block).not.toContain("onLeave");
  });

  /**
   * Negative control: the two assertions above can distinguish. `onLeave` IS
   * present in this file — on the dock, where it belongs — so the absence
   * inside the finished card is a real property, not a string that never
   * appears anywhere.
   */
  it("negative control: onLeave does exist in the file, on the dock", () => {
    const source = cinemaViewSource();
    expect(source).toContain("onLeave={onLeave}");

    const dockStart = source.indexOf("<CinemaControls");
    const dockBlock = stripComments(source.slice(dockStart, source.indexOf("/>", dockStart)));
    expect(dockBlock).toContain("onLeave");
  });
});

// ── 5 & 6. repository invariants ───────────────────────────────────────

const SRC_TAURI = resolve(__dirname, "../../src-tauri/src");

/** Count live (non-comment) occurrences of `needle` in Rust source. */
function liveReferences(source: string, needle: string): number {
  return stripComments(source).split(needle).length - 1;
}

type RustSource = { path: string; source: string };

function rustSourcesUnder(dir: string): RustSource[] {
  const found: RustSource[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      found.push(...rustSourcesUnder(full));
    } else if (entry.isFile() && entry.name.endsWith(".rs")) {
      found.push({ path: relative(SRC_TAURI, full), source: readFileSync(full, "utf8") });
    }
  }
  return found;
}

/**
 * The crate is read ONCE and shared by every assertion below. The workspace
 * lives on an external volume where a full recursive read is seconds, not
 * milliseconds, and four independent walks made this file the slowest in the
 * suite for no benefit.
 */
let cachedSources: RustSource[] | null = null;

function allRustSources(): RustSource[] {
  cachedSources ??= rustSourcesUnder(SRC_TAURI);
  return cachedSources;
}

function rustSourcesIn(dirRelativeToSrc: string): RustSource[] {
  const prefix = `${dirRelativeToSrc}/`;
  return allRustSources().filter(({ path }) => path.startsWith(prefix));
}

/**
 * Warm the crate read once, on an explicit budget.
 *
 * Measured: the walk costs ~2.5 s idle but **~6.4 s under full-suite load**
 * (vitest runs files in parallel, and the workspace is on an external volume).
 * Vitest's default per-test timeout is 5 s, so whichever assertion triggered the
 * walk first was aborted mid-read and reported as a failure — while the read
 * itself completed and populated the cache (the next assertions ran in 1–3 ms).
 * A green single-file run hid it completely.
 *
 * So the cost is declared here rather than left to a default that does not
 * describe it. This is not a timeout raised to conceal a defect: the assertions
 * are pure scans with no timing semantics, and the failure mode being fixed is
 * "the machine was busy", not "the answer was wrong".
 */
beforeAll(() => {
  allRustSources();
}, 30_000);

describe("AUD-08 — the orphaned shared_pipeline.rs is deleted, not repaired", () => {
  /**
   * Negative controls for the checker itself, before it is trusted.
   *
   * A file-scanning test that cannot fail is worthless, so the two ways it
   * could be wrong are pinned: it must DETECT a live declaration, and it must
   * IGNORE a comment-only mention (otherwise the checker would be "any mention
   * at all", which would fail on the documentation this batch deliberately
   * keeps).
   */
  it("negative control: the checker detects a live declaration", () => {
    expect(liveReferences("pub mod shared_pipeline;", "shared_pipeline")).toBe(1);
    expect(
      liveReferences("use crate::media::shared_pipeline::run_macos_shared_pipeline_proof;", "shared_pipeline"),
    ).toBe(2);
  });

  it("negative control: the checker ignores comment-only mentions", () => {
    expect(liveReferences("// shared_pipeline.rs was deleted", "shared_pipeline")).toBe(0);
    expect(liveReferences("/* shared_pipeline */\npub mod shared_stream;", "shared_pipeline")).toBe(0);
  });

  it("the file no longer exists", () => {
    expect(existsSync(resolve(SRC_TAURI, "media/shared_pipeline.rs"))).toBe(false);
  });

  it("is not declared as a module", () => {
    const modSource = readFileSync(resolve(SRC_TAURI, "media/mod.rs"), "utf8");
    expect(liveReferences(modSource, "mod shared_pipeline")).toBe(0);
  });

  it("has no live reference anywhere in the crate", () => {
    const offenders = allRustSources()
      .filter(({ source }) => liveReferences(source, "shared_pipeline") > 0)
      .map(({ path }) => path);

    expect(offenders).toEqual([]);
  });

  it("its obsolete transport API has no live reference either", () => {
    // `QuicClient::send_shared_stream_packet` was the API whose removal made
    // the file orphaned rather than stale. Nothing may call it.
    const offenders = allRustSources()
      .filter(({ source }) => liveReferences(source, "send_shared_stream_packet") > 0)
      .map(({ path }) => path);

    expect(offenders).toEqual([]);
  });

  it("the audit is reading real source, not an empty set", () => {
    // Guards the failure mode where a wrong path yields zero files and every
    // "no offenders" assertion passes vacuously.
    const sources = allRustSources();
    expect(sources.length).toBeGreaterThan(50);
    expect(sources.map(({ path }) => path)).toContain(join("media", "mod.rs"));
  });
});

describe("AUD-08 / 6 — Provider Shared remains disabled", () => {
  it("nothing in the media layer can activate Provider Shared", () => {
    // The deleted orphan lived here, and this batch must not have turned the
    // media layer into a Provider Shared activation site. The capability flag
    // is only ever set to `true` by a *verified capture diagnostic*
    // (`capture/diagnostic.rs`), never by anything under `media/`.
    const offenders = rustSourcesIn("media")
      .filter(({ source }) => {
        const code = stripComments(source);
        return code.includes("shared_available: true") || code.includes("PROVIDER_SHARED");
      })
      .map(({ path }) => path);

    expect(offenders).toEqual([]);
  });

  it("the shipped capability list still declares Shared unavailable", () => {
    const source = readFileSync(resolve(SRC_TAURI, "providers/sync.rs"), "utf8");
    const code = stripComments(source);

    expect(code).toContain("shared_available: false");
    expect(code).not.toContain("shared_available: true");
  });

  /**
   * Negative control for the two scans above: a source that DOES enable Shared
   * is caught by exactly the predicate they use.
   */
  it("negative control: an activation site would be caught", () => {
    const wouldBeCaught = (source: string): boolean => {
      const code = stripComments(source);
      return code.includes("shared_available: true") || code.includes("PROVIDER_SHARED");
    };

    expect(wouldBeCaught("shared_available: true,")).toBe(true);
    expect(wouldBeCaught('let mode = "PROVIDER_SHARED";')).toBe(true);
    expect(wouldBeCaught("// shared_available: true\nshared_available: false,")).toBe(false);
  });
});
