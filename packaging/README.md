# Packaging

- `linux/`: the `.desktop` file and icons in the Linux app archive.
- `macos/`: the app icon for the macOS bundle.
- `winget/`: the first manifests of the two winget packages,
  `SnakeTech.hdrshot` (desktop app) and `SnakeTech.hdrshot.CLI` (command
  line), for the initial submission to
  [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs). Later
  versions are submitted by the `winget` job in `release.yml`.
- `scoop/bucket/`: the contents of the
  [snaketech-tu/scoop-bucket](https://github.com/snaketech-tu/scoop-bucket)
  repository, which updates its manifest from new releases on its own.

## First winget submission

From a fork of microsoft/winget-pkgs, copy each package folder to
`manifests/s/SnakeTech/hdrshot/2.0.1/` and
`manifests/s/SnakeTech/hdrshot/CLI/2.0.1/` respectively, check them with
`winget validate --manifest <folder>`, and open a pull request. After both
are merged, add a classic GitHub token with the `public_repo` scope as the
`WINGET_TOKEN` secret of this repository, so releases update them.

## Scoop bucket

Create the empty repository `snaketech-tu/scoop-bucket` and copy the
contents of `scoop/bucket/` into it. Under Settings, Actions, General, allow
workflows read and write permissions so Excavator can commit updates.
