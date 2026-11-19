//! The scripts that keep an installation, run as an operator runs them: `backup.sh`
//! (with its files in the volume, and again as if they were in S3), the volumes taken
//! away, `restore.sh` into the new ones, and `update.sh`, which refuses an `.env` that
//! lacks a line before it stops anything, then updates and says how long fvoci was
//! down.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};
use serde_json::json;

use super::Trial;
use super::scenario::{self, Client};
use crate::steps::say;

/// What the tables hold, table by table, as PostgreSQL's superuser counts; but the
/// queue of changes for the search server, which the restore's new index fills and the
/// server empties in the seconds after.
const COUNT_ROWS: &str = "SELECT table_name || '=' || (xpath('/row/c/text()', \
     query_to_xml(format('SELECT count(*) AS c FROM public.%I', table_name), false, true, '')))[1]::text \
     FROM information_schema.tables WHERE table_schema = 'public' AND table_type = 'BASE TABLE' \
     AND table_name <> 'search_outbox' ORDER BY table_name";

/// Every file of the files' volume with its SHA-256, as fvoci's image lists them; the
/// readiness check's file and files being written are not the installation's.
const LIST_FILES: &str = "cd /data/files && find . -type f -not -path './.tmp/*' \
     -not -name .fvoci-ready -exec sha256sum {} + | sort -k 2";

/// A secret the trial's `.env` sets, whose line `update.sh` must miss.
const LACKING: &str = "FVOCI_SES_SECRET_ACCESS_KEY";

/// Where the trial's backups go, in `main` and `s3`: removed with the trial's other
/// files.
pub(super) fn folder(directory: &Path) -> PathBuf {
    directory.join("trial-backups")
}

/// Backs up, takes the volumes away, restores, and updates.
///
/// # Errors
///
/// Fails at the first script or comparison that does not go as it should.
pub(super) fn round_trip(trial: &Trial, directory: &Path) -> Result<()> {
    let backups = folder(directory).join("main");
    let kept = Kept::write(trial, directory)?;
    let rows = counted_rows(trial)?;
    let files = listed_files(trial)?;
    ensure!(
        files.contains(&format!("/{}/", kept.board)),
        "the upload is not in the volume: {files}"
    );

    let made = run_script(trial, "backup.sh", &["--dir", &backups.to_string_lossy()])?;
    let backup = only_backup(&backups)?;
    check_private(&backups, &backup, trial)?;
    say(&format!("    {}", last_said(&made, "backup")));
    backs_up_no_files_from_s3(trial, directory)?;
    refuses_a_full_installation(trial, &backup, &rows)?;

    super::mail::remove_fake(trial)?;
    let down = trial.compose(&["down", "--volumes", "--timeout", super::STOP_SECONDS])?;
    ensure!(down.succeeded, "docker compose down failed: {}", down.error);
    let restored = run_script(trial, "restore.sh", &[&backup.to_string_lossy()])?;
    ensure!(
        restored.contains("made anew from") && !restored.contains("made anew from 0 "),
        "restore.sh did not make the search index anew: {restored}"
    );
    ensure!(
        restored.contains("every file the records name is in the volume"),
        "restore.sh did not find every file: {restored}"
    );
    let rows_now = counted_rows(trial)?;
    ensure!(
        rows_now == rows,
        "the rows differ after the restore:\nbefore {rows}\nafter  {rows_now}"
    );
    let files_now = listed_files(trial)?;
    ensure!(
        files_now == files,
        "the files differ after the restore:\nbefore {files}\nafter  {files_now}"
    );
    kept.check(trial, directory)?;
    say(&format!(
        "    {}: rows, files, login, the file's bytes and the search are as before",
        last_said(&restored, "restore")
    ));

    refuses_a_lacking_env(trial, directory)?;
    updates(trial, &backups)
}

/// `restore.sh` refuses an installation with a container of fvoci, then, the container
/// removed, a database that holds tables; and changes nothing.
fn refuses_a_full_installation(trial: &Trial, backup: &Path, rows: &str) -> Result<()> {
    let backup = backup.to_string_lossy();
    let refusals = [
        ("fvoci has a container", None),
        (
            "the database holds",
            Some(["rm", "--stop", "--force", "fvoci"]),
        ),
    ];
    for (refusal, first) in refusals {
        if let Some(first) = first {
            let removed = trial.compose(&first)?;
            ensure!(
                removed.succeeded,
                "removing fvoci failed: {}",
                removed.error
            );
        }
        let (succeeded, said) =
            script_output(trial.script("restore.sh", &[&backup]), "restore.sh")?;
        no_secret_in(trial, "restore.sh", &said)?;
        ensure!(
            !succeeded && said.contains(refusal) && said.contains("Nothing was changed"),
            "restore.sh did not refuse, saying \"{refusal}\": {said}"
        );
        let rows_now = counted_rows(trial)?;
        ensure!(rows_now == rows, "restore.sh refused but the rows changed");
    }
    say("    restore.sh refuses fvoci's container and a database that holds tables");
    Ok(())
}

/// `update.sh` that expects another commit fails after fvoci is ready, saying it
/// serves; then one that expects the image's says how long fvoci was down.
fn updates(trial: &Trial, backups: &Path) -> Result<()> {
    let backups = backups.to_string_lossy();
    let (succeeded, said) = script_output(
        trial.script(
            "update.sh",
            &["--no-pull", "--commit", "0000000", "--dir", &backups],
        ),
        "update.sh",
    )?;
    no_secret_in(trial, "update.sh", &said)?;
    ensure!(
        !succeeded && said.contains("is ready and serving, but runs the commit"),
        "update.sh expecting another commit did not fail saying so: {said}"
    );
    let updated = run_script(trial, "update.sh", &["--no-pull", "--dir", &backups])?;
    let commit = format!("fvoci runs the commit {}", trial.images.revision());
    ensure!(
        updated.contains("fvoci was down") && updated.contains(&commit),
        "update.sh did not say how long fvoci was down and which commit it runs: {updated}"
    );
    say("    update.sh expecting another commit fails, saying fvoci serves");
    for line in updated.lines().filter(|line| line.starts_with("update: ")) {
        say(&format!("    {line}"));
    }
    Ok(())
}

/// The tag `update.sh` gives the image that ran before it.
pub(super) fn kept_tag(trial: &Trial) -> String {
    format!("fvoci-server:before-update-{}", trial.project)
}

/// What a person keeps in the installation before the backup: an account, a card whose
/// body the search finds, and a file.
struct Kept {
    password: String,
    workspace: String,
    /// The board, whose folder of the storage holds the file.
    board: String,
    card: String,
    /// The path the file is read back from.
    file_path: String,
    contents: String,
}

impl Kept {
    fn write(trial: &Trial, directory: &Path) -> Result<Self> {
        let client = Client::new(&trial.base, directory)?;
        let password = super::hex(&super::random::<16>()?);
        client.expect(
            "POST",
            "/api/v1/register",
            Some(&json!({
                "username": "keeper",
                "email": "keeper@example.com",
                "password": password,
                "organizationName": "Kept",
            })),
        )?;
        client.expect(
            "POST",
            "/api/v1/login",
            Some(&json!({"username": "keeper", "password": password})),
        )?;
        let workspaces = client.expect("GET", "/api/v1/users/me/workspaces", None)?;
        let workspace = scenario::text(
            workspaces
                .get(0)
                .context("the new account has no workspace")?,
            "id",
        )?;
        let w = format!("/api/v1/w/{workspace}");
        let board = client.expect(
            "POST",
            &format!("{w}/boards"),
            Some(&json!({"type": "O", "title": "Kept"})),
        )?;
        let board = scenario::text(&board, "id")?;
        let card = scenario::write_card(&client, &w, &board)?;
        // Text, so that it is read back as it was written.
        let contents = super::hex(&super::random::<1024>()?);
        let upload = directory.join("kept.txt");
        super::write_private(&upload, &contents)?;
        let answer = client.upload(&format!("{w}/containers/{board}/files"), &upload);
        let _ = std::fs::remove_file(&upload);
        let answer = answer?;
        ensure!(
            answer.status == 201,
            "the upload answered {}: {}",
            answer.status,
            answer.body
        );
        let uploaded: serde_json::Value =
            serde_json::from_str(&answer.body).context("the upload's answer")?;
        let file = scenario::text(&uploaded, "fileId")?;
        Ok(Self {
            password,
            file_path: format!("{w}/containers/{board}/files/{file}"),
            workspace: w,
            board,
            card,
            contents,
        })
    }

    /// Signs in anew and finds the card and the file as they were.
    fn check(&self, trial: &Trial, directory: &Path) -> Result<()> {
        let client = Client::new(&trial.base, directory)?;
        client.expect(
            "POST",
            "/api/v1/login",
            Some(&json!({"username": "keeper", "password": self.password})),
        )?;
        let file = client.send("GET", &self.file_path, None)?;
        ensure!(
            file.status == 200 && file.body == self.contents,
            "the file is not read back as it was written ({})",
            file.status
        );
        scenario::find_card(&client, &self.workspace, &self.card)
    }
}

/// Runs a script of `docker/` on the trial with `arguments`; it must succeed, and say
/// no secret of the trial. Returns what it wrote.
fn run_script(trial: &Trial, name: &str, arguments: &[&str]) -> Result<String> {
    let (succeeded, said) = script_output(trial.script(name, arguments), name)?;
    no_secret_in(trial, name, &said)?;
    ensure!(succeeded, "{name} failed:\n{said}");
    Ok(said)
}

/// What a script wrote, both streams, and whether it succeeded.
fn script_output(mut command: Command, name: &str) -> Result<(bool, String)> {
    let output = command
        .output()
        .with_context(|| format!("starting {name}"))?;
    let said = String::from_utf8_lossy(&output.stdout) + String::from_utf8_lossy(&output.stderr);
    Ok((output.status.success(), said.into_owned()))
}

fn no_secret_in(trial: &Trial, what: &str, text: &str) -> Result<()> {
    if trial
        .secrets
        .iter()
        .any(|secret| text.contains(secret.as_str()))
    {
        bail!("{what} wrote one of the trial's secrets");
    }
    Ok(())
}

/// The last line a script wrote itself (`backup: ...`), not compose's.
fn last_said<'text>(text: &'text str, script: &str) -> &'text str {
    text.lines()
        .rev()
        .find(|line| {
            line.strip_prefix(script)
                .is_some_and(|rest| rest.starts_with(": "))
        })
        .unwrap_or_default()
}

/// The one backup in `backups`.
fn only_backup(backups: &Path) -> Result<PathBuf> {
    let made: Vec<PathBuf> = std::fs::read_dir(backups)
        .with_context(|| format!("reading {}", backups.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    match &made[..] {
        [backup] => Ok(backup.clone()),
        _ => bail!("backup.sh made {made:?}, not one folder"),
    }
}

/// The backups' folder and each backup only its owner may read, and no file is named
/// after a secret.
fn check_private(backups: &Path, backup: &Path, trial: &Trial) -> Result<()> {
    let mode = |path: &Path| -> Result<u32> {
        Ok(std::fs::metadata(path)
            .with_context(|| format!("reading {}", path.display()))?
            .permissions()
            .mode()
            & 0o777)
    };
    for folder in [backups, backup] {
        ensure!(mode(folder)? == 0o700, "{} is not 700", folder.display());
    }
    let mut names = Vec::new();
    let entries =
        std::fs::read_dir(backup).with_context(|| format!("reading {}", backup.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("reading {}", backup.display()))?
            .path();
        ensure!(mode(&path)? == 0o600, "{} is not 600", path.display());
        names.push(path.to_string_lossy().into_owned());
    }
    ensure!(
        names.iter().any(|name| name.ends_with("/db.dump"))
            && names.iter().any(|name| name.ends_with("/files.tar")),
        "the backup lacks the database or the files: {names:?}"
    );
    no_secret_in(trial, "a backup's name", &names.join(" "))
}

/// `backup.sh` with an `.env` that keeps the files in S3 dumps the database alone, and
/// says the bucket is the operator's.
fn backs_up_no_files_from_s3(trial: &Trial, directory: &Path) -> Result<()> {
    let env = directory.join("s3.env");
    let text = std::fs::read_to_string(&trial.env_file).context("reading the trial's .env")?;
    super::write_private(&env, &format!("{text}\nFVOCI_FILE_STORAGE=s3\n"))?;
    let backups = folder(directory).join("s3");
    let made = run_script(
        trial,
        "backup.sh",
        &[
            "--env-file",
            &env.to_string_lossy(),
            "--dir",
            &backups.to_string_lossy(),
        ],
    );
    let _ = std::fs::remove_file(&env);
    let made = made?;
    let backup = only_backup(&backups)?;
    let files = backup.join("files.tar").exists();
    let dump = backup.join("db.dump").exists();
    std::fs::remove_dir_all(&backups).context("removing the backup of S3's settings")?;
    ensure!(
        dump && !files && made.contains("they are in S3, whose bucket is yours to back up"),
        "with S3, backup.sh did not dump the database alone, saying so: {made}"
    );
    say("    with the files in S3, the database alone, and the bucket is the operator's");
    Ok(())
}

/// `update.sh` with an `.env` that lacks a secret's line names it and stops before
/// anything changes: fvoci is the same container, started when it was.
fn refuses_a_lacking_env(trial: &Trial, directory: &Path) -> Result<()> {
    let before = fvoci_started(trial)?;
    let env = directory.join("lacking.env");
    let text = std::fs::read_to_string(&trial.env_file).context("reading the trial's .env")?;
    let line = format!("{LACKING}=");
    let lines: Vec<&str> = text
        .lines()
        .filter(|kept| !kept.starts_with(&line))
        .collect();
    super::write_private(&env, &lines.join("\n"))?;
    let refused = script_output(
        trial.script(
            "update.sh",
            &[
                "--no-pull",
                "--env-file",
                &env.to_string_lossy(),
                "--dir",
                &folder(directory).join("main").to_string_lossy(),
            ],
        ),
        "update.sh",
    );
    let _ = std::fs::remove_file(&env);
    let (succeeded, said) = refused?;
    no_secret_in(trial, "update.sh", &said)?;
    ensure!(
        !succeeded
            && said.contains(&format!("lacks {LACKING}"))
            && said.contains("Nothing was changed"),
        "update.sh with an .env lacking {LACKING} did not refuse, naming it: {said}"
    );
    ensure!(
        fvoci_started(trial)? == before,
        "update.sh refused but fvoci was made or started anew"
    );
    say(&format!(
        "    update.sh names the lacking {LACKING} and leaves fvoci running"
    ));
    Ok(())
}

/// fvoci's container and when it started.
fn fvoci_started(trial: &Trial) -> Result<String> {
    let container = trial.compose(&["ps", "--quiet", "fvoci"])?;
    ensure!(
        container.succeeded && !container.output.is_empty(),
        "fvoci has no container: {}",
        container.error
    );
    let started = crate::steps::capture(
        "docker",
        &[
            "inspect",
            "--format",
            "{{.Id}} {{.State.StartedAt}}",
            &container.output,
        ],
    )?;
    ensure!(
        started.succeeded,
        "docker inspect failed: {}",
        started.error
    );
    Ok(started.output)
}

/// The rows of every table, by table.
fn counted_rows(trial: &Trial) -> Result<String> {
    let answer = trial.compose(&[
        "exec",
        "-T",
        "postgres",
        "psql",
        "--username=postgres",
        "--dbname=fvoci",
        "--no-align",
        "--tuples-only",
        "--command",
        COUNT_ROWS,
    ])?;
    ensure!(answer.succeeded, "psql failed: {}", answer.error);
    let counts: BTreeMap<&str, &str> = answer
        .output
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    ensure!(
        counts.contains_key("users"),
        "no table users: {}",
        answer.output
    );
    Ok(counts
        .iter()
        .map(|(table, count)| format!("{table}={count}"))
        .collect::<Vec<_>>()
        .join(" "))
}

/// The files of the volume with their SHA-256, one a line.
fn listed_files(trial: &Trial) -> Result<String> {
    let answer = trial.compose(&["exec", "-T", "fvoci", "sh", "-c", LIST_FILES])?;
    ensure!(
        answer.succeeded,
        "listing the files failed: {}",
        answer.error
    );
    Ok(answer.output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_line_said_is_the_scripts_not_composes() {
        let said = "restore: starting\nrestore: restored x in 2.0 s\n Container a Healthy\n";
        assert_eq!(last_said(said, "restore"), "restore: restored x in 2.0 s");
        assert_eq!(last_said(said, "backup"), "");
        assert_eq!(last_said("restored: no", "restore"), "");
    }
}
