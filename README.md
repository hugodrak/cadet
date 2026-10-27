# Cadet

Fast, local-first, open-source parametric CAD for makers. Sketch → constrain → extrude/revolve → export STL/STEP for your printer.

## Install
Download the build for your OS from the GitHub Releases page. It's a single self-contained app with nothing else to install:

| OS | File | Notes |
|---|---|---|
| macOS (Apple Silicon / Intel) | `Cadet_x.y.z_aarch64.dmg` / `_x64.dmg` | Unsigned for now: right-click → **Open** the first time |
| Windows | `Cadet_x.y.z_x64-setup.exe` | SmartScreen may warn about an unknown publisher: **More info → Run anyway** |
| Linux | `Cadet_x.y.z_amd64.AppImage` or `.deb` | `chmod +x Cadet*.AppImage && ./Cadet*.AppImage` |

## Use
| Input | Action |
|---|---|
| Left drag · Shift/Right/Middle drag · Scroll | Orbit · Pan · Zoom |
| ⌘K / Ctrl+K | Command palette (every action is searchable) |
| S | New sketch: click XY/XZ/YZ or any flat face |
| E · R | Extrude · Revolve. Click profile regions to toggle them, set the values, Enter |
| F · ⇧F | Fillet · Chamfer. Click edges to toggle them, set the radius/distance, Enter |
| I | Measure: snaps to vertices/centers/edges/faces; shows distance, ΔXYZ, exact edge length/Ø, model size, volume |
| ⌘J | Python console (the `cadet` module is preloaded) |
| ⌘E | Export STL / STEP |
| ⌘N ⌘O ⌘S ⇧⌘S · ⌘Z ⇧⌘Z | Files · Undo/Redo |
| 0 1 2 3 · G | Home/Front/Right/Top views · Toggle grid |

**In a sketch:** L line · R rectangle · C circle · A arc · D dimension · H/V horizontal/vertical · P parallel · ⇧P perpendicular · E equal · T tangent · O coincident · F fix · Delete · Esc steps back · ⌘↵ finish.
Snap to points and curves while drawing. Use Select to pick geometry (Shift adds) and apply constraints. Drag to move geometry under its constraints. Double-click a dimension to edit it. Double-click timeline items to edit them; right-click to delete.

**Python:** the console and plugins run built-in Python (RustPython, so there's nothing to install). A plugin is a `.py` file in the plugins folder (Plugins → Open plugins folder):
```python
import cadet

@cadet.command("Plate")
def plate(width=40.0, depth=30.0, thickness=3.0):   # defaults become a form
    s = cadet.sketch("XY")
    cadet.rect(s, 0, 0, width, depth)
    cadet.extrude(s, thickness)
```
See `plugins/` for the spur gear and send-to-slicer examples.

Work is autosaved continuously and the last session reopens on launch. Double-clicking a `.cadet` file opens it. Designs are plain JSON `.cadet` files (see `examples/`).

## Build from source
Needs Rust (stable), CMake ≥ 3.16 and a C++ compiler. OpenCascade is compiled from source and linked statically on the first build (~4–20 min, then cached).
```sh
cargo run --release                         # run
cargo test --release --workspace            # tests
cargo install cargo-packager --locked
cargo packager --release -p cadet-app       # .app/.dmg, setup .exe, AppImage/.deb in dist/
```
No CMake? `python3 -m venv ~/.local/cadet-tools && ~/.local/cadet-tools/bin/pip install cmake` and put that `bin` on your `PATH`.
Pushing a `v*` tag builds and publishes all platforms via `.github/workflows/release.yml`.

Dev hooks: `CADET_SCREENSHOT=out.png` screenshots and exits. `CADET_SCRIPT=steps.txt` replays scripted input (`crates/cadet-app/src/devtools.rs`). `CADET_KERNEL_CRASH_ON=<feature>` tests crash recovery.

## License
Apache-2.0, see `LICENSE`. Bundled third-party software is listed in `THIRD_PARTY.md`.
