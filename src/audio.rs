//! Audio (slice 7): MIDI resources + playback state.
//!
//! The 11 flag-0 chunks in RP1 (chunks 30-40, resource ids
//! 1054-1064) are standard MIDI files, format 1, division
//! 960 — no decoding needed, unlike `d.java`'s 3GP path
//! (which handles video/3gp, not these). Each file's
//! declared track count matches its actual MTrk chunks.
//!
//! The media player `iM` (a `d` with 12 slots) loads
//! three of them in `n()` case 4:
//! - slot 0: resource 1063 = RP1 chunk 39 (6,274 bytes,
//!   11 tracks — the background music, played by `y(0)`)
//! - slot 1: resource 1064 = RP1 chunk 40 (261 bytes)
//! - slot 2: resource 1062 = RP1 chunk 38 (161 bytes)
//! `y(n)` only plays slots 0, 1, 2; other ids return
//! early. The other 8 MIDI chunks are not loaded by
//! `n()` case 4 — their use (if any) is not yet traced.
//!
//! Playback (slice 8): `Audio` still tracks enabled /
//! current-slot state exactly (the part the game logic
//! depends on), and on Windows it additionally plays the
//! slot's MIDI bytes through winmm (see midi.rs). On
//! other targets the player is inert and state is all
//! there is — same code, same tests, no silent fake.

/// (slot, resource id, RP1 chunk index)
pub const SLOTS: [(usize, u32, usize); 3] = [
    (0, 1063, 39),
    (1, 1064, 40),
    (2, 1062, 38),
];

pub const MIDI_RESOURCE_IDS: [u32; 11] = [
    1054, 1055, 1056, 1057, 1058, 1059, 1060, 1061, 1062, 1063, 1064,
];

/// Minimal MIDI validation: MThd header, format 1 or 0,
/// and the declared track count present as MTrk chunks.
pub fn validate_midi(bytes: &[u8]) -> Result<(u16, u16, u16), String> {
    if bytes.len() < 14 || &bytes[0..4] != b"MThd" {
        return Err("not a MIDI file (no MThd)".to_string());
    }
    let header_len = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if header_len != 6 {
        return Err(format!("unexpected MIDI header length {header_len}"));
    }
    let format = u16::from_be_bytes([bytes[8], bytes[9]]);
    let ntrk = u16::from_be_bytes([bytes[10], bytes[11]]);
    let division = u16::from_be_bytes([bytes[12], bytes[13]]);
    let mut off = 14usize;
    let mut found = 0u16;
    while off + 8 <= bytes.len() {
        if &bytes[off..off + 4] != b"MTrk" {
            break;
        }
        let len = u32::from_be_bytes([bytes[off + 4], bytes[off + 5], bytes[off + 6], bytes[off + 7]]) as usize;
        off += 8 + len;
        found += 1;
    }
    if found != ntrk {
        return Err(format!("track count mismatch: header {ntrk}, found {found}"));
    }
    Ok((format, ntrk, division))
}

/// No Clone/Copy: the player owns a background thread.
/// (Nothing outside audio.rs clones an Audio.)
pub struct Audio {
    pub enabled: bool,
    pub current_slot: Option<usize>,
    player: Option<crate::midi::MidiPlayer>,
}

impl Default for Audio {
    fn default() -> Self {
        Audio {
            enabled: false,
            current_slot: None,
            player: None,
        }
    }
}

impl Audio {
    fn stop_player(&mut self) {
        if let Some(mut p) = self.player.take() {
            p.stop();
        }
    }

    /// `iM.a(boolean)`: enable/disable the player.
    /// Disabling stops any playing song.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.stop_player();
            self.current_slot = None;
        }
    }

    /// `e.y(n)`: play slot n, only for n in {0, 1, 2} and
    /// only when enabled. State only — no sound. Returns
    /// true if a slot started.
    pub fn play(&mut self, slot: usize) -> bool {
        if !self.enabled || slot > 2 {
            return false;
        }
        self.current_slot = Some(slot);
        true
    }

    /// Same as `play`, but with the slot's MIDI bytes: on
    /// Windows the song actually plays (slot 0 loops, the
    /// others play once); elsewhere this is state only.
    /// A parse failure leaves the previous state untouched
    /// and returns false.
    pub fn play_midi(&mut self, slot: usize, bytes: &[u8], looped: bool) -> bool {
        if !self.enabled || slot > 2 {
            return false;
        }
        let song = match crate::midi::parse_song(bytes) {
            Ok(s) => s,
            Err(_) => return false,
        };
        self.stop_player();
        self.player = Some(crate::midi::MidiPlayer::start(song, looped));
        self.current_slot = Some(slot);
        true
    }

    /// `iM.c()`: stop the current player.
    pub fn stop(&mut self) {
        self.stop_player();
        self.current_slot = None;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn midi_validation_and_audio_rules() {
        let data = std::fs::read("../extracted/RP1").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        for &id in super::MIDI_RESOURCE_IDS.iter() {
            let chunk = pack.chunk(crate::rp::split_resource_id(id).1).unwrap();
            let (fmt, ntrk, div) = super::validate_midi(&chunk).unwrap();
            assert_eq!(fmt, 1);
            assert_eq!(div, 960);
            assert!(ntrk >= 3);
        }
        let mut a = super::Audio::default();
        assert!(!a.play(0)); // disabled
        a.set_enabled(true);
        assert!(a.play(0));
        assert_eq!(a.current_slot, Some(0));
        assert!(!a.play(3)); // y() ignores slots > 2
        a.set_enabled(false);
        assert_eq!(a.current_slot, None);
    }
}
