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

### Updates — there are none, by design

Movie Party has **no update mechanism**. There is no Tauri updater plugin, no update signing key, and
no `latest.json` manifest. The app never checks for updates and never installs one. **To update,
download the new release manually** and replace the app.

`Movie.Party_aarch64.app.tar.gz` is a plain compressed copy of the `.app` bundle, for people who want
the raw bundle without mounting the `.dmg`. It is **not** an updater artifact, it is not signed, and
nothing consumes it as one.
