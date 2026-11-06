//! Resource loading for the port, mirroring `a.m` / `a.d` / `a.c` / `a.b`
//! in `decompiled/a.java`.
//!
//! A pack is loaded on demand by file number (RP<n>) and cached; the
//! currently loaded pack in the original is `a.t` with chunk sizes `a.s`
//! and flags `a.f`. Here we cache every pack we load (the jar is only
//! 415 KB, so caching is cheap and avoids the original's pack-reload
//! dance). Resource ids are `(file << 10) | chunk` (`rp::split_resource_id`).

use crate::midp::Image;
use crate::rp::{parse_pack, parse_string_table, RpError, RpPack};
use crate::sprite::{parse_sprite_def, Frame, SpriteDef};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// HD art scale: AI-upscaled sheets are 4x the jar's sheets.
pub const HD_SCALE: usize = 4;

pub struct Resources {
    dir: PathBuf,
    packs: HashMap<u32, RpPack>,
    /// True once HD art is enabled (see `enable_hd`): sprite images
    /// are then loaded from `<dir>/hd/` when a matching file exists.
    pub hd_enabled: bool,
    /// Decoded images, shared by every draw call. PNG decoding (the
    /// std-only DEFLATE inflater) takes 100-900 ms per HD sheet, and
    /// the game loads the same sheets every frame (player, enemies,
    /// tileset, HUD, font, ...), so without this cache HD ran at ~1
    /// FPS. Images are immutable once decoded, so sharing them with
    /// Rc is safe.
    images: HashMap<(u32, Option<u32>), Rc<Image>>,
}

impl Resources {
    pub fn new(dir: impl AsRef<Path>) -> Resources {
        Resources {
            dir: dir.as_ref().to_path_buf(),
            packs: HashMap::new(),
            hd_enabled: false,
            images: HashMap::new(),
        }
    }

    /// Enable HD art if an `hd/` folder with upscaled sheets sits next
    /// to the RP packs. Returns true when HD is active. Off by default
    /// so tests and headless modes keep the original art unless they
    /// ask for HD explicitly; the window enables it when present.
    pub fn enable_hd(&mut self) -> bool {
        let hd = self.dir.join("hd");
        let on = hd.is_dir()
            && fs::read_dir(&hd)
                .map(|mut it| it.any(|e| {
                    e.map(|e| {
                        e.path()
                            .extension()
                            .map(|x| x == "png")
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
                }))
                .unwrap_or(false);
        self.hd_enabled = on;
        on
    }

    /// Framebuffer render scale for this resource set: 4 with HD art
    /// (960x1280), 1 otherwise (240x320).
    pub fn render_scale(&self) -> usize {
        if self.hd_enabled {
            HD_SCALE
        } else {
            1
        }
    }

    /// HD sheet file for an image: `<dir>/hd/{image_id}_{key_id}.png`
    /// where key_id is the palette-key resource id, or 0 for the base
    /// palette. Files are produced by `hd/batch_upscale.py` (4x
    /// Real-ESRGAN) from the user's own jar.
    fn hd_path(&self, id: u32, palette_key_id: Option<u32>) -> PathBuf {
        self.dir.join("hd").join(format!(
            "{}_{}.png",
            id,
            palette_key_id.unwrap_or(0)
        ))
    }

    fn pack(&mut self, file: u32) -> Result<&RpPack, RpError> {
        if !self.packs.contains_key(&file) {
            let path = self.dir.join(format!("RP{file}"));
            let data = fs::read(&path)
                .map_err(|e| RpError(format!("{}: {e}", path.display())))?;
            let pack = parse_pack(&data)?;
            self.packs.insert(file, pack);
        }
        Ok(self.packs.get(&file).unwrap())
    }

    pub fn chunk(&mut self, id: u32) -> Result<Vec<u8>, RpError> {
        let (file, chunk) = crate::rp::split_resource_id(id);
        let pack = self.pack(file)?;
        pack.chunk(chunk)
            .map(|c| c.to_vec())
            .ok_or_else(|| RpError(format!("resource {id} (RP{file} chunk {chunk}) missing")))
    }

    pub fn chunk_flag(&mut self, id: u32) -> Result<u8, RpError> {
        let (file, chunk) = crate::rp::split_resource_id(id);
        let pack = self.pack(file)?;
        pack.flags
            .get(chunk)
            .copied()
            .ok_or_else(|| RpError(format!("resource {id} flag missing")))
    }

    /// String table (RP flag 250, single chunk), `a.f`.
    pub fn strings(&mut self, id: u32) -> Result<Vec<String>, RpError> {
        let chunk = self.chunk(id)?;
        parse_string_table(&chunk)
    }

    /// PNG image (RP flag 253), optionally with a palette key applied
    /// (`a.c`: key chunk id, or None for the base palette).
    /// With HD enabled, an upscaled sheet in `hd/` is used instead
    /// when one exists for this (image, key) and is exactly 4x the
    /// jar image; otherwise the jar image is used (scale 1).
    /// The decoded image is cached and shared (see `images`): the
    /// same sheets are requested every frame, and re-decoding an HD
    /// sheet costs 100-900 ms.
    pub fn image(&mut self, id: u32, palette_key_id: Option<u32>) -> Result<Rc<Image>, RpError> {
        let key = (id, palette_key_id);
        if let Some(img) = self.images.get(&key) {
            return Ok(Rc::clone(img));
        }
        let img = Rc::new(self.load_image(id, palette_key_id)?);
        self.images.insert(key, Rc::clone(&img));
        Ok(img)
    }

    fn load_image(&mut self, id: u32, palette_key_id: Option<u32>) -> Result<Image, RpError> {
        if self.hd_enabled {
            let path = self.hd_path(id, palette_key_id);
            if let Ok(bytes) = fs::read(&path) {
                if let Ok(mut img) = Image::from_png(&bytes) {
                    // Verify against the jar image: an HD sheet must
                    // be exactly HD_SCALE times the original, else a
                    // stale or wrong file would corrupt every frame.
                    if let Ok(orig) = self.image_from_jar(id, palette_key_id) {
                        if img.width == orig.width * HD_SCALE
                            && img.height == orig.height * HD_SCALE
                        {
                            img.scale = HD_SCALE;
                            return Ok(img);
                        }
                        return Ok(orig);
                    }
                    img.scale = HD_SCALE;
                    return Ok(img);
                }
            }
        }
        self.image_from_jar(id, palette_key_id)
    }

    fn image_from_jar(&mut self, id: u32, palette_key_id: Option<u32>) -> Result<Image, RpError> {
        let mut png = self.chunk(id)?;
        if let Some(key_id) = palette_key_id {
            let key = self.chunk(key_id)?;
            png = crate::sprite::apply_palette_key(&png, &key)?;
        }
        Image::from_png(&png).map_err(|e| RpError(e))
    }

    /// Sprite set: definition (flag 254) + its image, `a.b`.
    /// `palette_key_id` is the key chunk for the image, or None.
    pub fn sprite_set(
        &mut self,
        def_id: u32,
        palette_key_id: Option<u32>,
    ) -> Result<SpriteSet, RpError> {
        let def_chunk = self.chunk(def_id)?;
        let def: SpriteDef = parse_sprite_def(&def_chunk)?;
        let (file, _) = crate::rp::split_resource_id(def_id);
        // Image chunk index is pack-relative (see sprite.rs): the image
        // lives in the same pack as the definition.
        let image_id = (file << 10) | (def.image_chunk as u32);
        let image = self.image(image_id, palette_key_id)?;
        Ok(SpriteSet {
            image,
            frames: def.frames,
        })
    }
}

#[derive(Debug, Clone)]
pub struct SpriteSet {
    /// Shared decoded image (see `Resources::images`): cloning the
    /// Rc is cheap, so a sprite set can be rebuilt every frame
    /// without re-decoding its sheet.
    pub image: Rc<Image>,
    pub frames: Vec<Frame>,
}

impl SpriteSet {
    pub fn frame(&self, index: usize) -> Option<&Frame> {
        self.frames.get(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hd_sheets_load_at_4x_with_matching_dimensions() {
        let mut res = Resources::new("../extracted");
        assert_eq!(res.render_scale(), 1);
        assert!(res.enable_hd(), "hd/ folder should exist in ../extracted");
        assert_eq!(res.render_scale(), HD_SCALE);
        // Tileset: base palette.
        let base = res.image(2058, None).unwrap();
        assert_eq!(base.scale, HD_SCALE);
        let orig = res.image_from_jar(2058, None).unwrap();
        assert_eq!(base.width, orig.width * HD_SCALE);
        assert_eq!(base.height, orig.height * HD_SCALE);
        // A level-3 palette variant of the tileset (key 2070).
        let var = res.image(2058, Some(2070)).unwrap();
        assert_eq!(var.scale, HD_SCALE);
        assert_eq!(var.width, orig.width * HD_SCALE);
        // Sprite set: the image is HD, the frame table stays logical.
        let set = res.sprite_set(28673, None).unwrap();
        assert_eq!(set.image.scale, HD_SCALE);
        assert_eq!(set.image.width, 258 * HD_SCALE);
        assert_eq!(set.image.height, 55 * HD_SCALE);
        assert!(set.frames.iter().all(|f| f.w <= 29 && f.h <= 18));
    }

    #[test]
    fn hd_disabled_by_default_and_falls_back_without_hd() {
        let mut res = Resources::new("../extracted");
        assert!(!res.hd_enabled);
        let img = res.image(2058, None).unwrap();
        assert_eq!(img.scale, 1);
    }
}
