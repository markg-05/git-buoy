# Publishing a release

Git Buoy publishes the Rust package to crates.io and native archives through
[the release workflow](../.github/workflows/release.yml). Homebrew and other
package-manager distribution remain follow-up work.

## Before creating the tag

1. Confirm every v0.1 blocker other than the release-artifact issue itself is closed in the [release-readiness tracker](https://github.com/markg-05/git-buoy/issues/12).
2. Run [release acceptance](release-acceptance.md) manually on the candidate commit. macOS and Linux must report `20 passed, 0 failed`; Windows must pass archive extraction, `--help`, and `--version` on its non-interactive hosted runner.
3. Record the workflow link and artifact hashes in `release-acceptance.md`, then change its machine-readable line to `Release status: Accepted`.
4. Confirm `Cargo.toml` uses the intended version and that `docs/release-notes/v<version>.md` exists.
5. Land the candidate on the default branch and confirm its normal CI run is green.

Do not create the tag while the acceptance record says `Blocked`. The release workflow fails closed if GitHub issue state cannot be read, a blocker is open, or the committed acceptance record is not accepted.

## Tag and publish

For version 0.1.0, create and push the matching annotated tag:

```sh
git tag -a v0.1.0 -m "Git Buoy 0.1.0"
git push origin v0.1.0
```

The tag starts the release workflow. Before publishing, it:

- requires the tag to equal `v` plus the `Cargo.toml` version;
- runs format, Clippy, and tests on Linux, macOS, and Windows;
- builds archives whose names contain the version, platform, and architecture;
- extracts each archive and checks `--help` and the displayed version;
- runs the disposable-repository terminal acceptance suite on hosted runners that provide a usable terminal;
- creates a SHA-256 file for each archive.

Only the final job has `contents: write` permission. It receives the already accepted archives and checksums and creates the GitHub release using the matching notes file. A failed prerequisite leaves the tag without a published release.

After publication, verify the release page and checksums, close the release-artifact issue, and re-audit the parent readiness tracker before closing it.
