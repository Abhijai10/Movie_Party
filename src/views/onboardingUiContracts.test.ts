import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { WIZARD_STAGES } from "../onboarding/wizard";

/**
 * Batch 9's wizard must speak with the same control voice as the rest of the
 * app, must never let the surface invent its own status words, and must have a
 * reachable state for every stage the reducer can return.
 *
 * These are repository invariants, so they read source files (the same approach
 * as `providerUiContracts.test.ts`) rather than mounting a DOM — the repo has no
 * component-render harness, and the behavioural coverage lives in
 * `src/onboarding/wizard.test.ts` plus the Rust reducer tests.
 */

/** Comments may name the forbidden thing without violating the rule. */
function code(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
}

const VIEW = "src/views/OnboardingWizardView.tsx";

describe("onboarding wizard control uniformity", () => {
  it("drives every control through CinemaButton", () => {
    const source = code(readFileSync(VIEW, "utf8"));
    expect(source).not.toContain("<button");
    expect(source).toContain("CinemaButton");
  });

  it("takes its status words from the model, never from itself", () => {
    const source = code(readFileSync(VIEW, "utf8"));
    // "Connected" is the model's claim, gated on a handshake proof. A literal
    // here would mean the surface could assert it on its own.
    expect(source).not.toMatch(/["'`]Connected["'`]/);
    for (const guard of [
      "connectednessCopy(",
      "handshakeEvidenceLine(",
      "stageActions(",
      "directionCopy(",
      "shareLinkSafetyCopy(",
      "topologyCopy(",
    ]) {
      expect(source, `${guard} must come from src/onboarding/wizard`).toContain(guard);
    }
  });

  it("never renders a pasted bearer link back to the screen", () => {
    const source = code(readFileSync(VIEW, "utf8"));
    // The paste field is cleared on submit and only the model's summary (which
    // carries a fingerprint, not a token) is displayed.
    expect(source).toContain('setLinkText("")');
    expect(source).toContain("shareLinkSafetyCopy");
  });
});

describe("onboarding CSS stays inside the shared system", () => {
  it("no rule redraws a wizard control as a competing rectangle", () => {
    const css = readFileSync("src/styles.css", "utf8");
    const competing = [...css.matchAll(/\.onboarding[^{]*button[^{]*\{[^}]*\}/g)].filter((rule) =>
      /border-radius|background:\s*#/.test(rule[0]),
    );
    expect(competing).toEqual([]);
  });
});

describe("requirement 18: every stage has a reachable UI state", () => {
  it("the browser fixture covers all six stages", () => {
    const fixture = readFileSync("src/test-ui/onboarding.fixture.tsx", "utf8");
    for (const stage of WIZARD_STAGES) {
      expect(fixture, `no fixture renders stage ${stage}`).toContain(`stage: "${stage}"`);
    }
  });

  it("the fixture also covers the states a healthy machine cannot produce", () => {
    const fixture = readFileSync("src/test-ui/onboarding.fixture.tsx", "utf8");
    for (const code of [
      "MP-ONB-001", // no Tailscale
      "MP-ONB-004", // expired node key
      "MP-ONB-008", // share pending
      "MP-ONB-011", // not a tailnet admin
      "MP-ONB-012", // wrong sharing direction
      "MP-ONB-013", // tunnel only, no handshake
      "MP-ONB-014", // blocked
    ]) {
      expect(fixture, `no fixture renders ${code}`).toContain(code);
    }
  });

  it("no fixture builds a checklist by appending overrides", () => {
    // The reducer emits exactly one row per check id. A fixture that spreads a
    // full list and then re-adds an id produced two rows for one id, and React
    // rendered both — a healthy state can then still show "MP-ONB-001".
    const fixture = readFileSync("src/test-ui/onboarding.fixture.tsx", "utf8");
    expect(fixture).not.toMatch(/\.\.\.checks\(/);
    expect(fixture).not.toContain("failedCheck(");
  });

  it("the routing table can reach the wizard", () => {
    const routing = readFileSync("src/components/appRouting.ts", "utf8");
    expect(routing).toContain('"ONBOARDING"');
    const shell = readFileSync("src/components/AppShell.tsx", "utf8");
    expect(shell).toContain('appScreen === "ONBOARDING"');
    expect(shell).toContain("<OnboardingWizardView");
  });
});
