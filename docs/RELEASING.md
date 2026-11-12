# Releases and automatic updates

Updates use tauri-plugin-updater; releases are published on GitHub Releases.

## Overview

1. `npm run bump -- <patch|minor|major|x.y.z>` bumps the version, commits, and tags `v<version>`.
2. Pushing the tag runs `.github/workflows/release.yml`, which:
   - checks that the tag matches the version in all three files (`package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`);
   - type-checks, tests, builds the installer and signs it;
   - turns the commit subjects since the previous tag into the release notes (skipping version-bump commits, at most 20);
   - uploads the installer, its signature (`.sig`) and `latest.json` to a **draft release**, then publishes it.
3. The app checks
   `https://github.com/cignoir/terminal-conductor/releases/latest/download/latest.json`
   5 seconds after start and every 6 hours after that.

The draft step keeps "latest" from switching over before all files are in place: drafts and pre-releases are never `releases/latest`.

## In the app

| Step | What happens |
|---|---|
| Check | `updater_check`. If there is a newer version, the Settings button gets a red dot and Settings shows the version and its notes. Failures and "up to date" stay quiet |
| Download | `updater_download`. Only fetches the installer; installs nothing, restarts nothing. A failed download can be retried without checking again |
| Restart and update | Only when the user presses it. The frontend saves its documents, the backend sends Ctrl+C twice to every pane so Claude exits cleanly, then runs the installer. When it finishes the app starts again and each Claude resumes its conversation with `--resume` |

- Development builds (`tauri dev`) never check: comparing against the released version would offer to overwrite the installed app.
- The webview is not granted `updater:default`. With it, the webview could call download-and-install directly and skip the clean shutdown of Claude.

## One-time setup

### Signing key

- Private key: `~/.tauri/terminal-conductor-updater.key`, kept outside the repository. **Never commit or share it, and keep a backup**: without it no further update can be signed.
- Public key: embedded in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
- To regenerate: `npx tauri signer generate -w ~/.tauri/terminal-conductor-updater.key -p "" -f`, then put the new public key in `pubkey`. **Changing the key means existing installs can no longer update**; users would have to reinstall by hand.

### Repository

The update endpoint in `src-tauri/tauri.conf.json` (`plugins.updater.endpoints`) names `cignoir/terminal-conductor`. If you publish under another name, change it there and in the README links.

```sh
git init -b main
git add -A
git commit -m "feat: initial release"
gh repo create cignoir/terminal-conductor --public --source . --push
```

### GitHub secrets (Settings → Secrets and variables → Actions)

| Name | Value |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | The contents of `~/.tauri/terminal-conductor-updater.key` |

```sh
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/terminal-conductor-updater.key
```

The key has no password, so `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is not needed (signing was verified to work with the variable unset). The workflow uses `GITHUB_TOKEN` automatically (`contents: write`).

### First release

The version is already 0.1.0, so tag it directly instead of running `npm run bump`:

```sh
git tag v0.1.0
git push origin v0.1.0
```

When the workflow finishes, check that the release has `terminal-conductor_0.1.0_x64-setup.exe`, its `.sig`, and `latest.json`.

## Cutting a release

```sh
npm run bump -- minor            # 0.1.0 -> 0.2.0 (patch|minor|major|x.y.z)
npm run bump -- minor --push     # also push, which starts the release workflow
npm run bump -- --dry-run        # show what would change; touches nothing
```

- Run it on a clean working tree; it stops if there are uncommitted changes.
- Without `--push`: `git push origin HEAD && git push origin v<version>`.

## Trying the update flow locally

Release builds refuse non-https endpoints, so test builds allow http through a config override.

```sh
# 1. Build the older version signed, and install it
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/terminal-conductor-updater.key)"
npx tauri build --config '{"plugins":{"updater":{"dangerousInsecureTransportProtocol":true}}}'
target/release/bundle/nsis/terminal-conductor_<old>_x64-setup.exe /S

# 2. Temporarily bump the version, build the same way, and put the installer and .sig in a folder
# 3. Add latest.json there (url: http://127.0.0.1:8765/<installer name>) and serve it
python -m http.server 8765 --bind 127.0.0.1

# 4. Start the installed older version against the local endpoint
TC_UPDATE_ENDPOINT=http://127.0.0.1:8765/latest.json "$LOCALAPPDATA/terminal-conductor/terminal-conductor.exe"
```

Then go to Settings → Updates, press Download, then Restart and update, and confirm the new version starts. This was verified end to end with 0.1.0 → 0.1.1 on 2026-11-12, including Claude resuming its conversation after the update. That run used a local http server; updating from GitHub Releases, and the release workflow itself, are first exercised by the first two real releases (v0.1.0 and the one after).

## Not done yet

- Authenticode code signing (needs a certificate; without it SmartScreen warns)
- A staging channel for pre-releases
