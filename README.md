# tile_packer

Pack a folder of sprite sheets into a single texture atlas, driven by a small
JSON config. Each source image is sliced into a grid of tiles, and the tiles
are shelf-packed into one output image. You get back the packed image plus a
map describing where every tile landed.

## Features

- Slice images into fixed-size tiles and pack them into one atlas.
- Per-image tile sizes selected by stem, full key, or regex.
- Optional recursive collection, with folder-aware or flat naming.
- A default tile size for anything not matched by a rule.
- Deterministic, sorted JSON output (keyed by image name).
- Supports PNG, JPEG, and WebP input.
- Optional incremental rebuilds (`hash` feature).
- Parse-only builds for wasm and other no-filesystem targets.

## Cargo features

| Feature | Default | Purpose                                                        |
| ------- | ------- | -------------------------------------------------------------- |
| `fs`    | yes     | Filesystem access: packing, `load_settings`, `load_atlas`.     |
| `hash`  | no      | Incremental rebuilds. Implies `fs`.                            |

For wasm or any target without a filesystem, disable defaults and use
`parse_atlas` to read metadata produced at build time on another machine:

```toml
tile_packer = { version = "0.2", default-features = false }
```

## How it works

1. Supported images are collected from the target folder. Subfolders are
   skipped unless `recurse` is set.
2. Each image gets a **key** — by default its file stem; with
   `recurse: "folder_naming"`, its path relative to the target folder,
   without the extension (`ui/button.png` → `ui/button`).
3. Each key is matched against your `specs` to find its tile size; the first
   matching spec wins, otherwise `default_size` is used.
4. Images are sorted tallest-first and sliced into grids of their tile size.
5. Tiles are packed left-to-right into rows `atlas_width` pixels wide. When a
   row fills, packing moves to a new row and the output image grows in height.
6. You receive an `Atlas` (key → size + tile positions, plus the final atlas
   dimensions) and the packed `RgbaImage`.

An image whose dimensions are not an exact multiple of its tile size is
rejected with an error, as is a spec whose tile width exceeds `atlas_width`,
and a duplicate key.

## Configuration

Settings are loaded from a JSON file via `load_settings`:

```json
{
  "default_size": { "w": 16, "h": 16 },
  "atlas_width": 256,
  "recurse": "folder_naming",
  "specs": [
    { "stem": "hero", "w": 32, "h": 32 },
    { "full_key": "ui/coin", "w": 8, "h": 8 },
    { "regex": "^world/big_item_*", "w": 64, "h": 64 }
  ]
}
```

| Field          | Meaning                                                        |
| -------------- | -------------------------------------------------------------- |
| `default_size` | Tile size for images no spec matches.                          |
| `atlas_width`  | Output atlas width in pixels. Height grows as rows fill.       |
| `recurse`      | Optional. Omit for top-level only. See below.                  |
| `specs`        | Per-image overrides; first matching spec wins.                 |

### `recurse`

| Value             | Behaviour                                                      |
| ----------------- | -------------------------------------------------------------- |
| *omitted*         | Only images directly in the folder are packed.                 |
| `"folder_naming"` | Subfolders are walked; the relative path becomes the key.      |
| `"flat"`          | Subfolders are walked; only the file stem is used as the key.  |

`"flat"` fails the build if two images share a stem, so it suits trees where
folders are purely organisational. `"folder_naming"` keeps folder structure in
the keys, which also lets a single regex spec target a whole folder.

### Specs

Each entry in `specs` combines an identifier and a size in one flat object.
The identifier key selects the matching strategy — all match against the
**key**, not the filename on disk:

| Key        | Matches against                                                  |
| ---------- | ---------------------------------------------------------------- |
| `stem`     | The last path segment of the key (`button` matches `ui/button`). |
| `full_key` | The complete key, exactly (`ui/button`).                         |
| `regex`    | The complete key, matched as a regex (`^ui/.*`).                 |

Keys always use forward slashes, on every platform.

## Usage

Build in memory and handle the results yourself:

```rust,no_run
use std::path::Path;
use tile_packer::{create_atlas, load_settings};

fn main() -> anyhow::Result<()> {
    let settings = load_settings(Path::new("settings.json"))?;
    let (atlas, image) = create_atlas(Path::new("assets"), settings)?;

    image.save("atlas.png")?;
    // `atlas.entries` maps each key to its tile size and positions.
    Ok(())
}
```

Or write both the image and its JSON metadata straight to a folder:

```rust,no_run
use std::path::Path;
use tile_packer::{create_atlas_files, load_settings};

fn main() -> anyhow::Result<()> {
    let settings = load_settings(Path::new("settings.json"))?;
    create_atlas_files(
        Path::new("assets"),
        Path::new("out"),
        Some(settings),
        Some("sprites"), // writes out/sprites.png and out/sprites.json
    )?;
    Ok(())
}
```

With the `hash` feature, repack only when sources changed:

```rust,no_run
use std::path::Path;
use tile_packer::{BuildOutcome, create_atlas_files_on_change, load_settings};

fn main() -> anyhow::Result<()> {
    let settings = load_settings(Path::new("settings.json"))?;
    match create_atlas_files_on_change(
        Path::new("assets"),
        Path::new("out"),
        Some(settings),
        Some("sprites"),
    )? {
        BuildOutcome::Rebuilt => println!("repacked"),
        BuildOutcome::Unchanged => println!("up to date"),
    }
    Ok(())
}
```

## API

**Building** (requires `fs`)

- `create_atlas(folder, settings)` → `(Atlas, RgbaImage)`. Build in memory and
  do what you like with the packed image and metadata.
- `create_atlas_files(src, dst, settings, name)` → writes `<name>.png` and
  `<name>.json` into `dst` (name defaults to `"atlas"`).

**Incremental** (requires `hash`)

- `create_atlas_on_changed(folder, atlas_path, settings)` → `Option<(Atlas, RgbaImage)>`.
  `None` if the fingerprint matches the existing atlas.
- `create_atlas_files_on_change(src, dst, settings, name)` → `BuildOutcome`.

**Loading**

- `load_settings(path)` → `Settings`. Requires `fs`.
- `load_atlas(path)` → `Atlas`. Requires `fs`.
- `parse_atlas(json)` → `Atlas`. Available everywhere, including wasm.

A typical pipeline uses one side at build time and the other at runtime:
`create_atlas_files` bakes the sprites into `sprites.png` + `sprites.json`,
then your application calls `load_atlas` (native) or `parse_atlas` (wasm) to
map each key to its tiles.

## Example: packing a hero, a tree, and a sword

Three source images in `assets/`:

```
assets/
├── hero.png    →  16×8   (two 8×8 frames side by side)
├── tree.png    →   8×16  (one 8×16 tile)
└── sword.png   →   8×8   (one 8×8 tile)
```

With this config:

```json
{
  "default_size": { "w": 8, "h": 8 },
  "atlas_width": 16,
  "specs": [
    { "stem": "tree", "w": 8, "h": 16 }
  ]
}
```

Images are packed tallest-first, so `tree` (16px) goes down before the two
8px tiles. The atlas is 16px wide, so it fits two 8px-wide tiles per row:

```
final atlas (16×24)
┌────┬────┐
│    │    │
│ T  │ H1 │   tree  at (0,0)
│    │    │   hero  at (8,0)
├────┼────┤
│ H2 │ S  │   hero  at (0,16)
└────┴────┘   sword at (8,16)
```

The resulting `Atlas`:

```json
{
  "entries": {
    "hero":  { "size": { "w": 8, "h": 8 },  "pos": [ { "x": 8, "y": 0 }, { "x": 0, "y": 16 } ] },
    "sword": { "size": { "w": 8, "h": 8 },  "pos": [ { "x": 8, "y": 16 } ] },
    "tree":  { "size": { "w": 8, "h": 16 }, "pos": [ { "x": 0, "y": 0 } ] }
  },
  "size": { "w": 16, "h": 24 }
}
```

Entries are sorted by key (the atlas is a `BTreeMap`), so the JSON order is
stable across runs.

## Migrating from 0.1

- `NameIdentifier::FileName` is gone. Specs now match keys, which never
  include the file extension — use `full_key` or `regex` instead.
- Regexes match keys, not filenames: `"big_item_.*\\.png"` becomes
  `"big_item_.*"`.
- `Atlas` gained a `size` field. Old JSON will fail to parse.
- Filesystem functions are behind the `fs` feature, on by default.

## License
MIT
