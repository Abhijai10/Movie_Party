---

### Downloads

| Platform | File |
|---|---|
| macOS (Apple Silicon) | the `.dmg` in the assets below |
| Windows 11 x64 | the `x64-setup.exe` in the assets below |

**Tailscale is not bundled.** Install it separately from <https://tailscale.com/download> and sign in
on both machines before creating or joining a party.

### macOS first launch — read this before reporting a broken download

The macOS build is **ad-hoc signed and NOT notarized**. It has no Developer ID signature and no
Apple notarization ticket, so **Gatekeeper will refuse the first launch**. Depending on the macOS
version the wording is one of:

* *"Movie Party" is damaged and can't be opened. You should move it to the Trash.*
* *"Movie Party" cannot be opened because Apple cannot check it for malicious software.*
* *Apple could not verify "Movie Party" is free of malware.*

**None of these mean the download is corrupted or that the app is unsafe.** They mean Apple has not
vetted this build. To open it:

1. Drag **Movie Party** into **Applications**.
2. **Control-click** (or right-click) the app → **Open**.
3. In the dialog, click **Open** again.

After that first launch it opens normally, including by double-click.

Alternatively, clear the quarantine flag from a terminal:

```bash
xattr -d com.apple.quarantine "/Applications/Movie Party.app"
```

Do **not** disable Gatekeeper system-wide to work around this. The README carries the same
walkthrough; it is the authoritative version if the two ever disagree.

### Windows first launch — read this before reporting a broken download

The Windows installer is **not code-signed**. There is no Authenticode certificate, because that
requires a paid code-signing certificate. Windows therefore shows **SmartScreen** the first time the
installer is run:

> **Windows protected your PC** — Microsoft Defender SmartScreen prevented an unrecognized app from
> starting. Running this app might put your PC at risk.

**This does not mean the download is corrupted, and it is not a build failure.** It means Microsoft
has never seen this file before. To install it:

1. Click **More info** (the link inside the dialog).
2. Click **Run anyway**.
3. Complete the installer normally.

You may see a second, similar prompt from User Account Control; the installer runs per-user
(`installMode: currentUser`), so it does not require administrator rights.

If Windows *deletes* the file instead of prompting, check **Windows Security → Protection history**
and restore it, then repeat the steps above. Do not disable SmartScreen or Defender system-wide.

The same caveat applies to the installed app itself: it is unsigned, so the first launch may prompt
again. This is the Windows equivalent of the macOS Gatekeeper section above, and it is the same
trade-off — the project has no paid signing certificates for either platform.

### Updates — there are none, by design

Movie Party has **no update mechanism**. There is no Tauri updater plugin, no update signing key, and
no `latest.json` manifest. The app never checks for updates and never installs one. **To update,
download the new release manually** and replace the app.

`Movie.Party_aarch64.app.tar.gz` is a plain compressed copy of the `.app` bundle, for people who want
the raw bundle without mounting the `.dmg`. It is **not** an updater artifact, it is not signed, and
nothing consumes it as one.
