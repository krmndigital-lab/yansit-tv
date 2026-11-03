//! Android KeyEvent -> `ui::Key` mapper.

use crate::ui::Key;

pub const ACTION_DOWN: i32 = 0;
pub const ACTION_UP: i32 = 1;

pub fn map(code: i32) -> Option<Key> {
    Some(match code {
        19 => Key::Up,
        20 => Key::Down,
        21 => Key::Left,
        22 => Key::Right,
        23 | 66 | 160 | 96 => Key::Ok,
        4 | 111 | 97 => Key::Back,
        226 | 256 | 82 => Key::Menu,
        67 => Key::Backspace,
        165 | 172 => Key::Info,
        7..=16 => Key::Digit((code - 7) as u8),
        144..=153 => Key::Digit((code - 144) as u8),
        166 | 92 => Key::ChUp,
        167 | 93 => Key::ChDown,
        85 => Key::PlayPause,
        126 => Key::Play,
        127 => Key::Pause,
        86 => Key::Stop,
        90 | 272 | 274 => Key::Forward,
        89 | 273 | 275 => Key::Rewind,
        87 => Key::Next,
        88 => Key::Prev,
        _ => return None,
    })
}

fn repeats(k: Key) -> bool {
    matches!(
        k,
        Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::ChUp
            | Key::ChDown
            | Key::Forward
            | Key::Rewind
            | Key::Backspace
    )
}

#[derive(Default)]
pub struct KeyMap {
    ok_down: bool,
    ok_long: bool,
}

impl KeyMap {
    pub fn on(
        &mut self,
        code: i32,
        action: i32,
        repeat: i32,
        long_press: bool,
        uni: u32,
    ) -> (bool, Option<Key>) {
        let printable = char::from_u32(uni).filter(|c| !c.is_control());
        let k = match (map(code), printable) {
            (None, Some(c)) => Key::Char(c),
            (Some(Key::Digit(_)), Some(c)) if code < 144 => Key::Char(c),
            (Some(k), _) => k,
            (None, None) => return (false, None),
        };
        let down = action == ACTION_DOWN;
        if k == Key::Ok {
            let out = if down && repeat == 0 {
                self.ok_down = true;
                self.ok_long = false;
                None
            } else if down {
                (self.ok_down && !self.ok_long && (long_press || repeat > 0)).then(|| {
                    self.ok_long = true;
                    Key::Hold
                })
            } else {
                let fire = self.ok_down && !self.ok_long;
                self.ok_down = false;
                fire.then_some(Key::Ok)
            };
            return (true, out);
        }
        (true, (down && (repeat == 0 || repeats(k))).then_some(k))
    }
}
