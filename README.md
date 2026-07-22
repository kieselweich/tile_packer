# tile_packer

Pack a folder of sprite sheets into a single texture atlas, driven by a small
JSON config. Each source image is sliced into a grid of tiles, and the tiles
are shelf-packed into one output image. You get back the packed image plus a
map describing where every tile landed.

## Features

- Slice images into fixed-size tiles and pack them into one atlas.
- Per-image tile sizes selected by file stem, full file name, or regex.
- A default tile size for anything not matched by a rule.
- Deterministic, sorted JSON output (keyed by image name).
- Supports PNG, JPEG, and WebP input.

## How it works

1. Every supported image directly inside the target folder is collected
   (subfolders are skipped).
2. Each image is matched against your `specs` to find its tile size; the first
   matching spec wins, otherwise the default size is used.
3. Images are sorted tallest-first and sliced into grids of their tile size.
4. Tiles are packed left-to-right into rows `atlas_width` pixels wide. When a
   row fills, packing moves to a new row and the output image grows in height.
5. You receive an `Atlas` (name → size + tile positions) and the final
   `RgbaImage`.

An image whose dimensions are not an exact multiple of its tile size is
rejected with an error, as is a spec whose tile width exceeds `atlas_width`.

## Configuration

Settings are loaded from a JSON file via `load_settings`:

```json
{
  "default_size": { "w": 16, "h": 16 },
  "atlas_width": 256,
  "specs": [
    { "stem": "hero", "w": 32, "h": 32 },
    { "file_name": "coin.png", "w": 8, "h": 8 },
    { "regex": "big_item_.*\\.png", "w": 64, "h": 64 }
  ]
}
```

| Field          | Meaning                                                   |
| -------------- | -------------------------------------------------------- |
| `default_size` | Tile size for images no spec matches.                    |
| `atlas_width`  | Output atlas width in pixels. Height grows as rows fill. |
| `specs`        | Per-image overrides; first matching spec wins.           |

Each entry in `specs` combines an identifier and a size in one flat object.
The identifier key selects the matching strategy:

| Key         | Matches against                    |
| ----------- | ---------------------------------- |
| `stem`      | Filename without extension.        |
| `file_name` | Full filename including extension. |
| `regex`     | Full filename, matched as a regex. |

## Usage

Build in memory and handle the results yourself:

```rust,no_run
use std::path::Path;
use texture_atlas::{create_atlas, load_settings};

fn main() -> anyhow::Result<()> {
    let settings = load_settings(Path::new("settings.json"))?;
    let (atlas, image) = create_atlas(Path::new("assets"), settings)?;

    image.save("atlas.png")?;
    // `atlas.entries` maps each file stem to its tile size and positions.
    Ok(())
}
```

Or write both the image and its JSON metadata straight to a folder:

```rust,no_run
use std::path::Path;
use texture_atlas::{create_atlas_files, load_settings};

fn main() -> anyhow::Result<()> {
    let settings = load_settings(Path::new("settings.json"))?;
    create_atlas_files(
        Path::new("assets"),
        Path::new("out"),
        settings,
        Some("sprites"), // writes out/sprites.png and out/sprites.json
    )?;
    Ok(())
}
```

## API

Four entry points, split between building an atlas and loading one back:

**Building**

- `create_atlas(folder, settings)` → `(Atlas, RgbaImage)`. Build in memory and
  do what you like with the packed image and metadata.
- `create_atlas_files(src, dst, settings, name)` → writes `<name>.png` and
  `<name>.json` into `dst` (name defaults to `"atlas"`). Convenience wrapper
  over `create_atlas` for the common "just save it" case.

**Loading**

- `load_settings(path)` → `Settings`. Read a `settings.json` config from disk.
- `load_atlas(path)` → `Atlas`. Read back the `.json` written by
  `create_atlas_files`, so a consumer can look up tile positions without
  re-packing.

A typical pipeline uses one side at build time and the other at runtime:
`create_atlas_files` bakes the sprites into `sprites.png` + `sprites.json`,
then your application calls `load_atlas("sprites.json")` to map each name to
its tiles.

## Example: packing a hero, a tree, and a sword

Three source images in `assets/`:

```
assets/
├── hero.png    →  16×8   (two 8×8 frames side by side)
├── tree.png    →   8×16  (one 8×16 tile)
└── sword.png   →   8×8    (one 8×8 tile)
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
  }
}
```

Entries are sorted by name (the atlas is a `BTreeMap`), so the JSON order is
stable across runs.

## License
MIT
