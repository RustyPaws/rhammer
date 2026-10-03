# rhammer

**English** | [Русский](README.ru.md)

> 🤖 This project was created with Claude AI (Anthropic), mostly as an experiment to test what AI can do. It is not meant to compete with anyone.

**rhammer** is a Valve Hammer-style level editor for Source engine maps, written in Rust with [egui](https://github.com/emilk/egui). It reads and writes `.vmf` files and uses Hammer-like game configurations (FGD entity definitions, compile tools), so it fits into the familiar Source mapping workflow.

> Status: unstable. Expect rough edges.

## Features

- **VMF support** – open, edit and save `.vmf` maps; `func_instance` instances are loaded and shown in the viewports (VBSP-style path resolution, instance props and hidden objects are handled).
- **Hammer-style game configurations** – modelled after Hammer's `GameConfig.txt` (Options → Game configurations). Configs are stored in `rhammer_config.txt` and can be imported from Hammer's own `bin/GameConfig.txt`.
- **FGD parser** – entity classes, properties, choices, spawnflags and I/O.
- **I/O graph view** – visualize entity connections (inputs/outputs) as a graph.
- **Editing tools** – Select, Block, Entity, Clip and Texture application tools, plus Transform, Make hollow, Tie to entity, Move to world, Rotate and Mirror, texture lock, and undo/redo.
- **2D and 3D viewports** – 4-view layout, grid, wireframe mode, entity names, frame selection / frame all, Hammer-style selection/block dimensions in 2D views, selected objects drawn on top.
- **Asset loading** – game files are mounted from the `SearchPaths` of `gameinfo.txt`; VPK archives, VTF textures (including transparent textures flagged in VMT), and MDL models (with VVD/VTX/ANI).
- **Texture browser and model viewer** – browse textures, orbit and pan (right/middle drag) textured models, play sequences, show bones and attachments; the model viewer also works as a picker for `studio` keys.
- **Map compilation** – runs `vbsp` → `vvis` → `vrad`, copies the BSP and launches the game, with a live compile log (Map → Run map…).
- **Find, copy/paste, duplicate** and other everyday editing operations.

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+N` / `Ctrl+O` / `Ctrl+S` / `Ctrl+Shift+S` | New / Open / Save / Save as |
| `Ctrl+Z` / `Ctrl+Y` (`Ctrl+Shift+Z`) | Undo / Redo |
| `Ctrl+X` / `Ctrl+C` / `Ctrl+V` / `Ctrl+D` | Cut / Copy / Paste / Duplicate |
| `Ctrl+A` / `Ctrl+F` | Select all / Find |
| `Ctrl+M` / `Ctrl+H` / `Ctrl+T` | Transform / Make hollow / Tie to entity |
| `Shift+S` / `Shift+B` / `Shift+E` / `Shift+C` / `Shift+A` | Select / Block / Entity / Clip / Texture tool |
| `Shift+F` | Frame selection |
| `Enter` | Commit block / clip |
| `Esc` | Cancel / clear selection |
| `Del` / `Backspace` | Delete selection |
| `G` | Toggle grid |
| `[` / `]` | Halve / double grid size |
| `F5` | Toggle 3D wireframe |
| `F9` | Run map |

Hold the right mouse button in the 3D view to fly the camera.

## Building

You need a recent stable [Rust toolchain](https://rustup.rs/).

```sh
git clone <repository-url>
cd hammer
cargo run --release
```

On Linux, install the development packages first (these are the ones used in CI):

```sh
sudo apt-get install -y libgtk-3-dev libxkbcommon-dev libwayland-dev libx11-dev libxcb1-dev libgl1-mesa-dev
```

## Releases

Pushing a tag like `vX.Y.Z` triggers the release workflow, which builds and publishes archives for:

- Windows (amd64, aarch64)
- macOS (aarch64)
- Linux (amd64, aarch64)

## License

Released under the [MIT License](LICENSE.txt).

rhammer is an independent, non-commercial experiment and is not affiliated with or endorsed by Valve. "Hammer" and "Source" are trademarks of Valve Corporation.
