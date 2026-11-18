//! `cargo xtask check`, in the step "names": the words no file may hold.
//!
//! A banned word is refused in any case, alone or inside a longer word, in the text
//! and in the path of every file git tracks or would track. Nothing is excepted.
//!
//! The list does not say the words. A word is known by how many letters it has and
//! by the SHA-256 of those letters in lowercase, so every run of letters in a file is
//! read through a window of that many letters, and the window's digest is compared.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use sha2::{Digest as _, Sha256};

use crate::{names, steps};

/// The SHA-256 of a word's lowercase letters.
type Digest = [u8; 32];

/// The banned words: the number of letters of each and its digest in hexadecimal.
///
/// A word is added by its digest alone: `printf %s <the word in lowercase> | sha256sum`.
const BANNED: [(usize, &str); 6] = [
    (
        10,
        "7267391c427b5cef8b2f0a37903f2d460ba94e8cf09bb02eacf6310c8c4a958d",
    ),
    (
        10,
        "f1b7401bf3d4b49f15bedafd1620d177676ee171f1f478c0465b1cd7cc1dbdd5",
    ),
    (
        5,
        "d60d066d1880fa3927ee245ac461505237168992c474d6e6a2abbc22741dda87",
    ),
    (
        5,
        "2e1c8bae97f03ff81a32b99150ba5f6b139e2d7373ca046304ea85d9f4a9b34e",
    ),
    (
        4,
        "4473badd3d9aa548d11f9d28b5340d0fc21f812b22ce2ba9b2624f3d75e1a56f",
    ),
    (
        4,
        "1156331605d0331725171a37bef8adda2fa384b1310ebd7cb5554d1bd25a7abf",
    ),
];

/// The banned words, by their number of letters.
#[derive(Debug)]
struct Words {
    by_letters: BTreeMap<usize, Vec<Digest>>,
}

impl Words {
    /// Reads a list of words as [`BANNED`] writes them.
    fn parse(listed: &[(usize, &str)]) -> Result<Self> {
        let mut by_letters: BTreeMap<usize, Vec<Digest>> = BTreeMap::new();
        for &(letters, hexadecimal) in listed {
            if letters == 0 {
                bail!("a banned word has letters: {hexadecimal} is listed with none");
            }
            by_letters
                .entry(letters)
                .or_default()
                .push(digest_of_hexadecimal(hexadecimal)?);
        }
        Ok(Self { by_letters })
    }

    /// The number of letters of the shortest banned word, if any word is banned.
    fn shortest(&self) -> Option<usize> {
        self.by_letters.keys().next().copied()
    }

    /// Whether a run of lowercase letters holds a banned word.
    fn are_in_run(&self, run: &str) -> bool {
        if run.is_empty() {
            return false;
        }
        // Where each letter begins, and where the last one ends.
        let starts: Vec<usize> = run
            .char_indices()
            .map(|(start, _)| start)
            .chain([run.len()])
            .collect();
        let letters_in_run = starts.len() - 1;
        self.by_letters
            .iter()
            .take_while(|&(&letters, _)| letters <= letters_in_run)
            .any(|(&letters, digests)| {
                starts.windows(letters + 1).any(|window| {
                    let word = &run[window[0]..window[letters]];
                    let digest: Digest = Sha256::digest(word.as_bytes()).into();
                    digests.contains(&digest)
                })
            })
    }
}

/// A reading of texts for the banned words. It remembers the runs of letters it found
/// clean, as they were written, so that a word the files repeat is digested once.
#[derive(Debug)]
struct Reading<'words> {
    words: &'words Words,
    clean: HashSet<String>,
}

impl<'words> Reading<'words> {
    fn new(words: &'words Words) -> Self {
        Self {
            words,
            clean: HashSet::new(),
        }
    }

    /// Whether the text holds a banned word, in any case, alone or inside a longer one.
    fn finds_in(&mut self, text: &str) -> bool {
        let Some(shortest) = self.words.shortest() else {
            return false;
        };
        // A run has at most as many letters as bytes.
        text.split(|character: char| !character.is_alphabetic())
            .filter(|run| run.len() >= shortest)
            .any(|run| self.finds_in_run(run))
    }

    fn finds_in_run(&mut self, run: &str) -> bool {
        if self.clean.contains(run) {
            return false;
        }
        if self.words.are_in_run(&run.to_lowercase()) {
            return true;
        }
        self.clean.insert(run.to_owned());
        false
    }
}

/// Fails if a file has a banned word in its text or in its path.
///
/// # Errors
///
/// Fails on what it finds, and if git cannot list the files or one cannot be read.
pub(crate) fn run(root: &Path) -> Result<()> {
    let words = Words::parse(&BANNED)?;
    let files = names::tracked_files(root)?;
    let mut reading = Reading::new(&words);
    let mut hits = Vec::new();
    for file in &files {
        let text =
            names::tracked_text(&root.join(file)).with_context(|| format!("reading {file}"))?;
        hits.extend(check_file(file, &text, &mut reading));
    }
    if hits.is_empty() {
        steps::say(&format!(
            "    no banned word in the {} files, nor in their paths",
            files.len()
        ));
        return Ok(());
    }
    for hit in &hits {
        steps::say(&format!("    {hit}"));
    }
    steps::say("    these hold a banned word (xtask/src/banned.rs): say it in our own words");
    bail!("{} places have a banned word", hits.len())
}

/// The hits of one file: `path: in its name`, then `path:line` for each line.
fn check_file(file: &str, text: &str, reading: &mut Reading<'_>) -> Vec<String> {
    let mut hits = Vec::new();
    if reading.finds_in(file) {
        hits.push(format!("{file}: in its name"));
    }
    for (index, line) in text.lines().enumerate() {
        if reading.finds_in(line) {
            hits.push(format!("{file}:{}", index + 1));
        }
    }
    hits
}

/// The 32 bytes that 64 hexadecimal digits write.
fn digest_of_hexadecimal(hexadecimal: &str) -> Result<Digest> {
    let mut digest = [0_u8; 32];
    if hexadecimal.len() != 64 || !hexadecimal.is_ascii() {
        bail!("a digest is 64 hexadecimal digits: {hexadecimal}");
    }
    for (byte, pair) in digest.iter_mut().zip(hexadecimal.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).context("the digits are ASCII")?;
        *byte = u8::from_str_radix(pair, 16)
            .with_context(|| format!("a digest is 64 hexadecimal digits: {hexadecimal}"))?;
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    /// The list that bans the given words, written as [`BANNED`] is.
    fn banning(banned: &[&str]) -> Words {
        let listed: Vec<(usize, String)> = banned
            .iter()
            .map(|word| {
                let mut hexadecimal = String::new();
                for byte in Sha256::digest(word.as_bytes()) {
                    write!(hexadecimal, "{byte:02x}").expect("a string takes what is written");
                }
                (word.chars().count(), hexadecimal)
            })
            .collect();
        let listed: Vec<(usize, &str)> = listed
            .iter()
            .map(|(letters, hexadecimal)| (*letters, hexadecimal.as_str()))
            .collect();
        Words::parse(&listed).expect("the list is read")
    }

    #[test]
    fn a_word_is_found_in_any_case_and_inside_a_longer_word() {
        let words = banning(&["gadget", "가나다라"]);
        let mut reading = Reading::new(&words);
        for text in [
            "see the gadget docs",
            "see the GADGET docs",
            "the Gadgets of old",
            "old_gadget_port = 1",
            "megaGadgetry",
            "https://gadget.example/docs",
            "가나다라의 문서",
            "옛가나다라",
        ] {
            assert!(reading.finds_in(text), "{text:?} holds a banned word");
        }
    }

    #[test]
    fn a_plain_word_and_a_word_cut_apart_are_not_found() {
        let words = banning(&["gadget", "가나다라"]);
        let mut reading = Reading::new(&words);
        for text in [
            "",
            "the original file name",
            "a gadge, and a t",
            "gad-get",
            "gadge7",
            "가나다 라",
            "다라가나",
        ] {
            assert!(!reading.finds_in(text), "{text:?} holds no banned word");
        }
    }

    #[test]
    fn a_file_is_reported_by_its_name_and_by_its_lines() {
        let words = banning(&["gadget"]);
        let mut reading = Reading::new(&words);
        assert_eq!(
            check_file(
                "docs/GADGET-notes.md",
                "our notes\nonce a Gadget\n\nand gadgets\nand gadgets again\n",
                &mut reading
            ),
            [
                "docs/GADGET-notes.md: in its name",
                "docs/GADGET-notes.md:2",
                "docs/GADGET-notes.md:4",
                "docs/GADGET-notes.md:5"
            ]
        );
        assert_eq!(
            check_file("docs/notes.md", "our notes\n", &mut reading),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_list_of_this_workspace_is_read_and_a_wrong_one_is_refused() {
        let words = Words::parse(&BANNED).expect("the list is read");
        assert_eq!(
            words.by_letters.values().map(Vec::len).sum::<usize>(),
            BANNED.len()
        );
        assert!(Words::parse(&[(0, BANNED[0].1)]).is_err(), "no letters");
        assert!(Words::parse(&[(6, "7267391c")]).is_err(), "a short digest");
        let not_hexadecimal = "z".repeat(64);
        assert!(
            Words::parse(&[(6, not_hexadecimal.as_str())]).is_err(),
            "digits that are not hexadecimal"
        );
    }
}
