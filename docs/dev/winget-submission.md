# Winget submission setup

Notes for the maintainer (Chris) on getting Skylander Portal
Controller into `microsoft/winget-pkgs` so `winget install
ChristopherHotchkiss.SkylanderPortalController` becomes the primary
Windows install path. Solves the SmartScreen "unknown publisher"
friction without an Authenticode cert — winget runs as a trusted
authority and bypasses the manual "More info → Run anyway" gate
that plain MSI downloads hit.

This is the HTPC-side runbook for PLAN 13.2 → 13.4. All commands
run in PowerShell on the Windows HTPC; no Mac involvement.

> **Status (PLAN 13):** pre-flight done (13.1.x ✅). First
> submission + CI wiring blocked on HTPC keyboard time. Once
> 13.2 → 13.3 land, the existing CI hook in 13.4 auto-PRs each
> subsequent release tag to microsoft/winget-pkgs — manual flow
> only matters for the bootstrap.

## Decisions locked in (PLAN 13.1)

| Field                 | Value                                                                  |
| --------------------- | ---------------------------------------------------------------------- |
| Package Identifier    | `ChristopherHotchkiss.SkylanderPortalController`                       |
| Publisher             | `Christopher Hotchkiss`                                                |
| Package Name          | `Skylander Portal Controller`                                          |
| Moniker               | `skylander-portal-controller`                                          |
| License               | `GPL-2.0-only` (per repo `LICENSE`; relicensed from MIT in PLAN 16.2 — see note) |
| Install scope         | perMachine (matches `wix/main.wxs::InstallScope`)                      |
| Bootstrap version     | v1.5.1 (currently flagged prerelease in GitHub — winget accepts these) |
| Bootstrap MSI URL     | `https://github.com/chotchki/skylander-portal-controller/releases/download/v1.5.1/skylander-portal-controller-1.5.1-windows-x86_64.msi` |
| Bootstrap MSI SHA-256 | `be8f250b09415369d13794427b7043ab0a32f1277a9d3096e8b7f6f336c97eb3`     |

If you bootstrap against a later tag instead of v1.5.1, swap the
URL + SHA-256; everything else stays the same. The SHA-256 is in
`gh release view <tag> --json assets`.

## Why winget instead of an Authenticode cert

- Direct-download MSIs hit Windows Defender SmartScreen's "unknown
  publisher" prompt. Users have to click "More info → Run anyway"
  — friction that scares off non-technical installers.
- An EV Authenticode cert ($200–$400/yr) would clear SmartScreen
  on direct downloads, but winget already sidesteps it: when winget
  installs an MSI, SmartScreen treats winget as the publisher and
  doesn't prompt the user for unknown-publisher consent.
- Net result: no Authenticode spend, no cert rotation overhead, no
  signing pipeline to maintain on the Windows side. PLAN 10.9.2's
  Windows-Authenticode half is **superseded by Phase 13**. (The
  macOS Tier-2 half is unaffected — different OS, different
  problem, separate runbook at `docs/dev/release-signing.md`.)

## Security model — why this is low-risk

The bootstrap flow is one-time and manual; ongoing updates are
automated but only on `v*.*.*` tag pushes from upstream.

- Submission to `microsoft/winget-pkgs` is a PR through Chris's
  GitHub account. Microsoft's moderation team reviews every PR
  (1–7 day SLA typical) and runs sandbox install tests before
  merge.
- Per-release auto-PRs (Phase 13.4) run via
  `vedantmgoyal9/winget-releaser@latest` in `release.yml`'s
  Windows lane. Triggered only on `push` to `v*.*.*` tags — fork
  pushes don't reach the upstream workflow, same security
  property as `release.yml`'s tag trigger.
- The PAT secret `WINGET_PAT` (Phase 13.4.1) is fine-grained,
  scoped only to a personal fork of `microsoft/winget-pkgs`
  (`Contents: read+write` + `Pull requests: read+write` on the
  fork). Compromise blast radius: can open a PR against a public
  package repo. Mitigation: 90-day expiry + calendar reminder.

## Prerequisites

- HTPC running Windows 10 1809+ or Windows 11 (winget shipped in
  the App Installer package). `winget --version` should return
  something — if not, install "App Installer" from the Microsoft
  Store.
- A clean checkout of `skylander-portal-controller` (for the
  LICENSE URL + reference; not strictly required to submit).
- (Optional) Git + GitHub CLI (`gh auth login`) if you want to
  open the PR from the HTPC. Otherwise the PR can come from any
  machine after you push the manifest tree.

## Phase 13.2 — Manifest authoring

### 13.2.1 Install wingetcreate

```powershell
winget install --id Microsoft.WingetCreate --source winget
```

Sanity check:

```powershell
wingetcreate --version
```

### 13.2.2 Scaffold the manifest from the v1.5.1 MSI

```powershell
wingetcreate new https://github.com/chotchki/skylander-portal-controller/releases/download/v1.5.1/skylander-portal-controller-1.5.1-windows-x86_64.msi
```

It'll prompt interactively. Answer with the values from the
"Decisions locked in" table above, plus the per-version metadata
below:

| Prompt                  | Answer                                                                |
| ----------------------- | --------------------------------------------------------------------- |
| Package Identifier      | `ChristopherHotchkiss.SkylanderPortalController`                      |
| Version                 | `1.5.1` (should auto-detect; confirm)                                 |
| Publisher               | `Christopher Hotchkiss`                                               |
| Package Name            | `Skylander Portal Controller`                                         |
| Moniker                 | `skylander-portal-controller`                                         |
| Tags                    | `skylanders;rpcs3;portal;emulator;skylander`                          |
| ShortDescription        | `Phone-driven control of the RPCS3-emulated Skylanders portal over Wi-Fi.` |
| Description             | Paste the first paragraph of `README.md`.                             |
| License                 | `GPL-2.0-only`                                                        |
| LicenseUrl              | `https://github.com/chotchki/skylander-portal-controller/blob/main/LICENSE` |
| Copyright               | `Christopher Hotchkiss`                                               |
| PublisherUrl            | `https://github.com/chotchki`                                         |
| PublisherSupportUrl     | `https://github.com/chotchki/skylander-portal-controller/issues`      |
| PackageUrl              | `https://github.com/chotchki/skylander-portal-controller`             |
| ReleaseNotesUrl         | `https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.5.1` |
| Homepage                | `https://chotchki.github.io/skylander-portal-controller/`             |
| Submit to GitHub now?   | **No** — local-test first.                                            |

It writes three YAMLs to a path like
`.\manifests\c\ChristopherHotchkiss\SkylanderPortalController\1.5.1\`:

- `ChristopherHotchkiss.SkylanderPortalController.installer.yaml`
- `ChristopherHotchkiss.SkylanderPortalController.locale.en-US.yaml`
- `ChristopherHotchkiss.SkylanderPortalController.yaml` (the
  version manifest — points at the other two)

Note that path; the next steps reference it.

### 13.2.3 Validate + local install test

```powershell
$manifestDir = ".\manifests\c\ChristopherHotchkiss\SkylanderPortalController\1.5.1\"

winget validate --manifest $manifestDir
winget settings --enable LocalManifestFiles
winget install --manifest $manifestDir
```

Expected:

- `winget validate` reports "Manifest validation succeeded." Any
  warnings (e.g. about ShortDescription length, missing
  capitalisation) are worth fixing before submitting upstream.
- `winget install` actually installs the MSI — Start Menu
  shortcut + entry under `Program Files`. UAC prompt is expected
  (perMachine scope).
- The installed app launches via the Start Menu shortcut.
- `winget list ChristopherHotchkiss.SkylanderPortalController`
  shows it as version 1.5.1.

If any of those fail, capture the error output and fix the
manifest before submitting — moderation will catch the same
issues but with a 1–7 day round-trip per fix.

To remove the local install once you're happy:

```powershell
winget uninstall ChristopherHotchkiss.SkylanderPortalController
```

## Phase 13.3 — First PR to microsoft/winget-pkgs

### 13.3.1 Fork + push the manifest tree

If you have `gh` set up on the HTPC, `wingetcreate submit` does
this in one step:

```powershell
wingetcreate submit --token <your-pat> $manifestDir
```

The PAT for this can be the temporary fine-grained one you'll
create for Phase 13.4 (scope: `public_repo` on a personal fork
of `microsoft/winget-pkgs`). If you'd rather avoid generating
a PAT for the one-time bootstrap, do it the manual way:

1. Fork `https://github.com/microsoft/winget-pkgs` to
   `chotchki/winget-pkgs`.
2. Clone your fork somewhere convenient (the HTPC is fine):
   ```powershell
   git clone https://github.com/chotchki/winget-pkgs.git
   cd winget-pkgs
   git checkout -b add-skylander-portal-controller-1.5.1
   ```
3. Copy the manifest dir into the right place under your fork:
   ```powershell
   $dest = "manifests\c\ChristopherHotchkiss\SkylanderPortalController\1.5.1"
   New-Item -ItemType Directory -Force -Path $dest
   Copy-Item $manifestDir\* $dest\
   ```
4. Commit + push:
   ```powershell
   git add $dest
   git commit -m "New package: ChristopherHotchkiss.SkylanderPortalController version 1.5.1"
   git push -u origin add-skylander-portal-controller-1.5.1
   ```
5. Open a PR on GitHub against `microsoft/winget-pkgs:master`.
   Title format: `New version:
   ChristopherHotchkiss.SkylanderPortalController 1.5.1`.

### 13.3.2 Address moderation feedback

Microsoft's automation runs sandbox install tests + manifest
linting automatically (`azure-pipelines-pr-validation`,
`Microsoft.SecurityFox`, etc.). Common asks:

- ShortDescription too long (≤256 chars).
- Missing or 404 LicenseUrl / PublisherUrl.
- Tags non-conformant (lowercase, semicolon-separated).
- ProductCode missing or mismatched against the MSI.

Most fixes are one-line YAML edits + a force-push to your branch.
Re-validate locally first (`winget validate --manifest $dest`).

Don't start Phase 13.4 (auto-PR CI step) until 13.3 lands —
`winget-releaser` reads the existing package from
microsoft/winget-pkgs to compute the version bump, and can't
bootstrap a non-existent package.

### 13.3.3 Post-merge smoke test

Once the PR is merged + Microsoft's index rebuilds (~30 min):

```powershell
winget uninstall ChristopherHotchkiss.SkylanderPortalController
winget install ChristopherHotchkiss.SkylanderPortalController
```

Confirm:

- Install completes without the SmartScreen "unknown publisher"
  prompt (this is the whole point).
- `winget list` shows the package + version.
- App launches via Start Menu shortcut.

## Phase 13.4 — CI auto-PR for subsequent releases

### 13.4.1 Generate the WINGET_PAT secret

GitHub → Settings → Developer settings → Personal access tokens
→ Fine-grained tokens → Generate new token.

- **Resource owner:** `chotchki` (personal).
- **Repository access:** Only select repositories →
  `chotchki/winget-pkgs` (the fork from 13.3.1).
- **Repository permissions:**
  - Contents: Read and write
  - Pull requests: Read and write
  - Metadata: Read-only (always required)
- **Expiration:** 90 days. Calendar reminder to rotate (PLAN
  13.6.2).

Save the token immediately — GitHub shows it once. Add it to
the upstream repo as a secret:

GitHub → `chotchki/skylander-portal-controller` → Settings →
Secrets and variables → Actions → New repository secret →
Name: `WINGET_PAT`, Value: paste.

### 13.4.2 Wire winget-releaser into release.yml

Add this step to the `build-windows-release` job in
`.github/workflows/release.yml`, after the `softprops/action-gh-release`
step (the action needs the release assets to exist before it
computes the installer hash):

```yaml
      - name: Publish to winget
        if: startsWith(github.ref, 'refs/tags/v')
        uses: vedantmgoyal9/winget-releaser@latest
        with:
          identifier: ChristopherHotchkiss.SkylanderPortalController
          installers-regex: '\.msi$'
          token: ${{ secrets.WINGET_PAT }}
```

The action: downloads the MSI from the just-published release,
computes its SHA-256, clones `chotchki/winget-pkgs`, applies the
version bump via `wingetcreate update`, pushes a branch, opens
a PR against `microsoft/winget-pkgs`.

### 13.4.3 Smoke-test on the next real tag

Cut a real release tag (v1.6.0 or whatever's next). Within
~5 min of the GitHub Release going live:

- A PR should appear at
  https://github.com/microsoft/winget-pkgs/pulls?q=is%3Apr+is%3Aopen+ChristopherHotchkiss.SkylanderPortalController.
- Microsoft's automation runs validation; on green, a
  moderator merges within 1–3 days.
- After merge, `winget upgrade
  ChristopherHotchkiss.SkylanderPortalController` picks up the
  new version on any machine.

## Troubleshooting

### Submission validation flagged my MSI

Run `winget validate --manifest <dir>` locally before re-submitting.
For MSI-specific issues (ProductCode mismatch, signature
problems), see the `installer.yaml` reference at
https://learn.microsoft.com/en-us/windows/package-manager/package/manifest.

### Moderation labels: "Validation-Defender-Error"

Microsoft's automated AV scan flagged something. Usually false
positive on Rust binaries. Comment on the PR with
`/AzurePipelines run` to retry; if it persists, escalate to the
PR thread with a brief explanation of what the binary does.

### winget-releaser CI step fails with 401/403

PAT expired or scope wrong. Regenerate per 13.4.1, update the
secret. CI failure on the next tag is the early-warning signal.

### winget-releaser opens a PR against the wrong fork

The action expects `chotchki/winget-pkgs` to exist + be a
current fork of `microsoft/winget-pkgs`. If you ever delete or
rename the fork, recreate it before the next tag.

## Rotation reminders

- **WINGET_PAT:** 90 days. Calendar reminder; CI red on the next
  tag if you miss it.
- **Manifest schema version:** Microsoft bumps the manifest
  schema occasionally. `wingetcreate update` handles this
  automatically; `wingetcreate` itself just needs to stay
  reasonably current (`winget upgrade Microsoft.WingetCreate`
  on the HTPC every few months).
