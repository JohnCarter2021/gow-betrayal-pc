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

pub struct Resources {
    dir: PathBuf,
    packs: HashMap<u32, RpPack>,
}

impl Resources {
    pub fn new(dir: impl AsRef<Path>) -> Resources {
        Resources {
            dir: dir.as_ref().to_path_buf(),
            packs: HashMap::new(),
        }
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
    pub fn image(&mut self, id: u32, palette_key_id: Option<u32>) -> Result<Image, RpError> {
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
    pub image: Image,
    pub frames: Vec<Frame>,
}

impl SpriteSet {
    pub fn frame(&self, index: usize) -> Option<&Frame> {
        self.frames.get(index)
    }
}
