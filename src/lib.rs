#![doc = include_str!("../README.md")]

use anyhow::{Context, anyhow};
use image::{RgbaImage, imageops};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const SUPPORTED_IMAGE_TYPES: &[&'static str] = &["png", "jpg", "jpeg", "webp"];

/// Position of a tile in the atlas, in pixels from the top-left corner.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Pos {
    pub x: u32,
    pub y: u32,
}

impl From<(u32, u32)> for Pos {
    fn from(value: (u32, u32)) -> Self {
        Self {
            x: value.0,
            y: value.1,
        }
    }
}

/// Identifies which image files a [`TileSpec`] applies to.
///
/// Matching is attempted against each file when building an atlas:
/// - [`Stem`](NameIdentifier::Stem): matches the filename without its
///   extension.
/// - [`FileName`](NameIdentifier::FileName): matches the full filename
///   including extension — use this to distinguish assets that share a
///   stem but differ in format or version.
/// - [`Regex`](NameIdentifier::Regex): matches the full filename against a
///   regular expression (e.g. `"big_item_.*\\.png"`).
///
/// # JSON representation
///
/// Serialized as an externally-tagged object with a `snake_case` key:
/// `{ "stem": "..." }`, `{ "file_name": "..." }`, or `{ "regex": "..." }`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NameIdentifier {
    Stem(String),
    FileName(String),
    Regex(String),
}

impl TryInto<CompiledIdentifier> for NameIdentifier {
    type Error = anyhow::Error;

    fn try_into(self) -> anyhow::Result<CompiledIdentifier> {
        match self {
            NameIdentifier::Regex(s) => Ok(CompiledIdentifier::Regex(
                regex::Regex::new(&s).with_context(|| format!("invalid regex in spec: {s:?}"))?,
            )),
            NameIdentifier::Stem(s) => Ok(CompiledIdentifier::Stem(s.clone())),
            NameIdentifier::FileName(s) => Ok(CompiledIdentifier::FileName(s.clone())),
        }
    }
}

#[derive(Debug, Clone)]
enum CompiledIdentifier {
    Stem(String),
    FileName(String),
    Regex(regex::Regex),
}

impl CompiledIdentifier {
    fn matches(&self, path: &Path) -> bool {
        match self {
            Self::Stem(s) => path.file_stem().is_some_and(|st| st == s.as_str()),
            Self::FileName(s) => path.file_name().is_some_and(|n| n == s.as_str()),
            Self::Regex(re) => path
                .file_name()
                .is_some_and(|n| re.is_match(&n.to_string_lossy())),
        }
    }
}

/// The width and height of a tile, in pixels.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// width in pixels
    pub w: u32,
    /// height in pixels
    pub h: u32,
}

impl From<(u32, u32)> for Size {
    fn from(value: (u32, u32)) -> Self {
        Self {
            w: value.0,
            h: value.1,
        }
    }
}

/// Pairs a [`NameIdentifier`] with the [`Size`] that its matching images
/// should be sliced into.
///
/// Both fields are flattened into the parent object during (de)serialization,
/// so a spec is expressed as a single flat JSON object (see [`NameIdentifier`]
/// for the tagging of the identifier field).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TileSpec {
    #[serde(flatten)]
    identifier: NameIdentifier,
    #[serde(flatten)]
    size: Size,
}

impl TileSpec {
    /// Constructs a [`TileSpec`] that matches by file stem
    /// (filename without extension).
    pub fn new_stem(w: u32, h: u32, stem: impl Into<String>) -> Self {
        Self::new(w, h, NameIdentifier::Stem(stem.into()))
    }

    /// Constructs a [`TileSpec`] that matches file names against a
    /// regular expression.
    pub fn new_regex(w: u32, h: u32, regex: impl Into<String>) -> Self {
        Self::new(w, h, NameIdentifier::Regex(regex.into()))
    }

    /// Constructs a [`TileSpec`] from an explicit [`NameIdentifier`].
    pub fn new(w: u32, h: u32, identifier: NameIdentifier) -> Self {
        Self {
            identifier,
            size: Size { w, h },
        }
    }
}

impl TryInto<TileSpecComp> for &TileSpec {
    type Error = anyhow::Error;

    fn try_into(self) -> anyhow::Result<TileSpecComp> {
        let identifier: CompiledIdentifier = self.identifier.clone().try_into()?;
        Ok(TileSpecComp {
            identifier,
            size: self.size,
        })
    }
}

#[derive(Debug, Clone)]
struct TileSpecComp {
    pub identifier: CompiledIdentifier,
    pub size: Size,
}

/// Configuration controlling how an atlas is built.
///
/// Typically loaded from a `settings.json` file via [`load_settings`].
///
/// # JSON shape
///
/// ```json
/// {
///   "default_size": { "w": 16, "h": 16 },
///   "atlas_width": 256,
///   "specs": [
///     { "stem": "hero", "w": 32, "h": 32 },
///     { "regex": "big_item_.*\\.png", "w": 64, "h": 64 }
///   ]
/// }
/// ```
///
/// Each entry in `specs` flattens a [`NameIdentifier`] and a [`Size`] into
/// one object. `default_size` applies to any image no spec matches.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub default_size: Size,
    pub atlas_width: u32,
    pub specs: Vec<TileSpec>,
}

impl Settings {
    fn get_compiled_specs(&self) -> anyhow::Result<Vec<TileSpecComp>> {
        self.specs.iter().map(|ts| ts.try_into()).collect()
    }
}

/// The result of packing: a map from image name (file stem) to that image's
/// [`AtlasEntry`].
///
/// Keyed by file stem, so entries serialize in sorted, deterministic order.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Atlas {
    pub entries: BTreeMap<String, AtlasEntry>,
}

/// The tiles sliced from one source image: their shared [`Size`] and the
/// position of each tile, in row-major order (left-to-right, top-to-bottom)
/// as cut from the source.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AtlasEntry {
    pub size: Size,
    pub pos: Vec<Pos>,
}

impl From<(Size, Vec<Pos>)> for AtlasEntry {
    fn from(value: (Size, Vec<Pos>)) -> Self {
        Self {
            size: value.0,
            pos: value.1,
        }
    }
}

/// Loads [`Settings`] from a JSON file at `path`.
///
/// # Errors
///
/// Returns an error if the file cannot be read (e.g. it is missing) or if
/// its contents are not valid `Settings` JSON.
pub fn load_settings(path: &Path) -> anyhow::Result<Settings> {
    let settings = std::fs::read_to_string(path).context("Missing 'settings.json'")?;
    serde_json::from_str::<Settings>(&settings).context("No settings found")
}

/// Loads a previously written [`Atlas`] from a JSON file at `path`.
///
/// This is the inverse of the JSON produced by [`create_atlas_files`], letting
/// a consumer (e.g. a game or renderer) read back where each tile landed
/// without re-running the packing.
///
/// # Errors
///
/// Returns an error if the file cannot be read or its contents are not valid
/// [`Atlas`] JSON.
pub fn load_atlas(path: &Path) -> anyhow::Result<Atlas> {
    let json = std::fs::read_to_string(path)?;
    let atlas = serde_json::from_str::<Atlas>(&json)?;
    Ok(atlas)
}

/// Builds a texture atlas from every supported image in `folder`.
///
/// Each image is matched against `settings.specs` to determine its tile
/// [`Size`], falling back to [`Settings::default_size`]. The source image is
/// then sliced into a grid of that size, and the tiles are shelf-packed into
/// an output image `settings.atlas_width` pixels wide, growing in height as
/// rows fill up.
///
/// Supported input formats are PNG, JPEG, and WebP. Images are processed
/// tallest-first, so each row's height is fixed by its first (tallest) tile.
///
/// The first spec whose identifier matches an image wins; order your `specs`
/// from most to least specific.
///
/// # Returns
///
/// A tuple of the [`Atlas`] (name → [`AtlasEntry`]) and the packed
/// [`RgbaImage`].
///
/// # Errors
///
/// Returns an error if:
/// - a spec's tile width exceeds `atlas_width`,
/// - the folder cannot be read or contains no supported images,
/// - a spec contains an invalid regex,
/// - an image's dimensions are not an exact multiple of its tile size, or
/// - an image fails to open or decode.
pub fn create_atlas(folder: &Path, settings: Settings) -> anyhow::Result<(Atlas, RgbaImage)> {
    check_healthy_settings(&settings)?;
    let images = extract_image_paths(folder)?;
    if images.is_empty() {
        return Err(anyhow!("Folder is empty: {}", folder.display()));
    }
    let mut image_map: Vec<(Size, PathBuf)> = vec![];
    // need the size to unpack the images beneath
    let default = settings.default_size;
    let compiled = settings.get_compiled_specs()?;
    for image in images {
        let size = compiled
            .iter()
            .find(|&ts| ts.identifier.matches(&image))
            .map(|ts| ts.size)
            .unwrap_or(default);

        image_map.push((size, image));
    }

    image_map.sort_unstable_by(|a, b| b.0.h.cmp(&a.0.h));

    let mut x_cur = 0u32;
    let mut y_cur = 0u32;
    let mut atlas_map = BTreeMap::<String, AtlasEntry>::new();
    let first_height = image_map.first().map(|(size, _)| size.h).unwrap();
    let mut last_height = first_height;
    let mut dest = image::RgbaImage::new(settings.atlas_width, first_height);
    // relies on tallest-first sort: first tile of a row is its tallest
    for (size, path_buf) in image_map {
        let dim = image::image_dimensions(&path_buf)?;
        let path = path_buf.as_path();
        if dim.0 % size.w != 0 || dim.1 % size.h != 0 {
            return Err(anyhow!(
                "The dimensions on the image doesn't fits to the given size of '{:?}', image path: '{}'",
                size,
                path.display()
            ));
        }
        // collecting the data
        let mut pos_list: Vec<Pos> = Vec::with_capacity((dim.0 / size.w * dim.1 / size.h) as usize);

        // image creation
        let origin = image::open(path)?;
        for y in 0..dim.1 / size.h {
            for x in 0..dim.0 / size.w {
                // check if the cursor reaches the end
                if x_cur + size.w > settings.atlas_width {
                    // setup a new line
                    x_cur = 0;
                    y_cur += last_height;
                    last_height = size.h;
                    let mut new_image =
                        image::RgbaImage::new(settings.atlas_width, last_height + y_cur);
                    imageops::replace(&mut new_image, &dest, 0, 0);
                    dest = new_image;
                }

                let crop = imageops::crop_imm(&origin, x * size.w, y * size.h, size.w, size.h);
                imageops::replace(&mut dest, &*crop, x_cur as i64, y_cur as i64);
                pos_list.push((x_cur, y_cur).into());

                x_cur += size.w;
            }
        }
        let name: String = path.file_stem().unwrap().to_string_lossy().into();
        if atlas_map.contains_key(&name) {
            log::warn!(
                "Found duplicate: '{name}' - please be sure to have only one image with the same name."
            );
        }
        atlas_map.insert(name, (size, pos_list).into());
    }

    Ok((Atlas { entries: atlas_map }, dest.into()))
}

/// Builds an atlas from `folder_src` and writes it to `folder_dst` as two
/// files: `<name>.png` (the packed image) and `<name>.json` (the [`Atlas`]
/// metadata). `name` defaults to `"atlas"` when `None`.
///
/// The destination folder is created if it does not exist.
///
/// # Errors
///
/// Propagates any error from [`create_atlas`], plus failures writing the
/// output files.
pub fn create_atlas_files(
    folder_src: &Path,
    folder_dst: &Path,
    settings: Settings,
    name: Option<&str>,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(folder_dst)?;
    let (atlas, image) = create_atlas(folder_src, settings)?;

    let json = serde_json::to_string(&atlas)?;
    let name = name.unwrap_or("atlas");
    std::fs::write(folder_dst.join(format!("{name}.json")), json)?;

    image.save(folder_dst.join(format!("{name}.png")))?;

    Ok(())
}

fn extract_image_paths(folder: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut images = vec![];
    for entry in folder.read_dir()? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            log::info!("found subfolder that wont be included: {}", path.display());
            continue;
        }

        let ending = match path.extension() {
            Some(end) => end,
            None => {
                log::info!("found file that wont be included: {}", path.display());
                continue;
            }
        };

        if SUPPORTED_IMAGE_TYPES.contains(&ending.to_str().expect("ending should support utf-8")) {
            images.push(path);
        } else {
            continue;
        }
    }
    return Ok(images);
}

fn check_healthy_settings(settings: &Settings) -> anyhow::Result<()> {
    let found: Vec<_> = settings
        .specs
        .iter()
        .filter(|s| s.size.w > settings.atlas_width)
        .collect();
    match found.is_empty() {
        true => Ok(()),
        false => Err(anyhow!(
            "The atlas width has to be wider then the biggest tile width. Issues are in the Specs: {:?}",
            found
        )),
    }
}
