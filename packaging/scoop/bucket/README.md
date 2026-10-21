# Scoop bucket for hdrshot

```powershell
scoop bucket add snaketech https://github.com/snaketech-tu/scoop-bucket
scoop install hdrshot
```

This installs both `hdrshot-gui` (desktop app, with a Start menu shortcut)
and `hdrshot` (command line). `scoop update hdrshot` updates them.

The Excavator workflow checks for new hdrshot releases every six hours and
updates `bucket/hdrshot.json`.
