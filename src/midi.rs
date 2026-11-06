//! MIDI playback (slice 8): parse + Windows winmm output.
//!
//! The game's MIDI files are standard MIDI (format 1, division
//! 960 — see audio.rs). This module turns one into a timed list
//! of winmm short messages, and plays it on Windows through the
//! MIDI mapper (usually the Microsoft GS Wavetable Synth).
//!
//! Parser scope, kept to what these files use:
//! - VLQ deltas, running status (channel messages only),
//! - channel messages: Note Off/On (velocity-0 Note On is
//!   normalised to Note Off), Poly Pressure, Control Change,
//!   Program Change, Channel Pressure, Pitch Bend,
//! - meta: Set Tempo (0x51) drives the tick->ms conversion,
//!   End of Track (0x2F) ends a track; other meta is skipped,
//! - SysEx (0xF0/0xF7) is skipped by length.
//! Tracks are merged by absolute tick (stable, so same-tick
//! order follows track order). Tempo changes apply from the
//! tick they occur at, as in a real sequencer.
//!
//! Player: a background thread sends the messages on schedule
//! (sleeps in 5 ms slices so `stop()` is prompt even across a
//! long rest). Slot 0 (music) loops until stopped; slots 1/2
//! play once. On non-Windows targets the player is inert —
//! `Audio` still tracks enabled/current-slot state exactly,
//! and the same code compiles and tests there. This VM has
//! no MIDI synth, so sound itself is verified on the user's
//! PC, like the window was.
//!
//! winmm FFI is `#[cfg(windows)]` only; no POSIX-only API
//! is used anywhere in this module.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// One winmm short message, due after `delay_ms` from the
/// previous message. `msg` packs status | data1<<8 | data2<<16.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedMsg {
    pub delay_ms: u64,
    pub msg: u32,
}

/// A parsed MIDI song: timed short messages + total duration.
#[derive(Debug, Clone)]
pub struct MidiSong {
    pub division: u16,
    pub events: Vec<TimedMsg>,
    pub duration_ms: u64,
}

struct RawEvent {
    tick: u64,
    msg: Option<u32>, // None = tempo event
    tempo_us: u32,
}

fn read_vlq(data: &[u8], pos: &mut usize) -> Result<u64, String> {
    let mut value: u64 = 0;
    loop {
        let b = *data.get(*pos).ok_or("VLQ past end of track")?;
        *pos += 1;
        value = (value << 7) | (b as u64 & 0x7F);
        if b & 0x80 == 0 {
            return Ok(value);
        }
    }
}

fn parse_track(data: &[u8], out: &mut Vec<RawEvent>) -> Result<(), String> {
    let mut pos = 0usize;
    let mut tick: u64 = 0;
    let mut running: Option<u8> = None;
    while pos < data.len() {
        tick += read_vlq(data, &mut pos)?;
        let b = *data.get(pos).ok_or("event past end of track")?;
        let status: u8;
        let mut first_data: Option<u8> = None;
        if b < 0x80 {
            // Data byte: running status must already be set.
            // Consume it: it is the first data byte.
            status = running.ok_or("data byte with no running status")?;
            first_data = Some(b);
            pos += 1;
        } else {
            pos += 1;
            status = b;
            if status < 0xF0 {
                running = Some(status);
            }
        }
        match status {
            0xFF => {
                // Meta event: type, length, data. Resets running
                // status (per the MIDI spec, meta cancels it).
                running = None;
                let mtype = *data.get(pos).ok_or("meta type past end")?;
                pos += 1;
                let len = read_vlq(data, &mut pos)? as usize;
                let payload = data
                    .get(pos..pos + len)
                    .ok_or("meta payload past end of track")?;
                if mtype == 0x51 && len == 3 {
                    let us = ((payload[0] as u32) << 16)
                        | ((payload[1] as u32) << 8)
                        | (payload[2] as u32);
                    out.push(RawEvent {
                        tick,
                        msg: None,
                        tempo_us: us,
                    });
                }
                pos += len;
                if mtype == 0x2F {
                    return Ok(()); // End of Track
                }
            }
            0xF0 | 0xF7 => {
                // SysEx: length-prefixed, skipped. Resets running.
                running = None;
                let len = read_vlq(data, &mut pos)? as usize;
                pos = pos
                    .checked_add(len)
                    .filter(|&p| p <= data.len())
                    .ok_or("sysex past end of track")?;
            }
            0x80..=0xEF => {
                let data_len = if status & 0xF0 == 0xC0 || status & 0xF0 == 0xD0 {
                    1
                } else {
                    2
                };
                let d1 = match first_data {
                    Some(v) => v,
                    None => {
                        let v = *data.get(pos).ok_or("data byte past end")?;
                        pos += 1;
                        v
                    }
                };
                let d2 = if data_len == 2 {
                    let v = *data.get(pos).ok_or("data byte past end")?;
                    pos += 1;
                    v
                } else {
                    0
                };
                // Velocity-0 Note On == Note Off.
                let st = if status & 0xF0 == 0x90 && d2 == 0 {
                    0x80 | (status & 0x0F)
                } else {
                    status
                };
                let msg = (st as u32) | ((d1 as u32) << 8) | ((d2 as u32) << 16);
                out.push(RawEvent {
                    tick,
                    msg: Some(msg),
                    tempo_us: 0,
                });
            }
            _ => return Err(format!("unsupported MIDI status byte {status:#04x}")),
        }
    }
    Ok(())
}

/// Parse a whole MIDI file into timed short messages.
pub fn parse_song(bytes: &[u8]) -> Result<MidiSong, String> {
    let (format, ntrk, division) = crate::audio::validate_midi(bytes)?;
    if format != 0 && format != 1 {
        return Err(format!("unsupported MIDI format {format}"));
    }
    if division & 0x8000 != 0 {
        return Err("SMPTE time division not supported".to_string());
    }
    let division = division & 0x7FFF;
    let mut off = 14usize;
    let mut raw: Vec<RawEvent> = Vec::new();
    for _ in 0..ntrk {
        if off + 8 > bytes.len() || &bytes[off..off + 4] != b"MTrk" {
            return Err("missing MTrk chunk".to_string());
        }
        let len = u32::from_be_bytes([
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ]) as usize;
        let track = bytes
            .get(off + 8..off + 8 + len)
            .ok_or("MTrk past end of file")?;
        parse_track(track, &mut raw)?;
        off += 8 + len;
    }
    // Stable sort by tick: same-tick order follows track order.
    raw.sort_by_key(|e| e.tick);
    let mut events = Vec::new();
    let mut tempo_us: u64 = 500_000; // default 120 BPM
    let mut last_tick: u64 = 0;
    let mut duration_ms: u64 = 0;
    for e in &raw {
        let dt = e.tick - last_tick;
        last_tick = e.tick;
        let delay_ms = dt * tempo_us / (division as u64 * 1000);
        duration_ms += delay_ms;
        if let Some(msg) = e.msg {
            events.push(TimedMsg { delay_ms, msg });
        } else {
            tempo_us = e.tempo_us as u64;
        }
    }
    Ok(MidiSong {
        division,
        events,
        duration_ms,
    })
}

/// Background MIDI player. On Windows it opens the MIDI mapper
/// and sends the song's short messages on schedule; elsewhere
/// it holds no thread and plays nothing (state-only, as before).
pub struct MidiPlayer {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MidiPlayer {
    pub fn start(song: MidiSong, looped: bool) -> MidiPlayer {
        #[cfg(windows)]
        {
            let stop = Arc::new(AtomicBool::new(false));
            let stop2 = stop.clone();
            let thread = std::thread::spawn(move || {
                win::play_loop(&song, looped, &stop2);
            });
            MidiPlayer {
                stop,
                thread: Some(thread),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (song, looped);
            MidiPlayer {
                stop: Arc::new(AtomicBool::new(false)),
                thread: None,
            }
        }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for MidiPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(windows)]
mod win {
    use super::MidiSong;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    const MIDI_MAPPER: u32 = 0xFFFF_FFFF; // (UINT)-1
    const CALLBACK_NULL: u32 = 0;

    #[link(name = "winmm")]
    extern "system" {
        fn midiOutOpen(
            phmo: *mut usize,
            u_device_id: u32,
            dw_callback: usize,
            dw_instance: usize,
            dw_flags: u32,
        ) -> u32;
        fn midiOutShortMsg(hmo: usize, dw_msg: u32) -> u32;
        fn midiOutClose(hmo: usize) -> u32;
    }

    fn wait_ms(ms: u64, stop: &AtomicBool) {
        let mut left = ms;
        while left > 0 {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            let step = left.min(5);
            std::thread::sleep(Duration::from_millis(step));
            left -= step;
        }
    }

    pub fn play_loop(song: &MidiSong, looped: bool, stop: &AtomicBool) {
        let mut handle: usize = 0;
        let rc = unsafe {
            midiOutOpen(
                &mut handle as *mut usize,
                MIDI_MAPPER,
                0,
                0,
                CALLBACK_NULL,
            )
        };
        if rc != 0 || handle == 0 {
            return; // No MIDI device: stay silent, state still tracks.
        }
        loop {
            for ev in &song.events {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                wait_ms(ev.delay_ms, stop);
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let _ = unsafe { midiOutShortMsg(handle, ev.msg) };
            }
            if !looped || stop.load(Ordering::SeqCst) {
                break;
            }
        }
        let _ = unsafe { midiOutClose(handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-built minimal MIDI: format 0, 1 track, division 96.
    /// Note On (ch 0, note 60, vel 100) at tick 0, Note Off at
    /// tick 96 (= 500 ms at the default 120 BPM), then End.
    fn tiny_midi() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"MThd");
        v.extend_from_slice(&6u32.to_be_bytes());
        v.extend_from_slice(&0u16.to_be_bytes());
        v.extend_from_slice(&1u16.to_be_bytes());
        v.extend_from_slice(&96u16.to_be_bytes());
        let track: [u8; 12] = [
            0x00, 0x90, 60, 100, // delta 0, note on
            0x60, 0x80, 60, 0, // delta 96, note off
            0x00, 0xFF, 0x2F, 0x00, // delta 0, end of track
        ];
        v.extend_from_slice(b"MTrk");
        v.extend_from_slice(&(track.len() as u32).to_be_bytes());
        v.extend_from_slice(&track);
        v
    }

    #[test]
    fn tiny_midi_parses_with_correct_timing() {
        let song = parse_song(&tiny_midi()).unwrap();
        assert_eq!(song.division, 96);
        assert_eq!(song.events.len(), 2);
        assert_eq!(song.events[0].delay_ms, 0);
        assert_eq!(song.events[0].msg, 0x90 | (60 << 8) | (100 << 16));
        assert_eq!(song.events[1].delay_ms, 500);
        assert_eq!(song.events[1].msg, 0x80 | (60 << 8));
        assert_eq!(song.duration_ms, 500);
    }

    #[test]
    fn velocity_zero_note_on_becomes_note_off() {
        let mut v = tiny_midi();
        // Track starts at byte 22; the Note Off status is at 27
        // (velocity at 29 is already 0). Flip it to a velocity-0
        // Note On: the parser must normalise it back to Note Off.
        v[27] = 0x90;
        let song = parse_song(&v).unwrap();
        assert_eq!(song.events[1].msg & 0xF0, 0x80);
    }

    /// Running-status regression (v1.4.1, the silent-music bug):
    /// a second Note On that omits its status byte must parse
    /// with the right note AND the right velocity. The parser
    /// used to forget to consume the first data byte in the
    /// running-status branch, so the velocity byte was read as
    /// the next event's delta and the whole track desynchronised
    /// — the slot-0 song came out as 38 note-ons and 2363
    /// note-offs instead of 867 and 867, i.e. silent.
    #[test]
    fn running_status_note_ons_parse_with_correct_notes_and_velocities() {
        let mut v = Vec::new();
        v.extend_from_slice(b"MThd");
        v.extend_from_slice(&6u32.to_be_bytes());
        v.extend_from_slice(&0u16.to_be_bytes());
        v.extend_from_slice(&1u16.to_be_bytes());
        v.extend_from_slice(&96u16.to_be_bytes());
        let track: [u8; 18] = [
            0x00, 0x90, 60, 100, // delta 0, note on 60 vel 100
            0x30, 64, 80, // delta 48, running status: note on 64 vel 80
            0x30, 67, 90, // delta 48, running status: note on 67 vel 90
            0x30, 0x80, 60, 0, // delta 48, note off 60
            0x00, 0xFF, 0x2F, 0x00, // delta 0, end of track
        ];
        v.extend_from_slice(b"MTrk");
        v.extend_from_slice(&(track.len() as u32).to_be_bytes());
        v.extend_from_slice(&track);
        let song = parse_song(&v).unwrap();
        assert_eq!(song.events.len(), 4);
        assert_eq!(song.events[0].msg, 0x90 | (60 << 8) | (100 << 16));
        assert_eq!(song.events[1].msg, 0x90 | (64 << 8) | (80 << 16));
        assert_eq!(song.events[2].msg, 0x90 | (67 << 8) | (90 << 16));
        assert_eq!(song.events[3].msg, 0x80 | (60 << 8));
        // 48 ticks at 120 BPM, division 96 = 250 ms each.
        assert_eq!(song.events[1].delay_ms, 250);
        assert_eq!(song.events[2].delay_ms, 250);
        assert_eq!(song.events[3].delay_ms, 250);
        assert_eq!(song.duration_ms, 750);
    }

    /// The real slot-0 song (RP1 chunk 39) must parse to the
    /// note counts an independent parser finds: 867 note-ons
    /// and 867 note-offs (1752 events). Before the running-
    /// status fix it parsed as 38 note-ons / 2363 note-offs
    /// (2422 events) — the music was effectively silent.
    #[test]
    fn slot0_song_note_counts_match_independent_parse() {
        let data = std::fs::read("../extracted/RP1").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(crate::rp::split_resource_id(1063).1).unwrap();
        let song = parse_song(&chunk).unwrap();
        let (mut on, mut off) = (0, 0);
        for e in &song.events {
            match e.msg & 0xF0 {
                0x90 => on += 1,
                0x80 => off += 1,
                _ => {}
            }
        }
        assert_eq!(song.events.len(), 1752, "total events");
        assert_eq!(on, 867, "note-ons");
        assert_eq!(off, 867, "note-offs");
    }

    #[test]
    fn game_midi_files_parse() {
        let data = std::fs::read("../extracted/RP1").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        for &id in crate::audio::MIDI_RESOURCE_IDS.iter() {
            let chunk = pack.chunk(crate::rp::split_resource_id(id).1).unwrap();
            let song = parse_song(&chunk).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert!(!song.events.is_empty(), "{id}: no events");
            assert!(song.duration_ms > 0, "{id}: zero duration");
            assert!(
                song.events.iter().any(|e| e.msg & 0xF0 == 0x90),
                "{id}: no note-on"
            );
        }
        // Music slot: 1063 should be the longest of the three slots.
        let chunk = pack.chunk(crate::rp::split_resource_id(1063).1).unwrap();
        let music = parse_song(&chunk).unwrap();
        assert!(music.duration_ms > 10_000, "music duration {} ms", music.duration_ms);
    }
}
