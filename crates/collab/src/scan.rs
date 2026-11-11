//! A look through the bytes of an update before the Y library reads them.
//!
//! The library (yrs 0.28.1) reads a value that is a list in a list in a list by calling
//! itself once a level, without a limit: an update of twenty thousand bytes that nests
//! a value ten thousand deep ends the stack of the thread that reads it, and with it
//! the process, which no handler can catch. It also sets aside a table for as many
//! clients as the first bytes name before it reads on, so that five bytes naming four
//! thousand million clients cost it seconds and gigabytes. And where a clock runs past
//! 32 bits or a client identifier past 53, it wraps the number around, or panics in a
//! build that checks its arithmetic.
//!
//! So an update is walked here first, in the same framing the library reads (version 1
//! of the Y update encoding), and refused if a value in it nests deeper than any
//! attribute of a body does, if it names more than it has bytes for, or if it holds
//! anything this walk does not know: where the walk and the library could read the
//! same bytes differently, the update does not reach the library. A body's update
//! needs none of what is refused (a subdocument, a kind of content from another
//! version of the encoding, a number written longer than it needs).

/// How deep a value in an update may nest. The values of a body are attributes:
/// numbers, truths and texts, which do not nest at all.
const MAX_VALUE_DEPTH: usize = 8;

/// The largest client identifier: 53 bits, the whole numbers JavaScript keeps exactly.
const MAX_CLIENT: u64 = (1 << 53) - 1;

const MAX_U32: u64 = 0xffff_ffff;

/// Why an update is not handed to the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The bytes end before what they announce.
    CutShort,
    /// A number is longer or larger than the field it is in takes.
    Number,
    /// It names more clients, changes or values than it has bytes for.
    Count,
    /// A value nests deeper than [`MAX_VALUE_DEPTH`].
    TooDeep,
    /// It holds a subdocument, or content of a kind version 1 of the encoding lacks.
    Unknown,
}

impl Refusal {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::CutShort => "it ends before what it announces",
            Self::Number => "a number in it is longer or larger than its field takes",
            Self::Count => "it names more than it has bytes for",
            Self::TooDeep => "a value in it nests too deep",
            Self::Unknown => "it holds content no body has",
        }
    }
}

/// Walks the changes of an update.
///
/// # Errors
///
/// Returns why the update must not be handed to the Y library.
pub(crate) fn check(update: &[u8]) -> Result<(), Refusal> {
    let mut walk = Walk {
        bytes: update,
        at: 0,
    };
    let length = u64::try_from(update.len()).unwrap_or(u64::MAX);
    for _ in 0..walk.count()? {
        let changes = walk.count()?;
        walk.number(MAX_CLIENT)?;
        // The clock the changes begin at. Each change moves it on by its length: one
        // that says its length (`long`), or one that is no longer than its bytes.
        let clock = walk.number(MAX_U32)?;
        let mut long = 0_u64;
        for _ in 0..changes {
            long = long.saturating_add(walk.change()?);
        }
        if clock.saturating_add(long).saturating_add(length) > MAX_U32 {
            return Err(Refusal::Number);
        }
    }
    // What was deleted: for each client, stretches of its clock.
    for _ in 0..walk.count()? {
        walk.number(MAX_CLIENT)?;
        for _ in 0..walk.count()? {
            let clock = walk.number(MAX_U32)?;
            let stretch = walk.number(MAX_U32)?;
            if clock + stretch > MAX_U32 {
                return Err(Refusal::Number);
            }
        }
    }
    Ok(())
}

struct Walk<'bytes> {
    bytes: &'bytes [u8],
    at: usize,
}

/// A list or a map that is being read: how many of its entries have not begun.
struct Open {
    left: u64,
    map: bool,
}

impl Walk<'_> {
    fn byte(&mut self) -> Result<u8, Refusal> {
        let byte = *self.bytes.get(self.at).ok_or(Refusal::CutShort)?;
        self.at += 1;
        Ok(byte)
    }

    fn skip(&mut self, length: u64) -> Result<(), Refusal> {
        let length = usize::try_from(length).map_err(|_| Refusal::CutShort)?;
        let end = self.at.checked_add(length).ok_or(Refusal::CutShort)?;
        if end > self.bytes.len() {
            return Err(Refusal::CutShort);
        }
        self.at = end;
        Ok(())
    }

    /// A number in the variable-length form: seven bits a byte, the lowest first, the
    /// high bit set on every byte but the last. One that is past `max` is refused: the
    /// library would wrap it around and go on from another number than this walk.
    fn number(&mut self, max: u64) -> Result<u64, Refusal> {
        let mut number = 0_u64;
        for shift in (0..=56).step_by(7) {
            let byte = self.byte()?;
            number |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return if number <= max {
                    Ok(number)
                } else {
                    Err(Refusal::Number)
                };
            }
        }
        Err(Refusal::Number)
    }

    /// How many of something follow. Each takes a byte at least.
    fn count(&mut self) -> Result<u64, Refusal> {
        let count = self.number(MAX_U32)?;
        let left = u64::try_from(self.bytes.len() - self.at).unwrap_or(u64::MAX);
        if count > left {
            return Err(Refusal::Count);
        }
        Ok(count)
    }

    /// A text or bytes: the length, then that many bytes.
    fn text(&mut self) -> Result<(), Refusal> {
        let length = self.number(MAX_U32)?;
        self.skip(length)
    }

    /// The identifier of a change: the client and its clock.
    fn id(&mut self) -> Result<(), Refusal> {
        self.number(MAX_CLIENT)?;
        self.number(MAX_U32)?;
        Ok(())
    }

    /// One change: a struct of the update. Gives the length it says it has, for one
    /// that says so instead of holding what it is long of; nothing for any other.
    fn change(&mut self) -> Result<u64, Refusal> {
        const SKIPPED: u8 = 10;
        const COLLECTED: u8 = 0;
        let info = self.byte()?;
        if matches!(info, SKIPPED | COLLECTED) {
            return self.number(MAX_U32);
        }
        let (after, before) = (info & 0x80 != 0, info & 0x40 != 0);
        if after {
            self.id()?;
        }
        if before {
            self.id()?;
        }
        if !after && !before {
            // The parent: a root by its name, or a change by its identifier.
            if self.number(MAX_U32)? == 1 {
                self.text()?;
            } else {
                self.id()?;
            }
            if info & 0x20 != 0 {
                self.text()?;
            }
        }
        match info & 0x0f {
            // Deleted content: its length.
            1 => return self.number(MAX_U32),
            // Texts of JSON: one more than the number says.
            2 => {
                let more = self.count()?;
                if more >= 1 << 31 {
                    return Err(Refusal::Number);
                }
                for _ in 0..=more {
                    self.text()?;
                }
            }
            // Bytes, a text, an embedded value written as JSON.
            3..=5 => self.text()?,
            // Formatting: the name and the value written as JSON.
            6 => {
                self.text()?;
                self.text()?;
            }
            // A shared type; an XML element has its name.
            7 => match self.byte()? {
                3 => self.text()?,
                0..=2 | 4..=6 | 15 => {}
                _ => return Err(Refusal::Unknown),
            },
            // Values.
            8 => {
                for _ in 0..self.count()? {
                    self.value()?;
                }
            }
            // A subdocument (9), or what version 1 of the encoding does not have.
            _ => return Err(Refusal::Unknown),
        }
        Ok(0)
    }

    /// One value, with all it holds, read without a call a level.
    fn value(&mut self) -> Result<(), Refusal> {
        let mut open: Vec<Open> = Vec::new();
        loop {
            match self.byte()? {
                // Undefined, null, false and true.
                120 | 121 | 126 | 127 => {}
                // A whole number: the sign and six bits, then seven bits a byte.
                125 => {
                    let mut length = 1;
                    while self.byte()? & 0x80 != 0 {
                        length += 1;
                        if length > 10 {
                            return Err(Refusal::Number);
                        }
                    }
                }
                124 => self.skip(4)?,
                122 | 123 => self.skip(8)?,
                // Bytes and a text.
                116 | 119 => self.text()?,
                tag @ (117 | 118) => {
                    if open.len() >= MAX_VALUE_DEPTH {
                        return Err(Refusal::TooDeep);
                    }
                    let left = self.count()?;
                    open.push(Open {
                        left,
                        map: tag == 118,
                    });
                }
                _ => return Err(Refusal::Unknown),
            }
            // What was read ends every list it was the last entry of; the next entry
            // of the innermost list that has one begins.
            loop {
                let Some(innermost) = open.last_mut() else {
                    return Ok(());
                };
                if innermost.left == 0 {
                    open.pop();
                    continue;
                }
                innermost.left -= 1;
                if innermost.map {
                    self.text()?;
                }
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An update of one change by client 5 under the root "default": values.
    fn of_values(values: &[u8], count: u8) -> Vec<u8> {
        let mut bytes = vec![1, 1, 5, 0, 8, 1, 7];
        bytes.extend_from_slice(b"default");
        bytes.push(count);
        bytes.extend_from_slice(values);
        bytes.push(0);
        bytes
    }

    fn nested(depth: usize) -> Vec<u8> {
        let mut value = [117, 1].repeat(depth);
        value.push(126);
        of_values(&value, 1)
    }

    #[test]
    fn a_value_may_nest_a_little_and_no_deeper() {
        assert_eq!(check(&nested(0)), Ok(()));
        assert_eq!(check(&nested(MAX_VALUE_DEPTH)), Ok(()));
        assert_eq!(check(&nested(MAX_VALUE_DEPTH + 1)), Err(Refusal::TooDeep));
        assert_eq!(check(&nested(100_000)), Err(Refusal::TooDeep));
        // Deep in a map, behind entries that are not deep.
        let mut value = vec![118, 2, 1, b'a', 119, 2, b'u', b'u', 1, b'b'];
        value.extend([117, 1].repeat(MAX_VALUE_DEPTH));
        value.push(120);
        assert_eq!(check(&of_values(&value, 1)), Err(Refusal::TooDeep));
    }

    #[test]
    fn letters_that_look_like_lists_are_letters() {
        // The text "uuuu…": bytes 117, inside a text, where they nest nothing.
        let mut value = vec![119, 100];
        value.extend([117; 100]);
        assert_eq!(check(&of_values(&value, 1)), Ok(()));
        let mut change = vec![1, 1, 5, 0, 4, 1, 7];
        change.extend_from_slice(b"default");
        change.push(100);
        change.extend([117; 100]);
        change.push(0);
        assert_eq!(check(&change), Ok(()));
    }

    #[test]
    fn every_kind_of_value_is_walked_to_its_end() {
        let values = [
            vec![127],
            vec![126],
            vec![125, 0x7f],
            vec![125, 0xff, 0x01],
            vec![124, 0, 0, 0, 0],
            vec![123, 0, 0, 0, 0, 0, 0, 0, 0],
            vec![122, 0, 0, 0, 0, 0, 0, 0, 0],
            vec![121],
            vec![120],
            vec![119, 2, b'h', b'i'],
            vec![118, 1, 1, b'k', 117, 2, 126, 127],
            vec![117, 0],
            vec![116, 3, 1, 2, 3],
        ];
        let all: Vec<u8> = values.concat();
        let count = u8::try_from(values.len()).expect("a few values");
        assert_eq!(check(&of_values(&all, count)), Ok(()));
        // One value more than there is: the walk runs into what follows and stops.
        assert!(check(&of_values(&all, count + 2)).is_err(), "cut short");
    }

    #[test]
    fn what_is_named_but_not_there_is_refused() {
        assert_eq!(check(&[]), Err(Refusal::CutShort));
        assert_eq!(check(&[0, 0]), Ok(()), "the empty update");
        // Four thousand million clients, in five bytes.
        assert_eq!(
            check(&[0xff, 0xff, 0xff, 0xff, 0x0f, 0, 0]),
            Err(Refusal::Count)
        );
        // One client with more changes than bytes.
        assert_eq!(check(&[1, 200, 5, 0, 0, 1, 0]), Err(Refusal::Count));
        // A number longer than its field takes.
        assert_eq!(
            check(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01]),
            Err(Refusal::Number)
        );
        // A clock that runs past 32 bits: in what was deleted, and in the changes.
        assert_eq!(
            check(&[0, 1, 5, 1, 0xff, 0xff, 0xff, 0xff, 0x0f, 1]),
            Err(Refusal::Number)
        );
        assert_eq!(
            check(&[0, 1, 5, 1, 0xfe, 0xff, 0xff, 0xff, 0x0f, 1]),
            Ok(())
        );
        assert_eq!(
            check(&[1, 1, 5, 0xff, 0xff, 0xff, 0xff, 0x0f, 0, 1, 0]),
            Err(Refusal::Number)
        );
        assert_eq!(
            check(&[
                1, 2, 5, 0, 0, 0xff, 0xff, 0xff, 0xff, 0x07, 10, 0xff, 0xff, 0xff, 0xff, 0x08, 0
            ]),
            Err(Refusal::Number)
        );
        // A subdocument.
        let mut subdocument = vec![1, 1, 5, 0, 9, 1, 7];
        subdocument.extend_from_slice(b"default");
        subdocument.extend_from_slice(&[1, b'g', 118, 0, 0]);
        assert_eq!(check(&subdocument), Err(Refusal::Unknown));
    }
}
