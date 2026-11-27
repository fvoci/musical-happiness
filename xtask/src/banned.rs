//! `cargo xtask check`, in the step "names": the words and phrases no file may hold.
//!
//! A banned word is refused in any case, alone or inside a longer word, in the text
//! and in the path of every file git tracks or would track. Nothing is excepted.
//!
//! The names of repositories and folders that are not this one's ([`PRIVATE_NAMES`])
//! are refused the same way, in the characters that paths and identifiers are made
//! of, however they are joined (`a-b`, `a_b`, `AB`).
//!
//! Narrow phrases that explain code by where it came from ([`BANNED_PHRASES`]) and
//! retired identifiers ([`RETIRED_TOKENS`]) are also refused, in the text and in the
//! path of every tracked file but the third-party sources vendored under `vendor/`.
//!
//! Neither list says what it refuses. A word is known by how many letters it has and
//! by the SHA-256 of those letters in lowercase, so every run of letters in a file is
//! read through a window of that many letters, and the window's digest is compared; a
//! name likewise, in a run of the characters of a path ([`folded_name`]).

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
const BANNED: [(usize, &str); 10] = [
    (
        12,
        "4a7fed951cb0747c34015743118c82a0363a486c87f76acec059bc6cfb91ec60",
    ),
    (
        10,
        "7267391c427b5cef8b2f0a37903f2d460ba94e8cf09bb02eacf6310c8c4a958d",
    ),
    (
        10,
        "f1b7401bf3d4b49f15bedafd1620d177676ee171f1f478c0465b1cd7cc1dbdd5",
    ),
    (
        10,
        "51276467ed49274be5648cc47cbae3755b37f45a65b87d5ebe36b57742ee2e9c",
    ),
    (
        9,
        "842a2f40f327b6b111b4191409c96309cc17ecb8a3a71f8335e7985b8426d430",
    ),
    (
        7,
        "d729259911fb601d8e4cb68d88b6ef7a1486a7199d769a435d58a0eb4bb75ad1",
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

/// The names of repositories and folders that are not this one's to tell of: the number
/// of characters of each, folded as [`folded_name`] folds it, and the digest of those.
///
/// A name is added by its digest alone: `printf %s <the name, lowercase, without - and _>
/// | sha256sum`. A folder is named with the slashes around it, so that it is found in
/// any path that goes through it.
const PRIVATE_NAMES: [(usize, &str); 4] = [
    (
        16,
        "5428eb960dc395e10ddd5045fb6a23c24cb81cd568e4c54f7efadaf5b941fcb7",
    ),
    (
        17,
        "a33e576ab0edcf385fe3f01e4876e5bef232883faf3df4f70e659b0263152b96",
    ),
    (
        7,
        "18551096f0edeaaab35449c12667a5d6349461b5d45d5422eaa0784a50f6b6a7",
    ),
    (
        13,
        "0ef05cab22de8cfe2bde5e5437561e8fe7d4c32f143b2728f50d96fc4d33cb41",
    ),
];

/// Narrow phrases that explain code by where it came from, refused in tracked project
/// files (matched case-insensitively), and the names of another language's standard
/// library given as the reason for code: the code says what it does, or names the
/// standard it follows.
///
/// Kept narrow so ordinary phrases such as `"original file"`, `"original card"`,
/// `"the original board exists"`, `"원본 이미지"`, or `"원본 파일"` are not flagged. Some
/// are refused although a copy could say them of its source (`as the original`, the
/// original followed by a name in backticks): there the copy's text says `the source`.
/// Every entry must be lowercase; built with `concat!` so this file stays free of
/// literal matches. A phrase is also found across the lines of a comment
/// ([`LINES_JOINED`]), and in a file's path.
const BANNED_PHRASES: [&str; 17] = [
    concat!("ports ", "server/"),
    concat!("the ", "original's"),
    concat!("the ", "original server"),
    concat!("formats of ", "the original"),
    concat!("as ", "the original"),
    concat!("the ", "original did"),
    concat!("the ", "original does"),
    concat!("the ", "original app"),
    concat!("the ", "original tree"),
    concat!("the ", "original `"),
    concat!("원본", "의 "),
    concat!("원본", "과 "),
    concat!("원본", "과의"),
    concat!("원본", "에서 옮"),
    concat!("http.serve", "content"),
    concat!("check", "preconditions"),
    concat!("bytes.trim", "space"),
];

/// Retired pre-launch identifiers refused in tracked project files: in any case, inside
/// a longer name (with a prefix such as `old` or a suffix such as `_idx`), and in a
/// file's path, unless a lowercase letter goes on after them, as in `createAttachment` or
/// `updateAttributes`. Every entry is matched in lowercase; the tests hold the cases.
const RETIRED_TOKENS: [&str; 5] = [
    concat!("create", "At"),
    concat!("update", "At"),
    concat!("create", "_at"),
    concat!("update", "_at"),
    concat!("UserPreferences", "Patch"),
];

/// Path prefixes exempt from [`BANNED_PHRASES`] and [`RETIRED_TOKENS`] (never from
/// [`BANNED`]): the third-party sources as they were vendored, and nothing written here
/// beside them (the import script and its lists under `vendor/rhwp-studio/`).
const PHRASE_EXEMPT_PREFIXES: [&str; 1] = ["vendor/rhwp-studio/upstream/"];

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
    fn any_in_run(&self, run: &str) -> bool {
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

/// A reading of texts for the banned words and the private names. It remembers the runs
/// it found clean, as they were written, so that a run the files repeat is digested once.
#[derive(Debug)]
struct Reading<'lists> {
    words: &'lists Words,
    names: &'lists Words,
    clean: HashSet<String>,
    clean_names: HashSet<String>,
}

impl<'lists> Reading<'lists> {
    fn new(words: &'lists Words, names: &'lists Words) -> Self {
        Self {
            words,
            names,
            clean: HashSet::new(),
            clean_names: HashSet::new(),
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
        if self.words.any_in_run(&run.to_lowercase()) {
            return true;
        }
        self.clean.insert(run.to_owned());
        false
    }

    /// Whether the text holds a private name, in a run of the characters of a path or an
    /// identifier, however its parts are joined.
    fn finds_name_in(&mut self, text: &str) -> bool {
        let Some(shortest) = self.names.shortest() else {
            return false;
        };
        text.split(|character: char| !is_name_character(character))
            .filter(|run| run.len() >= shortest)
            .any(|run| self.finds_name_in_run(run))
    }

    fn finds_name_in_run(&mut self, run: &str) -> bool {
        if self.clean_names.contains(run) {
            return false;
        }
        if self.names.any_in_run(&folded_name(run)) {
            return true;
        }
        self.clean_names.insert(run.to_owned());
        false
    }
}

/// Whether `character` can be part of a name in a path or an identifier: a letter, a
/// digit, or what joins their parts and the folders of a path.
fn is_name_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '-' | '_' | '.' | '/' | '~')
}

/// A run of name characters as [`PRIVATE_NAMES`] lists it: in lowercase, and without the
/// `-` and `_` that join words, so that `a-b`, `a_b` and `AB` are one name.
fn folded_name(run: &str) -> String {
    run.chars()
        .filter(|character| !matches!(character, '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Fails if a file has a banned word, a private name or a banned phrase in its text or in
/// its path.
///
/// # Errors
///
/// Fails on what it finds, and if git cannot list the files or one cannot be read.
pub(crate) fn run(root: &Path) -> Result<()> {
    let words = Words::parse(&BANNED)?;
    let names = Words::parse(&PRIVATE_NAMES)?;
    let Found { files, hits } = find(root, &words, &names, &BANNED_PHRASES)?;
    if hits.is_empty() {
        steps::say(&format!(
            "    no banned word or phrase in the {files} files, nor in their paths"
        ));
        return Ok(());
    }
    for hit in &hits {
        steps::say(&format!("    {hit}"));
    }
    steps::say(
        "    these hold a banned word or phrase (xtask/src/banned.rs): say it in our own words",
    );
    bail!("{} places have a banned word or phrase", hits.len())
}

/// What a reading of the files under a root found.
#[derive(Debug)]
struct Found {
    /// How many files were read.
    files: usize,
    /// The places that hold a word or phrase, as [`check_file`] writes them.
    hits: Vec<String>,
}

/// Reads every file git tracks or would track under `root`, and its path, for `words`,
/// `private` names and `phrases`.
fn find(root: &Path, words: &Words, private: &Words, phrases: &[&str]) -> Result<Found> {
    let files = names::tracked_files(root)?;
    let mut reading = Reading::new(words, private);
    let mut hits = Vec::new();
    for file in &files {
        let text =
            names::tracked_text(&root.join(file)).with_context(|| format!("reading {file}"))?;
        hits.extend(check_file(file, &text, &mut reading, phrases));
    }
    Ok(Found {
        files: files.len(),
        hits,
    })
}

/// Whether `file` is exempt from [`BANNED_PHRASES`] and [`RETIRED_TOKENS`] (vendored code).
fn is_phrase_exempt(file: &str) -> bool {
    PHRASE_EXEMPT_PREFIXES
        .iter()
        .any(|prefix| file.starts_with(prefix))
}

/// Whether `line` contains any phrase from `phrases`, matched case-insensitively.
///
/// Precondition: every entry in `phrases` is already lowercase.
fn finds_phrase(line: &str, phrases: &[&str]) -> bool {
    if phrases.is_empty() {
        return false;
    }
    let lower = line.to_lowercase();
    phrases.iter().any(|phrase| lower.contains(phrase))
}

/// The marks a line of a comment or a quotation begins with, the longest first where
/// one begins another: Rust's doc and plain comments, a block comment's middle, the
/// comments of shells, YAML, TOML and Markdown's headings, SQL's, Markdown's quotations,
/// and the comments of Lisp, INI files and TeX.
const COMMENT_MARKS: [&str; 9] = ["//!", "///", "//", "*", "#", "--", ">", ";", "%"];

/// How many lines a phrase is looked for across: one that line breaks cut into this
/// many pieces, or fewer, is found. Never across a line that says nothing.
const LINES_JOINED: usize = 3;

/// What a line says: without the marks it begins with, as many as it has (`> * a`),
/// without the `\\` that carries it on to the next, and with every run of blanks one space.
fn said(line: &str) -> String {
    let mut rest = line.trim_start();
    while let Some(after) = COMMENT_MARKS
        .iter()
        .find_map(|mark| rest.strip_prefix(mark))
    {
        rest = after.trim_start();
    }
    let rest = rest.trim_end();
    let rest = rest.strip_suffix('\\').unwrap_or(rest);
    rest.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lines as one sentence, as a phrase cut by line breaks reads: what each says ([`said`])
/// with one space between. `None` if one of them says nothing, which ends a sentence.
fn joined(saids: &[String]) -> Option<String> {
    if saids.iter().any(String::is_empty) {
        return None;
    }
    Some(saids.join(" "))
}

/// Whether `line` holds a token of `tokens` in any case with no lowercase letter right
/// after it. ASCII lowercase keeps every byte where it was, so the letter after a match
/// is read from `line` itself.
fn finds_token(line: &str, tokens: &[&str]) -> bool {
    let lower = line.to_ascii_lowercase();
    let bytes = line.as_bytes();
    tokens.iter().any(|token| {
        let token = token.to_ascii_lowercase();
        lower.match_indices(&token).any(|(start, _)| {
            bytes
                .get(start + token.len())
                .is_none_or(|next| !next.is_ascii_lowercase())
        })
    })
}

/// The hits of one file: `path: in its name`, then `path:line` for each line. A phrase
/// that line breaks cut apart is a hit of the line it begins on.
fn check_file(file: &str, text: &str, reading: &mut Reading<'_>, phrases: &[&str]) -> Vec<String> {
    let mut hits = Vec::new();
    let (active_phrases, active_tokens) = if is_phrase_exempt(file) {
        (&[][..], &[][..])
    } else {
        (phrases, &RETIRED_TOKENS[..])
    };
    if reading.finds_in(file)
        || reading.finds_name_in(file)
        || finds_token(file, active_tokens)
        || finds_phrase(file, active_phrases)
    {
        hits.push(format!("{file}: in its name"));
    }
    let lines: Vec<&str> = text.lines().collect();
    let saids: Vec<String> = if active_phrases.is_empty() {
        Vec::new()
    } else {
        lines.iter().map(|line| said(line)).collect()
    };
    // What the `count` lines from `start` say together, in lowercase.
    let sentence = |start: usize, count: usize| {
        saids
            .get(start..start + count)
            .and_then(joined)
            .map(|sentence| sentence.to_lowercase())
            .unwrap_or_default()
    };
    for (index, line) in lines.iter().enumerate() {
        // A phrase that the lines from here say, and the same number of lines but this
        // one do not, begins here.
        let begins_here = !saids.is_empty()
            && (1..=LINES_JOINED).any(|count| {
                let (here, from_next) = (sentence(index, count), sentence(index + 1, count - 1));
                active_phrases
                    .iter()
                    .any(|phrase| here.contains(phrase) && !from_next.contains(phrase))
            });
        if reading.finds_in(line)
            || reading.finds_name_in(line)
            || finds_phrase(line, active_phrases)
            || begins_here
            || finds_token(line, active_tokens)
        {
            hits.push(format!("{file}:{}", index + 1));
        }
    }
    hits
}

/// The 32 bytes that 64 hexadecimal digits write.
fn digest_of_hexadecimal(hexadecimal: &str) -> Result<Digest> {
    let mut digest = [0_u8; 32];
    // Every digit is one byte, so the pairs below are cut between letters.
    if hexadecimal.len() != 64 || !hexadecimal.is_ascii() {
        bail!("a digest is 64 hexadecimal digits: {hexadecimal}");
    }
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hexadecimal[index * 2..index * 2 + 2], 16)
            .with_context(|| format!("a digest is 64 hexadecimal digits: {hexadecimal}"))?;
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::process::Command;

    use super::*;
    use crate::files::testing;

    /// Runs git in `root` and fails the test if git does.
    fn git(root: &Path, arguments: &[&str]) {
        let status = Command::new("git")
            .args(arguments)
            .current_dir(root)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {arguments:?} succeeds");
    }

    /// The list that bans the given words, written as [`BANNED`] is.
    fn banning(banned: &[&str]) -> Words {
        listing(banned.iter().map(|word| (*word).to_owned()))
    }

    /// The list of the given made-up private names, folded and written as
    /// [`PRIVATE_NAMES`] is.
    fn naming(private: &[&str]) -> Words {
        listing(private.iter().map(|name| folded_name(name)))
    }

    /// A list of `entries` written as [`BANNED`] is: each one's characters, and digest.
    fn listing(entries: impl Iterator<Item = String>) -> Words {
        let listed: Vec<(usize, String)> = entries
            .map(|entry| {
                let mut hexadecimal = String::new();
                for byte in Sha256::digest(entry.as_bytes()) {
                    write!(hexadecimal, "{byte:02x}").expect("a string takes what is written");
                }
                (entry.chars().count(), hexadecimal)
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
        let nothing = naming(&[]);
        let mut reading = Reading::new(&words, &nothing);
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
        let nothing = naming(&[]);
        let mut reading = Reading::new(&words, &nothing);
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
        let nothing = naming(&[]);
        let mut reading = Reading::new(&words, &nothing);
        assert_eq!(
            check_file(
                "docs/GADGET-notes.md",
                "our notes\nonce a Gadget\n\nand gadgets\nand gadgets again\n",
                &mut reading,
                &[]
            ),
            [
                "docs/GADGET-notes.md: in its name",
                "docs/GADGET-notes.md:2",
                "docs/GADGET-notes.md:4",
                "docs/GADGET-notes.md:5"
            ]
        );
        assert_eq!(
            check_file("docs/notes.md", "our notes\n", &mut reading, &[]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn every_file_git_lists_is_read_whatever_its_name_and_a_gone_one_by_its_name() {
        let words = banning(&["gadget", "가나다라"]);
        let root = testing::scratch("banned-find");
        testing::write(&root, "plain.md", "our notes\n");
        testing::write(&root, "notes/held.md", "our notes\nonce a Gadget\n");
        testing::write(&root, "기록/옛가나다라.md", "our notes\n");
        testing::write(&root, "기록/글.md", "첫 줄\n\n가나다라의 글\n");
        testing::write(&root, "gone-gadget.md", "our notes\n");
        std::os::unix::fs::symlink("notes/gadgets", root.join("link")).expect("the link is made");
        git(&root, &["init", "--quiet"]);
        git(&root, &["add", "gone-gadget.md"]);
        std::fs::remove_file(root.join("gone-gadget.md")).expect("the file is removed");

        let found = find(&root, &words, &naming(&[]), &[]).expect("the files are read");

        assert_eq!(found.files, 6);
        assert_eq!(
            found.hits,
            [
                "gone-gadget.md: in its name",
                "link:1",
                "notes/held.md:2",
                "기록/글.md:3",
                "기록/옛가나다라.md: in its name",
            ]
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

    #[test]
    fn the_private_names_of_this_workspace_are_read() {
        let names = Words::parse(&PRIVATE_NAMES).expect("the list is read");
        assert_eq!(
            names.by_letters.values().map(Vec::len).sum::<usize>(),
            PRIVATE_NAMES.len()
        );
    }

    #[test]
    fn a_private_name_is_found_however_its_parts_are_joined() {
        let words = banning(&[]);
        let names = naming(&[concat!("secret-", "repo"), concat!("/hidden", "-desk/")]);
        let mut reading = Reading::new(&words, &names);
        for text in [
            concat!("see secret-", "repo for that"),
            concat!("SECRET_", "REPO"),
            concat!("Secret", "Repo"),
            concat!("git clone https://example.com/org/secret-", "repo.git"),
            concat!("~/hidden-", "desk/secret-", "repo-carry/notes"),
            concat!("cd $HOME/hidden", "desk/other"),
            concat!("/home/someone/hidden_", "desk/a"),
        ] {
            assert!(reading.finds_name_in(text), "{text:?} holds a private name");
        }
        for text in [
            "a secret, the repo",
            "secret repo",
            "secrets",
            "repository",
            concat!("hidden", "desk"),
            concat!("/hidden", "desks/"),
        ] {
            assert!(
                !reading.finds_name_in(text),
                "{text:?} holds no private name"
            );
        }
        assert_eq!(
            check_file(
                concat!("docs/secret-", "repo/notes.md"),
                concat!("one\nnotes of Secret", "Repo\n"),
                &mut reading,
                &[]
            ),
            [
                concat!("docs/secret-", "repo/notes.md: in its name"),
                concat!("docs/secret-", "repo/notes.md:2"),
            ]
        );
        assert_eq!(
            check_file(
                "vendor/rhwp-studio/upstream/a.md",
                concat!("secret-", "repo\n"),
                &mut reading,
                &BANNED_PHRASES
            ),
            ["vendor/rhwp-studio/upstream/a.md:1"],
            "a private name is refused in vendored sources too"
        );
    }

    #[test]
    fn banned_phrases_match_case_insensitively_without_false_positives() {
        for phrase in BANNED_PHRASES {
            assert_eq!(
                phrase,
                phrase.to_lowercase(),
                "banned phrase {phrase:?} must be lowercase"
            );
        }

        for text in [
            concat!("Ports ", "server/api/files.rs"),
            concat!("PORTS ", "SERVER/app/boards.rs"),
            concat!("matches the ", "original's response"),
            concat!("verified against The ", "Original Server"),
            concat!("The formats of ", "the original archive"),
            concat!("기존 원본", "의 동작"),
            concat!("원본", "과 같다"),
            concat!("원본", "과의 비교"),
            concat!("원본", "에서 옮김"),
            concat!("ordered As the ", "Original does"),
            concat!("as the ", "original did"),
            concat!("what the ", "original app showed"),
            concat!("as in the ", "original tree"),
            concat!("the cases of the ", "original `a.test.ts`"),
            // Refused although a copy could say it of its source: it says `the source`.
            concat!("with the same members as the ", "original"),
            concat!("a copy of the ", "original `Block`"),
        ] {
            assert!(
                finds_phrase(text, &BANNED_PHRASES),
                "{text:?} must match a banned phrase"
            );
        }

        for text in [
            "original file",
            "the original file name",
            "original card",
            "the original board exists",
            "the original card stays where it was",
            "copies of the original file",
            "originally",
            "원본 이미지",
            "원본 파일",
            "원본 데이터",
            "the same members as the source",
            "a copy of the source `Block`",
        ] {
            assert!(
                !finds_phrase(text, &BANNED_PHRASES),
                "{text:?} must not match any banned phrase"
            );
        }
    }

    #[test]
    fn a_phrase_cut_by_a_line_break_is_found_on_the_line_it_begins_on() {
        let words = banning(&[]);
        let nothing = naming(&[]);
        let mut reading = Reading::new(&words, &nothing);
        for (file, text) in [
            (
                "crates/a.rs",
                concat!(
                    "// the name of the\n// ",
                    "original server's configuration\n"
                ),
            ),
            (
                "crates/a.rs",
                concat!("    /// kept as\n    /// the ", "original kept it\n"),
            ),
            (
                "crates/a.rs",
                concat!("//! read as the\n//! ", "original does\n"),
            ),
            (
                "apps/web/a.ts",
                concat!(" * the cases of the\n * ", "original `x.test.ts`\n */\n"),
            ),
            (
                "a.yaml",
                concat!("# laid out as in the\n# ", "original tree\n"),
            ),
            ("a.sql", concat!("-- the formats of\n-- ", "the original\n")),
            ("a.md", concat!("what the\n", "original app showed\n")),
            // Quotations, stacked marks and the comments of other languages.
            ("a.md", concat!("> kept as the\n> ", "original did\n")),
            ("a.md", concat!("> * kept as the\n> * ", "original did\n")),
            (
                "a.rs",
                concat!("// * kept as\n// * the ", "original kept it\n"),
            ),
            ("a.el", concat!(";; kept as the\n;; ", "original did\n")),
            ("a.tex", concat!("% kept as the\n% ", "original did\n")),
            // Blanks of any kind and number, and a line carried on to the next.
            ("a.rs", concat!("// kept as  the\t", "original did\n")),
            (
                "a.sh",
                concat!("echo \"kept as the \\\n  ", "original did\"\n"),
            ),
            // Three lines.
            (
                "a.rs",
                concat!("// kept as\n// the\n// ", "original kept it\n"),
            ),
        ] {
            let found = check_file(file, text, &mut reading, &BANNED_PHRASES);
            assert_eq!(found, [format!("{file}:1")], "{text:?}");
        }
        // A phrase within one line is its line's, once; the line before it is clean.
        assert_eq!(
            check_file(
                "crates/a.rs",
                concat!("// first\n// as the ", "original did\n// last\n"),
                &mut reading,
                &BANNED_PHRASES
            ),
            ["crates/a.rs:2"]
        );
        // What two lines say together is not banned unless a phrase is.
        assert_eq!(
            check_file(
                "crates/a.rs",
                "// the copy keeps\n// the original board exists\n// as it was\n",
                &mut reading,
                &BANNED_PHRASES
            ),
            Vec::<String>::new()
        );
        // Two phrases that overlap begin on their own lines.
        assert_eq!(
            check_file(
                "crates/a.rs",
                concat!("// kept as\n// the\n// ", "original did\n"),
                &mut reading,
                &BANNED_PHRASES
            ),
            ["crates/a.rs:1", "crates/a.rs:2"]
        );
        // Nor across a line that says nothing, which ends what was said.
        assert_eq!(
            check_file(
                "crates/a.rs",
                concat!("// kept as\n//\n// the ", "original kept it\n"),
                &mut reading,
                &BANNED_PHRASES
            ),
            Vec::<String>::new()
        );
        // Four lines are more than are joined.
        assert_eq!(
            check_file(
                "crates/a.rs",
                concat!("// formats\n// of\n// the\n// ", "original\n"),
                &mut reading,
                &BANNED_PHRASES
            ),
            Vec::<String>::new()
        );
        assert_eq!(said("   /// text  "), "text");
        assert_eq!(said(" * text"), "text");
        assert_eq!(said("//! text"), "text");
        assert_eq!(said("-- text"), "text");
        assert_eq!(said("> > * text"), "text");
        assert_eq!(said("// a \t  b \\"), "a b");
        assert_eq!(said("plain text"), "plain text");
        assert_eq!(
            joined(&[said("// one"), said("  # two ")]).as_deref(),
            Some("one two")
        );
        assert_eq!(joined(&[said("// one"), said("//")]), None);
    }

    #[test]
    fn a_phrase_in_a_path_is_found() {
        let words = banning(&[]);
        let nothing = naming(&[]);
        let mut reading = Reading::new(&words, &nothing);
        assert_eq!(
            check_file(
                concat!("crates/http.serve", "content.rs"),
                "clean\n",
                &mut reading,
                &BANNED_PHRASES
            ),
            [concat!("crates/http.serve", "content.rs: in its name")]
        );
    }

    #[test]
    fn vendored_sources_skip_phrases_but_still_enforce_banned_words() {
        let words = banning(&["gadget"]);
        let nothing = naming(&[]);
        let mut reading = Reading::new(&words, &nothing);
        let phrase_line = concat!("-- the ", "original's board block\n");
        let word_line = "-- uses gadget here\n";

        assert_eq!(
            check_file(
                "crates/store/schema/sqlite/0001_baseline.sql",
                phrase_line,
                &mut reading,
                &BANNED_PHRASES
            ),
            ["crates/store/schema/sqlite/0001_baseline.sql:1"]
        );
        assert_eq!(
            check_file(
                "vendor/rhwp-studio/upstream/src/example.ts",
                concat!("// 원본", "과 비교\n"),
                &mut reading,
                &BANNED_PHRASES
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            check_file(
                "vendor/rhwp-studio/import.mjs",
                concat!("// 원본", "과 비교\n"),
                &mut reading,
                &BANNED_PHRASES
            ),
            ["vendor/rhwp-studio/import.mjs:1"],
            "what is written here beside the vendored sources is held to the phrases"
        );
        assert_eq!(
            check_file(
                "vendor/rhwp-studio/upstream/src/example.ts",
                word_line,
                &mut reading,
                &BANNED_PHRASES
            ),
            ["vendor/rhwp-studio/upstream/src/example.ts:1"]
        );
        assert_eq!(
            check_file("docs/notes.md", phrase_line, &mut reading, &BANNED_PHRASES),
            ["docs/notes.md:1"]
        );
    }

    #[test]
    fn retired_tokens_are_found_in_any_case_inside_names_and_in_paths() {
        let words = banning(&[]);
        let nothing = naming(&[]);
        let mut reading = Reading::new(&words, &nothing);
        for hit_line in [
            concat!("pub ", "create", "_at: i64,"),
            concat!("SELECT b.", "update", "_at FROM blocks b"),
            concat!(r#"{"id":"1",""#, "create", "At", r#"":10}"#),
            concat!("card.", "update", "At"),
            concat!("CREATE INDEX blocks_parent_type_", "create", "_at_idx"),
            concat!("const old", "Create", "At = 1;"),
            concat!("ALTER TABLE b DROP COLUMN ", "CREATE", "_AT"),
            concat!("let Preferences = ", "UserPreferences", "Patch::default();"),
        ] {
            assert!(
                finds_token(hit_line, &RETIRED_TOKENS),
                "{hit_line:?} must match a retired token"
            );
            for file in [
                "crates/model/src/lib.rs",
                "crates/store/schema/sqlite/0001_baseline.sql",
            ] {
                assert_eq!(
                    check_file(file, hit_line, &mut reading, &[]),
                    [format!("{file}:1")]
                );
            }
            assert_eq!(
                check_file(
                    "vendor/rhwp-studio/upstream/lib/a.ts",
                    hit_line,
                    &mut reading,
                    &[]
                ),
                Vec::<String>::new(),
                "vendored sources are not ours to rename"
            );
            assert_eq!(
                check_file("vendor/rhwp-studio/import.mjs", hit_line, &mut reading, &[]),
                ["vendor/rhwp-studio/import.mjs:1"]
            );
        }
        let named = concat!("crates/store/src/", "create", "_at.rs");
        assert_eq!(
            check_file(named, "", &mut reading, &[]),
            [format!("{named}: in its name")]
        );

        for clean_line in [
            "pub created_at: i64,",
            "pub updated_at: i64,",
            "CREATED_AT and UPDATED_AT",
            r#"{"createdAt":10,"updatedAt":20}"#,
            "backend.createAttachment(boardId, cardId, file)",
            "editor.commands.updateAttributes('codeBlock', {})",
            "const NO_CREATED_AT: &str = \"missing\";",
            "blocks_parent_type_created_at_idx",
        ] {
            assert!(
                !finds_token(clean_line, &RETIRED_TOKENS),
                "{clean_line:?} must not match any retired token"
            );
        }
    }
}
