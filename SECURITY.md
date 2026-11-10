# Security policy

## Reporting a vulnerability

Please don't report security problems in public issues. Report them
privately through
[GitHub's private vulnerability reporting](https://github.com/snaketech-tu/hdrshot/security/advisories/new)
instead. You'll get an answer within a week.

Problems of interest include an image file that makes hdrshot run code,
write outside the chosen folder or crash in a way that affects other
files, and anything that weakens the update check, such as a way to make
the app install a file that isn't from an official release.

## Supported versions

Fixes are released for the latest version only. The desktop app's
**Check for updates** installs it.

## Verifying downloads

Each release lists the SHA-256 checksums of its files in `SHA256SUMS.txt`,
and the release workflow records a signed build attestation for every file.
With the [GitHub CLI](https://cli.github.com), check that a download was
built by this repository's release workflow:

```sh
gh attestation verify hdrshot.exe --repo snaketech-tu/hdrshot
```
