# hdrshot desktop app

All options of the desktop app. The basics are in the
[README](../README.md#desktop-app).

- **Copy for Discord.** After converting, the last PNG (the HDR one if made)
  is copied to the clipboard as a file, so pasting it into Discord, a
  browser upload or a folder gives the full file. Each converted image also
  has its own **Copy for Discord** button. The copy leaves out the `cICP`
  chunk, which makes Discord turn HDR images yellow; the saved file keeps it
  for Windows Photos and HDR viewers. When the copy is larger than
  Discord's 10 MB upload limit, the app offers **Copy a smaller version**,
  which copies it at 1440p, or 1080p or 720p if needed, so it fits. The
  saved PNGs stay at full size. The button shows when you hover a row, and
  stays on the image converted last. With AVIF or Ultra HDR JPEG as the HDR
  format, the copy is an HDR PNG made for Discord, since Discord shows
  AVIF too bright and Ultra HDR JPEGs in SDR only. For the last image of a
  batch, which is copied automatically, the app makes it while converting;
  for the others, **Copy for Discord** makes it, which takes a second or
  two.
- **Drag out** (Windows). Drag a converted image from the list into
  Discord, a browser upload or a folder to drop the same file that **Copy
  for Discord** gives; with several rows selected, all of them go. When an
  AVIF or Ultra HDR image's PNG for Discord isn't made yet, the first drag
  makes it and the app says when it is ready to drag.
- **Discord-compatible PNG**, below the HDR format, saves HDR PNGs without
  the `cICP` and `mDCV` chunks, like the Discord copy, for uploading the
  saved files to Discord yourself. Windows Photos and most HDR viewers then
  show them in SDR only, so it is off by default.
- **SDR format** **JPEG** saves the SDR image as a JPEG at quality 90,
  a fraction of the PNG's size, for chats and forums that don't show HDR.
  JPEG has no transparency, so transparent areas are shown over black.
- **Originals: Replace the original** moves each original to the Recycle
  Bin (the Trash on macOS and Linux) once all of its files are saved, so
  only the converted files are left in the folder and the original can
  still be restored. Images that fail, or whose files already existed, are
  kept, and so is everything when you stop a batch. It is off by default;
  the button for the steps after converting, under the file list, shows
  whether originals are kept or replaced.
- **Watch folder**, in Settings, converts each image saved to
  the chosen folder or its subfolders from then on, with the current
  settings, as soon as it is completely written. On Windows its menu offers
  the Game Bar captures folder (`Videos\Captures`) and the NVIDIA app's
  (`Videos\NVIDIA`, with a subfolder per game) when they exist. Images
  already in the folder are left alone, and PNGs are ignored, since hdrshot
  writes its own there. It works while
  the app is open, and resumes when the app starts again. While it is on, a
  pill at the top of the window names the folder; click it to pause
  watching until you click it again or restart the app. With **Copy the
  last converted image** on, each new screenshot is on the clipboard, ready
  to paste into Discord. While the list is empty, the app offers to turn it
  on for the Game Bar or NVIDIA app folder, whichever holds the newest HDR
  screenshot.
- **Background** (Windows), in Settings: with **Keep running in the tray
  when the window is closed** on, closing the window leaves hdrshot in the
  notification area, where it keeps watching. Click its icon to open the
  window again; right-click it to pause or resume watching, or to quit.
  **Start with Windows, in the tray** starts it there when you sign in
  (it is listed in Task Manager's Startup apps), and **Notify when images
  are converted in the background** shows a Windows notification when
  images were converted while the window was closed or behind other
  windows; click it to open hdrshot. All of these are off by default,
  except the notifications once the tray is on.
- **Previews.** Each file gets a thumbnail; hover it for a larger SDR
  preview. When the fine-tuning settings are changed, the preview shows the
  automatic result and the result with your settings, so you can compare.
  Click a thumbnail, or select a row and press Space, to see the image as
  large as the window; the arrow keys and the buttons at the sides show the
  previous and next image, and Escape, Space or a click beside the image
  closes it. It is made when you open it and freed when you close it.
- **The file list.** Each row shows a thumbnail, the image's size and type,
  and once converted the files made and their sizes, or why it failed.
  Hovering a row shows buttons to open the image, show it in its folder and
  remove it. Double-click a converted image to open it. Right-click a row to
  open the HDR or SDR image, show it in its folder, convert it again or
  remove it. **Clear**, above the list, removes all files or only the
  converted ones. After removing, **Undo** puts the files back for a few
  seconds. Once every file is converted, the button turns into
  **Convert again**, which converts them all again, for example after
  changing the settings.
- **Selecting rows.** Click a row to select it, Ctrl-click to add or take
  out more, Shift-click to select a range, and press Ctrl+A to select all
  (Cmd on macOS). Right-click a selection to convert or remove all of it.
  The keyboard shortcuts:

  | Key | Does |
  |---|---|
  | Delete | Removes the selected rows |
  | Ctrl+Z | Puts removed rows back |
  | Enter | Converts the files not converted yet |
  | Space | Shows the selected image as large as the window |
  | Ctrl+C | Copies the selected image for Discord |
  | Ctrl+O | Adds files |
  | Ctrl+V | Adds files copied in Explorer or Finder |
  | Arrow keys | Move the selection |
  | Escape | Clears the selection, or closes Settings and the large preview |
- **PNGs that already exist.** When some of the PNGs a batch would make
  are already there, the app asks once per batch whether to replace them,
  keep both (the new ones get a number, e.g. `shot_hdr (2).png`) or skip
  them.
- **Resolution** scales the output down to 2160p, 1440p, 1080p or 720p,
  keeping the aspect ratio and never scaling up. It starts at Source every
  time.
- **Open .jxr files with hdrshot** (Windows and Linux), in Settings, makes
  a double-click on a screenshot open it in the app. Windows asks you to
  confirm the choice in Settings. It also lists hdrshot under **Open with**
  for `.wdp`, `.exr`, `.jxl`, `.hdr` and `.heic` files, without making it
  their default.
  On Windows it also adds **Convert with hdrshot** to the right-click menu
  of these files (on Windows 11 under **Show more options**), which opens
  the app and converts them right away with your saved settings. Selecting
  several files converts them as one batch.
  On macOS, hdrshot is listed under **Open With** for all of these; to make
  it the default for a type, choose it in a file's **Get Info** window and
  click **Change All**.
- **Settings** also has **Keep the capture date** (on by default), which gives
  each PNG the date of its screenshot so folders sorted by date stay in
  capture order. **Save HDR and SDR images in subfolders** writes them
  to `hdr` and `sdr` subfolders, and with that on, **End file names in _hdr
  and _sdr** can be turned off. Below these, it shows the exact names an
  image gets with the current settings, using the first listed file.
  **Theme** picks light or dark, or follows the system (the default).
  Under **Fine-tuning** it holds the tuning
  options from the command line. They are off by default and reset every
  time the app starts, so results always match the original screenshot
  unless you change them; while they are changed, a **Custom tuning**
  button shows under the list.
- **Check for updates**, at the bottom right, looks for a newer release on
  GitHub and shows its release notes. **Update** downloads it and checks it
  against the release's `SHA256SUMS.txt`: on Windows and Linux it replaces
  the app and restarts it, on macOS it opens the new disk image to drag to
  Applications. **Skip** keeps the current version.
  The app also checks by itself when it starts, at most once a week; when
  there is a newer version, the button becomes **Update to** and the
  version, and nothing else happens until you click it. Turn this off with
  **Check for updates once a week** in Settings. The app connects only to
  GitHub, only for these checks and updates.
- While converting, the Windows taskbar button shows the progress, and the
  button flashes (the Dock icon bounces on macOS) when a batch finishes
  while you're in another window.
- The window is drawn on the CPU and only redraws when something changes,
  so it uses almost no CPU or GPU while idle and doesn't flicker on variable
  refresh rate monitors (G-Sync, FreeSync).
