# Releasing

One tag publishes all three packages under the same version:

| Registry | Package | Credential |
|---|---|---|
| [crates.io](https://crates.io) | `letsdecide` (Rust core) | None: trusted publishing |
| [npm](https://www.npmjs.com) | `letsdecide` (`bindings/js`, WASM) | None: trusted publishing |
| [PyPI](https://pypi.org) | `letsdecide` (`bindings/python`) | None: trusted publishing, plus repository variable `PUBLISH_PYPI=true` |

No registry token is stored in GitHub or anywhere else. Each publish job proves
its identity to the registry (this repository, the workflow file `release.yml`)
with a short-lived OIDC token and gets a short-lived publish credential back. The
registry only does that if you told it to trust this workflow, once per package
(see *One-time setup*).

Registry screens change. If one differs from the steps below, follow the
registry's own guide (linked in each section).

Nothing is published unless every package builds first, and every built wheel
is installed and tested on Linux, macOS and Windows before anything ships. A
registry that doesn't trust the workflow fails its publish job, so a red job
means that registry did not ship. PyPI is the exception: its job is skipped,
without a warning, unless the repository variable `PUBLISH_PYPI` is `true`.

## One-time setup

### A new package needs a first release by hand

crates.io and npm only let you add a trusted publisher to a package that
already exists, so the first version of a new package is published another way,
and trust is configured afterwards. (`letsdecide` 0.3.0 on npm was published
this way, so it has no provenance attestation. Releases from the workflow do.)

- **crates.io:** `cargo publish` locally with a short-lived API token that has
  the `publish-new` scope (<https://crates.io/settings/tokens>). Revoke the token afterwards.
- **npm:** publish with your own 2FA login, `npm publish` or `npm stage publish`
  (a person approves a staged publish with 2FA). Don't use a token that bypasses 2FA.
- **PyPI:** nothing by hand. A *pending publisher* (below) creates the project on
  the first release.

### crates.io: trusted publisher

1. Sign in at <https://crates.io> with GitHub and open the crate's
   **Settings → Trusted Publishing**.
2. Add a GitHub publisher:

   | Field | Value |
   |---|---|
   | Repository owner | `Azoies1` |
   | Repository name | `LetsDecide` |
   | Workflow filename | `release.yml` |
   | Environment | leave empty (the job has none) |

The `publish-crates` job calls `rust-lang/crates-io-auth-action`, which trades
the job's OIDC token for a short-lived crates.io token that exists only for the
`cargo publish` step. Guide: <https://crates.io/docs/trusted-publishing>.

### npm: trusted publisher

Needs npm 11.15.0 or newer, write access to the package and an account with
two-factor authentication. The command asks for a 2FA login. A token that
bypasses 2FA can't configure trust.

```sh
npm install -g npm@^11.15.0
npm trust github letsdecide --file release.yml --repo Azoies1/LetsDecide --allow-publish
npm trust list letsdecide        # check it
```

A package has one trust configuration at a time. To replace it:
`npm trust revoke --id <id> letsdecide`, then create the new one.

A new configuration **expires if no publish through it succeeds within 48
hours** ([announcement](https://github.blog/changelog/2026-10-02-unvalidated-npm-trusted-publishing-configurations-now-expire)).
If it lapses, create it again. The `publish-npm` job installs npm 11.5.1 or newer
(trusted publishing needs it) and publishes with provenance. Guide:
<https://docs.npmjs.com/trusted-publishers>.

### PyPI: trusted publisher

1. Create an account at <https://pypi.org> and enable two-factor authentication.
2. Open <https://pypi.org/manage/account/publishing/> and add a **pending
   publisher** (it creates the project on the first release) with:

   | Field | Value |
   |---|---|
   | PyPI project name | `letsdecide` |
   | Owner | `Azoies1` |
   | Repository name | `LetsDecide` |
   | Workflow name | `release.yml` |
   | Environment name | `pypi` |

   Use the owner and repository exactly as GitHub shows them.
3. Add the **repository** variable `PUBLISH_PYPI` with the value `true`
   (repository **Settings → Secrets and variables → Actions → Variables**, the
   repository tab, not an environment's: the job's `if:` can't see environment
   variables).

A pending publisher does not reserve the name. If someone else registers
`letsdecide` first, the publisher is invalidated, so release soon after
creating it. Guide: <https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/>.

## Making a release

1. Set the new version in `Cargo.toml`, `bindings/wasm/Cargo.toml`,
   `bindings/python/Cargo.toml`, `bindings/uniffi/Cargo.toml`,
   `bindings/js/package.json`, `bindings/python/pyproject.toml` and
   `bindings/kotlin/build.gradle.kts`. Run `cargo check` to refresh `Cargo.lock`.
2. Move the `[Unreleased]` entries in `CHANGELOG.md` under the new version and
   update the compare links.
3. Merge to the default branch, then tag it:
   `git tag v<version> && git push origin v<version>`.
4. Watch the **Release** run under the repository's *Actions* tab. The tag must
   match all seven versions, or the run stops before building.
5. Check each registry: `cargo add letsdecide`, `npm view letsdecide`,
   `pip download letsdecide --no-deps`. A new npm version can take a few minutes
   before its tarball downloads.

A published version number can never be reused, even if the release is removed.
To warn users off a bad release, *yank* it (crates.io, PyPI) or *deprecate* it
(npm), then publish a fixed version.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `verify` fails, `... is 0.2.0, tag is 0.3.0` | A manifest wasn't bumped. The error names the file. |
| `publish-crates` fails at `crates-io-auth-action` | The crate has no trusted publisher matching this repository, workflow file and environment, or the job lost `id-token: write`. |
| `publish-npm` fails with `E403`, `E404` or `ENEEDAUTH` | npm doesn't trust this run: check `npm trust list letsdecide` (repository `Azoies1/LetsDecide`, file `release.yml`, no environment), that the configuration hasn't expired (48 hours without a publish), and that the job has npm 11.5.1 or newer. |
| npm provenance error about `repository.url` | `repository.url` in `bindings/js/package.json` must match the GitHub repository. |
| PyPI `invalid-publisher` | The pending publisher's owner, repository, workflow or environment doesn't match the run. Compare each field. |
| Run is green but PyPI has no new version | The repository variable `PUBLISH_PYPI` isn't `true`, or it was set on an environment instead of the repository. The job is skipped without a warning. |
| A registry says the version already exists | It was published on an earlier run. Use *Re-run failed jobs*, not *Re-run all jobs*. |
