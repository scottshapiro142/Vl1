//! Computer-keyboard to MIDI note mapping.
//!
//! The usual two-row tracker layout: the `z` row is one octave with its black
//! keys on `s d g h j`, and the `q` row is the octave above with its black keys
//! on `2 3 5 6 7`. The rows overlap by an octave, so together they cover a
//! little over two and a half.
//!
//! This is deliberately free of any terminal or MIDI dependency so that it can
//! be tested without a keyboard attached.

/// Lowest and highest octave the transpose control will move to. Octave 4 puts
/// `z` on middle C.
pub const MIN_OCTAVE: i32 = -1;
pub const MAX_OCTAVE: i32 = 8;

/// Semitones above the current octave's C for a given key, if it is a note key.
pub fn semitone(key: char) -> Option<i32> {
    let s = match key.to_ascii_lowercase() {
        // Lower row: one octave from C, plus a few notes into the next.
        'z' => 0,
        's' => 1,
        'x' => 2,
        'd' => 3,
        'c' => 4,
        'v' => 5,
        'g' => 6,
        'b' => 7,
        'h' => 8,
        'n' => 9,
        'j' => 10,
        'm' => 11,
        ',' => 12,
        'l' => 13,
        '.' => 14,
        ';' => 15,
        '/' => 16,
        // Upper row: starts an octave higher.
        'q' => 12,
        '2' => 13,
        'w' => 14,
        '3' => 15,
        'e' => 16,
        'r' => 17,
        '5' => 18,
        't' => 19,
        '6' => 20,
        'y' => 21,
        '7' => 22,
        'u' => 23,
        'i' => 24,
        '9' => 25,
        'o' => 26,
        '0' => 27,
        'p' => 28,
        _ => return None,
    };
    Some(s)
}

/// The MIDI note a key plays in the given octave, or `None` if the key is not a
/// note key or the result would fall outside MIDI's range.
pub fn note(key: char, octave: i32) -> Option<u8> {
    let semis = semitone(key)?;
    // Octave 4 => C4 => MIDI 60.
    let n = 12 * (octave + 1) + semis;
    if (0..=127).contains(&n) {
        Some(n as u8)
    } else {
        None
    }
}

/// True if this key plays a note (as opposed to being a command key).
pub fn is_note_key(key: char) -> bool {
    semitone(key).is_some()
}

/// A printable diagram of the layout, for the player's reference.
pub fn layout_help() -> &'static str {
    "  black   2 3   5 6 7   9 0        s d   g h j\n\
     \x20 white  q w e r t y u i o p     z x c v b n m , . /\n\
     \x20        (octave above)          (current octave)"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn z_row_starts_at_middle_c() {
        assert_eq!(note('z', 4), Some(60));
        assert_eq!(note('m', 4), Some(71));
        assert_eq!(note(',', 4), Some(72));
    }

    #[test]
    fn q_row_is_an_octave_above_the_z_row() {
        for (low, high) in [('z', 'q'), ('s', '2'), ('x', 'w'), ('m', 'u')] {
            assert_eq!(
                note(high, 4).unwrap(),
                note(low, 4).unwrap() + 12,
                "{high} should be an octave above {low}"
            );
        }
    }

    #[test]
    fn rows_form_a_chromatic_scale() {
        let row = "zsxdcvgbhnjm";
        for (i, key) in row.chars().enumerate() {
            assert_eq!(note(key, 4), Some(60 + i as u8));
        }
    }

    #[test]
    fn octave_shifts_by_twelve() {
        assert_eq!(note('z', 3), Some(48));
        assert_eq!(note('z', 5), Some(72));
    }

    #[test]
    fn out_of_range_notes_are_rejected_not_wrapped() {
        assert_eq!(note('z', MIN_OCTAVE), Some(0));
        assert_eq!(note('z', MAX_OCTAVE), Some(108));
        // The top of the upper row runs past MIDI 127 in the highest octave.
        assert_eq!(note('p', MAX_OCTAVE), None);
    }

    #[test]
    fn command_keys_are_not_notes() {
        for key in ['1', '4', '8', '[', ']', '-', '=', ' '] {
            assert!(!is_note_key(key), "{key} should not play a note");
        }
    }

    #[test]
    fn uppercase_plays_the_same_note() {
        assert_eq!(note('Z', 4), note('z', 4));
    }
}
