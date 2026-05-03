# Release signing setup

Notes for the maintainer (Chris) on how the macOS Tier-2 signing +
notarization pipeline is wired and what GitHub repo settings need
to exist before the next tag-push will produce a fully-signed
`.dmg`. Also covers the security model so the answer to "can a fork
steal my Apple Developer keys" is documented in one place.

> **Status (PLAN 10.9.2):** code path landed; secrets pending
> creation. Until the secrets exist on the repo, the release lane
> still produces an unsigned `.tar.gz` portable artifact (which
> always works) and the signing/notarization steps fail loudly when
> they hit a missing secret. Once secrets are in place, every
> `v*.*.*` tag push produces a signed + notarized + stapled `.dmg`
> alongside the tarball.

## Security model — why fork PRs can't exfiltrate the keys

GitHub repo secrets are **never** exposed to workflows triggered by
`pull_request` from a fork. That's a platform guarantee — see
[GitHub Actions security guide].

`release.yml` only triggers on:

- `push` to tags matching `v*.*.*` — fork pushes don't reach this
  repo, so this only fires from upstream.
- `workflow_dispatch` — requires write access to the repo, forks
  can't dispatch upstream workflows.

Neither trigger is reachable from a fork. The classic
secrets-leak pattern (`pull_request_target` exposing secrets to
fork PRs) does not exist in this workflow.

Belt-and-suspenders defenses also in place:

1. **`environment: release`** on the macOS job. The `release`
   GitHub Environment scopes secrets so only jobs that target it
   can pull them, and deployment-branch restriction limits which
   refs can deploy. Configured in repo Settings → Environments →
   `release` (see [Environment setup](#environment-setup) below).
2. **`if: github.repository == 'chotchki/skylander-portal-controller'`**
   guard on every signing/notarization step. If the workflow ever
   runs in a fork's context (e.g. someone copies the file into
   their own repo), the steps no-op before touching any secret.
3. **App-specific password**, not the Apple ID password. Generated
   at <https://appleid.apple.com> → Sign-in and Security →
   App-Specific Passwords. Limited to Apple service auth and
   revocable independently of the main account.

## Environment setup

Repo Settings → Environments → New environment → name `release`.

Configure:

- **Deployment branches and tags:** Selected branches and tags →
  Add deployment tag rule → `v*.*.*`. (Belt-and-suspenders: even
  if `release.yml` ever gets a non-tag trigger, secrets in this
  environment only flow to runs on a matching ref.)
- **Required reviewers:** optional. Adding yourself adds a manual-
  approve gate before the macOS job runs — useful for catching a
  surprise tag-push, friction otherwise. Skip for now; flip on if
  the release cadence is slow enough that the friction is OK.

## Secrets to create

In the `release` environment (Settings → Environments → release →
Add environment secret):

| Name                       | Value                                                                 |
| -------------------------- | --------------------------------------------------------------------- |
| `MACOS_CERT_P12_BASE64`    | Developer ID Application certificate, exported as `.p12`, base64-encoded. See [export instructions](#exporting-the-developer-id-cert). |
| `MACOS_CERT_PASSWORD`      | Passphrase set during the `.p12` export.                              |
| `MACOS_CERT_IDENTITY`      | Cert common name, e.g. `Developer ID Application: Christopher Hotchkiss (ABCD123XYZ)`. Run `security find-identity -v -p codesigning` after import to find the exact string. |
| `KEYCHAIN_PASSWORD`        | Random string for the throwaway CI keychain. Generate with `openssl rand -base64 32`. Doesn't need to be memorable. |
| `APPLE_ID`                 | Apple Developer email (`chris@hotchkiss.io`).                         |
| `APPLE_APP_PASSWORD`       | App-specific password from <https://appleid.apple.com>. NOT your Apple ID password. |
| `APPLE_TEAM_ID`            | 10-character team ID from <https://developer.apple.com> → Membership. |

## Exporting the Developer ID cert

Done once on your laptop (the cert lives in your login keychain;
exporting it as a `.p12` is what travels to CI):

1. Open Keychain Access → My Certificates.
2. Right-click `Developer ID Application: Christopher Hotchkiss (TEAMID)`
   → Export. Format: Personal Information Exchange (`.p12`). Set a
   strong passphrase (this becomes `MACOS_CERT_PASSWORD`).
3. base64 the file:
   ```sh
   base64 -i developer_id_application.p12 | pbcopy
   ```
   Paste into the `MACOS_CERT_P12_BASE64` secret value field.
4. Delete the local `.p12` once the secret is saved — it has no
   other purpose.

## What the CI job does with the secrets

`release.yml`'s macOS lane:

1. **Import certificate** — base64-decodes the `.p12`, imports
   into a fresh `$RUNNER_TEMP/build.keychain-db`, runs
   `security set-key-partition-list` to grant codesign access
   without an interactive prompt.
2. **Build signed .app + .dmg** — `tools/build-macos-app.sh` with
   `SIGN_IDENTITY` set. The script runs `codesign --force
   --options runtime --timestamp --sign "$SIGN_IDENTITY"` on the
   binary, the `.app` bundle, and the `.dmg`.
3. **Notarize** — `xcrun notarytool submit ... --wait` uploads
   the `.dmg` to Apple, blocks until the verdict (typically <2
   minutes for a ~30 MB artifact), bails on rejection.
4. **Staple** — `xcrun stapler staple` bakes the notarization
   ticket into the `.dmg` so Gatekeeper's offline check passes
   (the user's Mac doesn't need to phone Apple on first launch).
5. **Verify** — `spctl -a -t open --context
   context:primary-signature -vv` — same check Gatekeeper runs
   on launch.
6. **Upload** — both the tarball (unsigned portable fallback) and
   the signed/notarized `.dmg` are attached to the draft release.

## Local signed builds

You can sign locally too — useful for testing notarization
configuration against a draft `.dmg` before tagging:

```sh
# Identity must already be in your login keychain — your normal
# `Developer ID Application: Christopher Hotchkiss (TEAMID)` cert.
SIGN_IDENTITY="Developer ID Application: Christopher Hotchkiss (TEAMID)" \
  tools/build-macos-app.sh

# Then notarize manually:
xcrun notarytool submit dist/Skylander-Portal-Controller-*.dmg \
  --apple-id chris@hotchkiss.io \
  --team-id TEAMID \
  --password app-specific-password \
  --wait
xcrun stapler staple dist/Skylander-Portal-Controller-*.dmg
```

The build script no-ops on signing if `SIGN_IDENTITY` is unset, so
unsigned local builds still work for fast iteration.

## Cert rotation / revocation

When the Apple Developer cert eventually expires (it's typically
5 years from issue):

1. Generate a new cert via Xcode or the Apple Developer portal.
2. Re-export to `.p12`, re-base64, update
   `MACOS_CERT_P12_BASE64` + `MACOS_CERT_PASSWORD` +
   `MACOS_CERT_IDENTITY` (the team-suffixed common name will be
   identical, but rotate the secret values just to invalidate the
   old `.p12` blob in case it ever leaked).
3. Stapled `.dmg`s already published stay valid — Apple's
   notarization persists past cert revocation.

If a cert is suspected leaked, revoke it immediately at
<https://developer.apple.com> → Certificates, Identifiers &
Profiles → Certificates. Stapled `.dmg`s already shipped continue
to work; new builds need a fresh cert before they can sign again.

[GitHub Actions security guide]: https://docs.github.com/en/actions/security-for-github-actions/security-guides/using-secrets-in-github-actions#using-secrets-in-a-workflow
