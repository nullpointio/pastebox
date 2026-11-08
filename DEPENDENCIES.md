# Dependency policy

Pastebox ships as a Windows desktop binary. A vulnerable or wrongly licensed
crate is a shipped defect, not a build detail — and because the GUI and Win32
crates cannot be built on Linux, these checks are deliberately
**resolution-only** (no compilation) so they still run on the Linux runner.

## Rules

- **Latest stable at build time.** CI builds on `dtolnay/rust-toolchain@stable`;
  there is no pinned toolchain file.
- **MSRV is declared, not implied.** `rust-version` in `[workspace.package]`
  (currently **1.80**) is the oldest compiler we promise to support. A bump that
  raises the MSRV is a deliberate decision: update `rust-version` in the same PR
  and say so in the description.
- **`Cargo.lock` must be committed.** It is the input to every check below. The
  workspace currently has none: `deps` generates one at runtime and emits a
  warning, but until it is committed the audit results describe a graph nobody
  actually builds.
- **Dependabot runs weekly** (`.github/dependabot.yml`). Minor + patch updates
  arrive as one grouped PR (`rust-deps`), majors as a separate PR
  (`rust-deps-major`), and `github-actions` versions get their own weekly PR. At
  most 5 open PRs per ecosystem.
- **Advisories block release.** `cargo audit` fails on any advisory; a failing
  audit means no release.
- **Licenses are allow-listed.** `cargo deny check licenses bans sources`
  (`deny.toml`) permits only MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC,
  Unicode-3.0, Zlib and MPL-2.0. GPL, AGPL and SSPL are effectively denied
  (anything not on the list is), and only the crates.io registry is accepted —
  no git or unknown-registry sources.

## Precondition: the workspace must resolve

`deps` starts with `cargo metadata --no-deps`. Every workspace member needs
`src/lib.rs` or `src/main.rs` before the checks mean anything; while
`crates/platform`, `crates/ui` or `crates/app` are still empty the job fails at
that step **by design** — an unresolvable manifest cannot be audited and cannot
be shipped. The failure message is the manifest error, not a dependency problem.

## What runs where

| Workflow | Job (check name) | Trigger | Blocks a release? |
| --- | --- | --- | --- |
| `deps.yml` | `Dependency audit` | weekly cron, manual, push touching `Cargo.lock`/`Cargo.toml`/`crates/*/Cargo.toml`/`deny.toml` | **yes** — required check by name |
| `deps.yml` | `Dependency freshness` | same | no — advisory, reports via the issue |

The workflow runs weekly (`17 6 * * 1`, Mondays 06:17 UTC), on
`workflow_dispatch`, and on any push to `main` that changes the lockfile or a
manifest.

## Release gating (to be wired when `ci.yml` lands)

There is no `ci.yml` in this repo yet, so nothing runs on a `v*` tag today. When
the build pipeline is added:

1. Add these two steps to its test job, before the build steps:

   ```yaml
   - uses: taiki-e/install-action@v2
     with:
       tool: cargo-audit
   - name: Dependency audit (blocks release)
     run: cargo audit
   ```

2. Make `release` depend on that job (`needs:`), directly or transitively.
3. Mark **`Dependency audit`** (workflow `deps`) as a *required status check* on
   the `main` branch protection rule. GitHub reports it as
   `deps / Dependency audit`. Keep the job name stable — renaming it silently
   un-gates releases.

A tag is only released once `cargo audit` has passed on that commit.

## When the issue appears

An open issue titled **`deps: dependency drift or advisory`** means a check
failed. Its body holds the captured output in collapsible blocks.

1. Read the failing section: `audit.txt` = advisory, `deny.txt` = license/source
   problem, `outdated.txt` = a direct dependency is behind.
2. **Advisory:** bump the affected crate to a patched version
   (`cargo update -p <crate> --precise <version>`). If no fix exists, stop
   shipping and evaluate removing the dependency — do not silence it.
3. **License failure:** swap the crate, or — only with a deliberate decision
   recorded in the PR — add a narrow `exceptions` entry in `deny.toml`. Never
   widen the global allow-list to unblock one build. The GUI stack
   (`eframe`/`egui`/`winit`) pulls a large tree; the first green run may surface
   a license outside the list. Handle it through this process, not by editing
   the list on the fly.
4. **Outdated:** take the Dependabot PR, or run `cargo update -p <crate>`. Then
   `cargo test --workspace` on Windows.
5. Close the issue once `deps` is green. It reopens itself if the problem
   returns.

Local reproduction:

```sh
cargo audit
cargo deny check licenses bans sources
cargo outdated --workspace --root-deps-only --exit-code 1
```
