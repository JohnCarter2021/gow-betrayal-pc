//! Save file (slice 4b): the original's `z(1)` / `A(1)`.
//!
//! `z(1)` writes a 76-byte buffer to RecordStore "2"
//! (store id = n + 1). The buffer holds 18 big-endian i32
//! fields, in order: gP, gQ, gN, gO, gV, gW, ho, hp, hq,
//! gZ, ha, gX, gY, bD (level progress), hr, hs, he, hz
//! (arena best kills). That is 72 bytes; the last 4 bytes
//! of the 76-byte buffer are never written (zero).
//! `A(1)` reads the same layout, clamps gN >= 3 and
//! gO >= 2, and derives hm = ho*192 + 1024,
//! hn = hp*192 + 1024 — hm/hn are NOT stored directly.
//! A fresh save (store absent) is gN = 3, gO = 2, all
//! others 0, so hm = hn = 1024.
//!
//! The port's `SaveData` is this layout. Persistence goes
//! through the file-backed `midp::RecordStore` in a save
//! directory supplied by the runner (`--game` / `--window`
//! save-dir argument); without one, the game runs with an
//! in-memory default save, as before.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveData {
    pub g_p: i32,
    pub g_q: i32,
    pub g_n: i32,
    pub g_o: i32,
    pub g_v: i32,
    pub g_w: i32,
    pub ho: i32,
    pub hp: i32,
    pub hq: i32,
    pub g_z: i32,
    pub ha: i32,
    pub g_x: i32,
    pub g_y: i32,
    pub progress: i32, // bD
    pub hr: i32,
    pub hs: i32,
    pub he: i32,
    pub hz: i32,
}

impl Default for SaveData {
    fn default() -> Self {
        SaveData {
            g_p: 0,
            g_q: 0,
            g_n: 3,
            g_o: 2,
            g_v: 0,
            g_w: 0,
            ho: 0,
            hp: 0,
            hq: 0,
            g_z: 0,
            ha: 0,
            g_x: 0,
            g_y: 0,
            progress: 0,
            hr: 0,
            hs: 0,
            he: 0,
            hz: 0,
        }
    }
}

impl SaveData {
    pub fn hm(&self) -> i32 {
        self.ho * 192 + 1024
    }
    pub fn hn(&self) -> i32 {
        self.hp * 192 + 1024
    }

    pub fn to_bytes(&self) -> [u8; 76] {
        let fields = [
            self.g_p, self.g_q, self.g_n, self.g_o, self.g_v, self.g_w, self.ho, self.hp,
            self.hq, self.g_z, self.ha, self.g_x, self.g_y, self.progress, self.hr, self.hs,
            self.he, self.hz,
        ];
        let mut out = [0u8; 76];
        for (i, f) in fields.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&f.to_be_bytes());
        }
        out
    }

    /// Returns None if the buffer is too short (< 72 bytes).
    pub fn from_bytes(bytes: &[u8]) -> Option<SaveData> {
        if bytes.len() < 72 {
            return None;
        }
        let mut fields = [0i32; 18];
        for i in 0..18 {
            fields[i] = i32::from_be_bytes([bytes[i * 4], bytes[i * 4 + 1], bytes[i * 4 + 2], bytes[i * 4 + 3]]);
        }
        let mut save = SaveData {
            g_p: fields[0],
            g_q: fields[1],
            g_n: fields[2],
            g_o: fields[3],
            g_v: fields[4],
            g_w: fields[5],
            ho: fields[6],
            hp: fields[7],
            hq: fields[8],
            g_z: fields[9],
            ha: fields[10],
            g_x: fields[11],
            g_y: fields[12],
            progress: fields[13],
            hr: fields[14],
            hs: fields[15],
            he: fields[16],
            hz: fields[17],
        };
        if save.g_n < 3 {
            save.g_n = 3;
        }
        if save.g_o < 2 {
            save.g_o = 2;
        }
        Some(save)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn save_roundtrip_and_defaults() {
        let mut save = super::SaveData::default();
        save.progress = 3;
        save.hq = 42;
        save.ho = 2;
        assert_eq!(save.hm(), 2 * 192 + 1024);
        let bytes = save.to_bytes();
        assert_eq!(bytes.len(), 76);
        assert_eq!(&bytes[72..], &[0, 0, 0, 0]);
        let back = super::SaveData::from_bytes(&bytes).unwrap();
        assert_eq!(back, save);
    }

    #[test]
    fn save_clamps_and_rejects_short() {
        let mut save = super::SaveData::default();
        save.g_n = 1;
        save.g_o = 0;
        let back = super::SaveData::from_bytes(&save.to_bytes()).unwrap();
        assert_eq!(back.g_n, 3);
        assert_eq!(back.g_o, 2);
        assert!(super::SaveData::from_bytes(&[0u8; 10]).is_none());
    }
}

#[cfg(test)]
mod persistence_tests {
    #[test]
    fn game_loads_save_from_record_store() {
        let dir = std::path::PathBuf::from("/tmp/save-persist-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut g1 = crate::game::Game::new("../extracted");
        g1.set_save_dir(&dir);
        g1.boot().unwrap();
        g1.save.progress = 3;
        g1.save.hq = 42;
        g1.persist_save();
        let mut g2 = crate::game::Game::new("../extracted");
        g2.set_save_dir(&dir);
        g2.boot().unwrap();
        assert_eq!(g2.save.progress, 3);
        assert_eq!(g2.save.hq, 42);
        assert_eq!(g2.menu_progress, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
