//! Lines of an `.env` that compose refuses or warns of, quoting what was pasted (a key's
//! lines, a quote never closed, a `$` read as a variable): `backup.sh`, `update.sh` and
//! `restore.sh` stop before anything changes, and say what compose said without a word
//! of what was pasted. The table is compose's messages as the trial's compose writes
//! them.

use std::path::Path;

use anyhow::{Context as _, Result, ensure};

use super::Trial;
use super::backup::{fvoci_started, no_secret_in, script_output};
use crate::steps::say;

/// One line pasted wrong: what it is, the line with `{0}`, `{1}`, `{2}` standing for
/// words of a secret, and what the scripts show of compose's complaint.
struct Pasted {
    what: &'static str,
    line: &'static str,
    shown: &'static str,
}

/// The lines, each after the trial's `.env`. compose quotes each one's words but the
/// space's (it names the line), and the port's and the address's (lines it does not
/// know are counted, not shown).
const TABLE: [Pasted; 15] = [
    Pasted {
        what: "a key's line",
        line: "MIIEvQ+{0}=",
        shown: ": unexpected character <hidden> in variable name <hidden>",
    },
    Pasted {
        what: "a key's line in quotes",
        line: "\"{0}\"=x",
        shown: ": unexpected character <hidden> in variable name <hidden>",
    },
    Pasted {
        what: "a second quoted value",
        line: "FVOCI_PASTED=\"x\" \"{0}\"",
        shown: ": unexpected character <hidden> in variable name <hidden>",
    },
    Pasted {
        what: "a space in a key's line",
        line: "{0} {1}=x",
        shown: ": key cannot contain a space",
    },
    Pasted {
        what: "a double quote never closed",
        line: "FVOCI_PASTED=\"{0}",
        shown: ": unterminated quoted value <hidden>",
    },
    Pasted {
        what: "a double quote never closed, a quote escaped inside",
        line: "FVOCI_PASTED=\"{0}\\\"{1}",
        shown: ": unterminated quoted value <hidden>",
    },
    Pasted {
        what: "a single quote never closed, a quote escaped inside",
        line: "FVOCI_PASTED='{0}\\'{1}",
        shown: ": unterminated quoted value <hidden>",
    },
    Pasted {
        what: "single quotes inside a double quote never closed",
        line: "FVOCI_PASTED=\"{0}'{1}'{2}",
        shown: ": unterminated quoted value <hidden>",
    },
    Pasted {
        what: "double quotes inside a single quote never closed",
        line: "FVOCI_PASTED='{0}\"{1}\"{2}",
        shown: ": unterminated quoted value <hidden>",
    },
    Pasted {
        what: "a ${ never closed",
        line: "FVOCI_PASTED={0}${{1}",
        shown: "compose: Invalid template: <hidden>",
    },
    Pasted {
        what: "a ${:?} whose variable and message are pasted",
        line: "FVOCI_PASTED=${{0}:?{1} {2}}",
        shown: "compose: required variable <hidden> is missing a value",
    },
    Pasted {
        what: "a $ read as a variable",
        line: "FVOCI_PASTED={0}${1}",
        shown: "compose: warning: The <hidden> variable is not set.",
    },
    Pasted {
        what: "a ${} read as a variable in double quotes",
        line: "FVOCI_PASTED=\"{0}${{1}}{2}\"",
        shown: "compose: warning: The <hidden> variable is not set.",
    },
    Pasted {
        what: "a port pasted",
        line: "FVOCI_HOST_PORT={0}",
        shown: "compose: 1 line(s) hidden: they may quote a value of the .env",
    },
    Pasted {
        what: "an address pasted",
        line: "FVOCI_BIND={0}",
        shown: "compose: 1 line(s) hidden: they may quote a value of the .env",
    },
];

/// Gives each script an `.env` with each line of [`TABLE`] in turn.
///
/// # Errors
///
/// Fails at the first script that does not stop, shows a word of what was pasted or
/// shows compose's complaint otherwise than the table says, or if fvoci was made or
/// started anew.
pub(super) fn refused_hidden(trial: &Trial, directory: &Path) -> Result<()> {
    let before = fvoci_started(trial)?;
    let text = std::fs::read_to_string(&trial.env_file).context("reading the trial's .env")?;
    let env = directory.join("pasted-wrong.env");
    let backups = super::backup::folder(directory).join("pasted-wrong");
    let outcome = TABLE
        .iter()
        .try_for_each(|pasted| try_line(trial, &text, &env, &backups, pasted));
    let _ = std::fs::remove_file(&env);
    outcome?;
    ensure!(
        !backups.exists(),
        "a script made {} from an .env it refused",
        backups.display()
    );
    ensure!(
        fvoci_started(trial)? == before,
        "a script refused an .env but fvoci was made or started anew"
    );
    say(&format!(
        "    {} lines pasted wrong, each in backup.sh, update.sh and restore.sh: refused, hidden",
        TABLE.len()
    ));
    Ok(())
}

/// Runs the three scripts with the trial's `.env` and `pasted`'s line after it.
fn try_line(trial: &Trial, text: &str, env: &Path, backups: &Path, pasted: &Pasted) -> Result<()> {
    let words = [word()?, word()?, word()?];
    let line = words
        .iter()
        .enumerate()
        .fold(pasted.line.to_owned(), |line, (at, word)| {
            line.replace(&format!("{{{at}}}"), word)
        });
    super::write_private(env, &format!("{text}\n{line}\n"))?;
    let env_text = env.to_string_lossy();
    let backups_text = backups.to_string_lossy();
    // Never made: restore.sh reads compose's files before it looks at the folder.
    let never_made = backups.join("a-backup");
    let never_made_text = never_made.to_string_lossy();
    let runs: [(&str, &[&str]); 3] = [
        ("backup.sh", &["--dir", &backups_text]),
        ("update.sh", &["--no-pull", "--dir", &backups_text]),
        ("restore.sh", &[&never_made_text]),
    ];
    for (name, own) in runs {
        let arguments = [&["--env-file", env_text.as_ref()][..], own].concat();
        let (succeeded, said) = script_output(trial.script(name, &arguments), name)?;
        no_secret_in(trial, name, &said)?;
        let shown = words.iter().any(|word| said.contains(word.as_str()));
        ensure!(
            !succeeded
                && !shown
                && said.contains(pasted.shown)
                && said.contains("Nothing was changed"),
            "{name} with {} in the .env did not stop showing \"{}\" and no word of it: {}",
            pasted.what,
            pasted.shown,
            words
                .iter()
                .fold(said, |said, word| said.replace(word.as_str(), "<pasted>"))
        );
    }
    Ok(())
}

/// A word of a secret, which a `$` can name: a letter, then random hexadecimal digits.
fn word() -> Result<String> {
    Ok(format!("S{}", super::hex(&super::random::<8>()?)))
}
