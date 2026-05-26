//! Camelot wheel key compatibility.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CamelotKey {
    pub number: u8, // 1..=12
    pub letter: Letter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Letter {
    A, // minor
    B, // major
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMatch {
    Identical,
    Adjacent, // +/-1 step on the wheel, same letter
    Relative, // same number, different letter
    None,
}

impl KeyMatch {
    pub fn score(self) -> f64 {
        match self {
            KeyMatch::Identical => 1.0,
            KeyMatch::Adjacent => 0.7,
            KeyMatch::Relative => 0.6,
            KeyMatch::None => 0.0,
        }
    }
}

impl CamelotKey {
    pub fn parse(s: &str) -> Option<Self> {
        if s.len() < 2 || s.len() > 3 {
            return None;
        }
        let (num_str, letter_str) = s.split_at(s.len() - 1);
        let number: u8 = num_str.parse().ok()?;
        if !(1..=12).contains(&number) {
            return None;
        }
        let letter = match letter_str {
            "A" => Letter::A,
            "B" => Letter::B,
            _ => return None,
        };
        Some(Self { number, letter })
    }

    pub fn compatibility(&self, other: &Self) -> KeyMatch {
        if self == other {
            return KeyMatch::Identical;
        }
        if self.number == other.number && self.letter != other.letter {
            return KeyMatch::Relative;
        }
        if self.letter == other.letter {
            let diff = circular_distance(self.number, other.number, 12);
            if diff == 1 {
                return KeyMatch::Adjacent;
            }
        }
        KeyMatch::None
    }
}

fn circular_distance(a: u8, b: u8, modulo: u8) -> u8 {
    let raw = a.abs_diff(b);
    raw.min(modulo - raw)
}
