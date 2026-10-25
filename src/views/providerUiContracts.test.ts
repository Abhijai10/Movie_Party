import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

/**
 * The provider and YouTube surfaces must speak with one control voice.
 *
 * Batch 5 shipped a player whose panels mixed the app's `CinemaButton` pill with
 * plain `<button>` elements that CSS drew as rectangular, sentence-case boxes —
 * two button languages inside the same panel. These assertions are repository
 * invariants, so they belong in the suite that reads source files rather than
 * in a component test.
 */

/** Comments may name the forbidden thing without violating the rule. */
function code(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
}

const CONTROLS = [
  "src/views/ProviderContentChooser.tsx",
  "src/views/ProviderSessionReset.tsx",
  "src/youtube/YoutubePanel.tsx",
  "src/providers/ProviderRoomStatus.tsx",
  "src/providers/ProviderGestureControl.tsx",
];

describe("provider and YouTube control uniformity", () => {
  it.each(CONTROLS)("%s drives every control through CinemaButton", (file) => {
    const source = code(readFileSync(file, "utf8"));
    expect(source).not.toContain("<button");
    expect(source).toContain("CinemaButton");
  });

  it("no rule redraws a provider-chooser control as a competing rectangle", () => {
    const css = readFileSync("src/styles.css", "utf8");
    const competing = [...css.matchAll(/\.provider-chooser[^{]*button[^{]*\{[^}]*\}/g)].filter(
      (rule) => /border-radius|background:\s*#/.test(rule[0]),
    );
    expect(competing).toEqual([]);
  });
});
