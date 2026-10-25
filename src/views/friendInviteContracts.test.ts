import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

/**
 * Batch 8/9 left one acceptance path, and these are the repository invariants
 * that keep it that way.
 *
 * What went wrong before: the Friends screen handed out the *unsigned*
 * identity-only link (`{n, pk}`) while the acceptance verifier rejected exactly
 * that shape as `MP-FRIEND-002`, and the screen pre-validated pasted links with
 * its own parser — so a signed invitation was refused by the client before the
 * Rust verifier ever saw it. Both halves are pinned here.
 */

function code(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
}

describe("friend invitation integrity", () => {
  it("hands out the signed invitation, not the legacy identity card", () => {
    const view = code(readFileSync("src/views/FriendsView.tsx", "utf8"));
    expect(view).toContain("requestSignedFriendInvite(");
    expect(view).not.toContain("friendInviteLink(");
    expect(view).not.toContain("buildFriendInviteLink");
  });

  it("lets the Rust verifier decide whether a link is acceptable", () => {
    const view = code(readFileSync("src/views/FriendsView.tsx", "utf8"));
    expect(view).toContain("previewFriendInvite(");
    // No second parser in the screen: a local one rejected valid signed links.
    expect(view).not.toContain("parseFriendInvite");
    const deepLinks = code(readFileSync("src/invites/deepLinks.ts", "utf8"));
    expect(deepLinks).not.toContain("parseFriendInvite");
    expect(deepLinks).not.toContain("buildFriendInviteLink");
  });

  it("cannot reach a command that creates a friend without a decision", () => {
    const bridge = code(readFileSync("src/backend/appRuntime.ts", "utf8"));
    for (const retired of ['"add_friend"', '"accept_friend_invite"', '"friend_invite_link"']) {
      expect(bridge, `${retired} must stay retired`).not.toContain(retired);
    }
    // The consent-correct pair is what remains.
    expect(bridge).toContain('invoke<IncomingInvite>("preview_friend_invite"');
    expect(bridge).toContain('invoke<StoredFriend>("accept_friend_invitation"');
  });
});
