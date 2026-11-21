# Releasing

One tag publishes all three packages under the same version:

| Registry | Package | Credential |
|---|---|---|
| [crates.io](https://crates.io) | `letsdecide` (Rust core) | API token, repository secret `CARGO_REGISTRY_TOKEN` |
| [npm](https://www.npmjs.com) | `letsdecide` (`bindings/js`, WASM) | Granular access token, repository secret `NPM_TOKEN` |
| [PyPI](https://pypi.org) | `letsdecide` (`bindings/python`) | None: trusted publishing, plus repository variable `PUBLISH_PYPI=true` |

Registry screens change. If one differs from the steps below, follow the
registry's own guide (linked in each section).

Publishing is opt-in per registry. A registry without its credential is
skipped, so a tag on an unconfigured repository only builds and verifies.
Nothing is published unless every package builds first.

## One-time setup

GitHub secrets and variables live in the repository under **Settings →
Secrets and variables → Actions** (secrets on the *Secrets* tab, variables on
the *Variables* tab). Anything you paste into a secret is never shown again.

### crates.io: API token

1. Sign in at <https://crates.io> with GitHub and verify your email address
   (account settings). Publishing requires a verified email.
2. Create a token at <https://crates.io/settings/tokens>. Give it the
   `publish-new` scope (the first release creates the crate) and
   `publish-update`. Set an expiry. Copy the token; it is shown once.
3. Add it as the repository secret `CARGO_REGISTRY_TOKEN`.

crates.io also supports trusted publishing (no stored token), but only after the
crate exists, so the first release needs the token. Guide:
<https://crates.io/docs/trusted-publishing>.

### npm: granular access token

1. Create an account at <https://www.npmjs.com> and enable two-factor
   authentication.
2. Profile menu → **Access Tokens** → **Generate New Token** → **Granular
   Access Token**. Permission: **Read and write** on packages. Choose all
   packages, because the package doesn't exist yet and can't be selected.
   Enable the option to bypass two-factor authentication, which CI needs.
3. Add it as the repository secret `NPM_TOKEN`.

Classic npm tokens no longer exist. Granular tokens that can publish expire:
the default is 7 days and the maximum is 90. If a release fails with `401` or
`403`, the token has probably expired: create a new one and update the secret.

A token that bypasses 2FA is on its way out for publishing. GitHub's
[announcement](https://github.blog/changelog/2026-07-08-npm-install-time-security-and-gat-bypass2fa-deprecation/)
says such tokens will lose the ability to publish directly around January 2027,
keeping only reading private packages and staging a publish that a person
approves with 2FA. They can already no longer create tokens or change trusted
publishing configuration, so do those interactively with 2FA. Use the token for
the first release, delete it, then move npm to trusted publishing
(<https://docs.npmjs.com/trusted-publishers>) or staged publishing
(<https://docs.npmjs.com/staged-publishing>).

### PyPI: trusted publisher (no token)

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
3. Add the repository variable `PUBLISH_PYPI` with the value `true`.

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
   `pip download letsdecide --no-deps`.

A published version number can never be reused, even if the release is removed.
To warn users off a bad release, *yank* it (crates.io, PyPI) or *deprecate* it
(npm), then publish a fixed version.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `verify` fails, `... is 0.2.0, tag is 0.3.0` | A manifest wasn't bumped. The error names the file. |
| npm `401` / `403`, or an error asking for a staged publish | Token expired (90 days at most), missing the 2FA-bypass option, or the January 2027 change has taken effect. Publish by hand with your 2FA login, or use staged publishing. |
| npm provenance error about `repository.url` | `repository.url` in `bindings/js/package.json` must match the GitHub repository. |
| PyPI `invalid-publisher` | The pending publisher's owner, repository, workflow or environment doesn't match the run. Compare each field. |
| A registry says the version already exists | It was published on an earlier run. Use *Re-run failed jobs*, not *Re-run all jobs*. |
