//! MIDI input handling, including a VL1-flavoured controller map.
//!
//! The VL1 was designed around a breath controller (CC2); everything else here
//! follows the wind-controller conventions that grew up around it.

use crate::engine::Engine;

/// Controller numbers this engine responds to.
pub mod cc {
    pub const MODULATION: u8 = 1;
    pub const BREATH: u8 = 2;
    pub const EMBOUCHURE: u8 = 3;
    pub const VOLUME: u8 = 7;
    pub const EXPRESSION: u8 = 11;
    pub const SCREAM: u8 = 16;
    pub const GROWL: u8 = 17;
    pub const DAMPING: u8 = 18;
    pub const ABSORPTION: u8 = 19;
    pub const SUSTAIN: u8 = 64;
    pub const ALL_SOUND_OFF: u8 = 120;
    pub const ALL_NOTES_OFF: u8 = 123;
}

/// Feed a raw MIDI message (2-3 bytes) to the engine. Unknown messages are
/// ignored. Running status is not handled — pass complete messages.
pub fn handle(engine: &mut Engine, message: &[u8]) {
    if message.is_empty() {
        return;
    }
    let status = message[0] & 0xF0;
    let d1 = message.get(1).copied().unwrap_or(0) & 0x7F;
    let d2 = message.get(2).copied().unwrap_or(0) & 0x7F;

    match status {
        0x80 => engine.note_off(d1),
        0x90 => {
            if d2 == 0 {
                engine.note_off(d1)
            } else {
                engine.note_on(d1, d2)
            }
        }
        0xB0 => control_change(engine, d1, d2),
        0xD0 => engine.controls_mut().aftertouch = d1 as f32 / 127.0,
        0xE0 => {
            let value = ((d2 as i32) << 7 | d1 as i32) - 8192;
            engine.set_pitch_bend(value as f32 / 8192.0);
        }
        _ => {}
    }
}

fn control_change(engine: &mut Engine, number: u8, value: u8) {
    let v = value as f32 / 127.0;
    match number {
        cc::MODULATION => engine.controls_mut().modulation = v,
        // Breath pressure runs slightly past unity so a player can lean on it.
        cc::BREATH => engine.controls_mut().pressure = v * 1.3,
        cc::EMBOUCHURE => engine.controls_mut().embouchure = v * 2.0 - 1.0,
        cc::VOLUME | cc::EXPRESSION => engine.controls_mut().expression = v,
        cc::SCREAM => engine.controls_mut().scream = v,
        cc::GROWL => engine.controls_mut().growl = v,
        cc::DAMPING => engine.controls_mut().damping = v * 4.0 - 2.0,
        cc::ABSORPTION => engine.controls_mut().absorption = v * 2.0 - 1.0,
        cc::SUSTAIN => engine.set_sustain(value >= 64),
        cc::ALL_SOUND_OFF => engine.panic(),
        cc::ALL_NOTES_OFF => {
            for note in 0..128u8 {
                engine.note_off(note);
            }
        }
        _ => {}
    }
}
