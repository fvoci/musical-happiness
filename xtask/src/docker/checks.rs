//! What the installation must not do, looked at while it runs.

use anyhow::{Context as _, Result, bail, ensure};
use serde_json::Value;

use super::Trial;
use crate::steps::{capture, say};

/// The services of the installation that run without a profile.
const SERVICES: [&str; 3] = ["fvoci", "postgres", "search"];

/// The role the server connects as, as the database's superuser sees it: no superuser,
/// no `BYPASSRLS`, no `CREATEDB` or `CREATEROLE`, the owner of no table, while the
/// tables with row-level security have it forced; and, as itself, unable to turn it off.
///
/// # Errors
///
/// Fails if the role passes the wall or owns a table, or the database cannot be asked.
pub(super) fn database_role(trial: &Trial) -> Result<()> {
    let query = "SELECT r.rolsuper OR r.rolbypassrls OR r.rolcreatedb OR r.rolcreaterole, \
         (SELECT count(*) FROM pg_tables WHERE schemaname = 'public' AND tableowner = r.rolname), \
         (SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
           WHERE n.nspname = 'public' AND c.relrowsecurity), \
         (SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
           WHERE n.nspname = 'public' AND c.relrowsecurity AND NOT c.relforcerowsecurity) \
         FROM pg_roles r WHERE r.rolname = 'fvoci'";
    let answer = trial.compose(&[
        "exec",
        "-T",
        "postgres",
        "psql",
        "--username=postgres",
        "--dbname=fvoci",
        "--no-align",
        "--tuples-only",
        "--field-separator=|",
        "--command",
        query,
    ])?;
    ensure!(answer.succeeded, "psql failed: {}", answer.error);
    let fields: Vec<&str> = answer.output.split('|').collect();
    let [past, owned, walled, unforced] = fields[..] else {
        bail!("the role fvoci is not there: {:?}", answer.output);
    };
    ensure!(
        past == "f",
        "fvoci is a superuser or may pass the wall or make roles: {}",
        answer.output
    );
    ensure!(owned == "0", "fvoci owns {owned} tables");
    let walled: u32 = walled.parse().context("the count of walled tables")?;
    ensure!(walled > 0, "no table of the schema has row-level security");
    ensure!(
        unforced == "0",
        "{unforced} walled tables do not force their policies"
    );

    // As fvoci, with the password the database container was given: the wall stays.
    let attempt = trial.compose(&[
        "exec", "-T", "postgres", "sh", "-c",
        "PGPASSWORD=$(cat /run/secrets/fvoci_db_password) psql --host=127.0.0.1 \
         --username=fvoci --dbname=fvoci --command='ALTER TABLE blocks NO FORCE ROW LEVEL SECURITY'",
    ])?;
    ensure!(
        !attempt.succeeded && attempt.error.contains("must be owner"),
        "fvoci was not refused the change of a table: {} {}",
        attempt.output,
        attempt.error
    );
    say(&format!(
        "    fvoci: no superuser, no BYPASSRLS, owns no table, may not alter one; \
         {walled} tables walled and forced"
    ));
    Ok(())
}

/// The containers of the services, by name.
fn containers(trial: &Trial) -> Result<Vec<Value>> {
    let answer = trial.compose(&["ps", "--format", "json"])?;
    ensure!(
        answer.succeeded,
        "docker compose ps failed: {}",
        answer.error
    );
    answer
        .output
        .lines()
        .map(|line| serde_json::from_str(line).with_context(|| format!("not JSON: {line}")))
        .collect()
}

/// Only fvoci is reachable from the host, and only on the loopback interface (the
/// trial's `.env` keeps the default address).
///
/// # Errors
///
/// Fails for any other published port.
pub(super) fn published_ports(trial: &Trial) -> Result<()> {
    let mut published = Vec::new();
    for container in containers(trial)? {
        let service = container
            .get("Service")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let publishers = container.get("Publishers").and_then(Value::as_array);
        for publisher in publishers.into_iter().flatten() {
            let port = publisher
                .get("PublishedPort")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if port != 0 {
                let address = publisher.get("URL").and_then(Value::as_str).unwrap_or("?");
                published.push((service.to_owned(), address.to_owned(), port));
            }
        }
    }
    let unexpected: Vec<_> = published
        .iter()
        .filter(|(service, address, _)| service != "fvoci" || address != "127.0.0.1")
        .collect();
    ensure!(
        unexpected.is_empty(),
        "published beyond fvoci on 127.0.0.1: {unexpected:?}"
    );
    ensure!(!published.is_empty(), "fvoci's port is not published");
    say(&format!("    published: {published:?}"));
    Ok(())
}

/// Every process of every service runs as a user other than root.
///
/// # Errors
///
/// Fails for a process of user ID 0.
pub(super) fn processes(trial: &Trial) -> Result<()> {
    for service in SERVICES {
        let id = trial.compose(&["ps", "--quiet", service])?;
        ensure!(
            id.succeeded && !id.output.is_empty(),
            "{service} is not running"
        );
        let top = capture("docker", &["top", &id.output, "-o", "pid,uid,comm"])?;
        ensure!(top.succeeded, "docker top {service} failed: {}", top.error);
        // pid, uid, command
        let rows: Vec<Vec<&str>> = top
            .output
            .lines()
            .skip(1)
            .map(|line| line.split_whitespace().collect())
            .collect();
        let root: Vec<_> = rows.iter().filter(|row| row.get(1) == Some(&"0")).collect();
        ensure!(root.is_empty(), "{service} runs as root: {root:?}");
        let users: Vec<&str> = rows.iter().filter_map(|row| row.get(1).copied()).collect();
        say(&format!("    {service}: user IDs {users:?}"));
    }
    Ok(())
}

/// None of the trial's secrets is in an image (its layers' commands, environment,
/// labels) or in the description of a container (its environment and command line).
///
/// # Errors
///
/// Fails naming where a secret was found, never the secret.
pub(super) fn secrets(trial: &Trial) -> Result<()> {
    let mut places = Vec::new();
    for image in [&trial.images.server.local, &trial.images.search.local] {
        let history = capture(
            "docker",
            &["history", "--no-trunc", "--format", "{{.CreatedBy}}", image],
        )?;
        let inspect = capture("docker", &["image", "inspect", image])?;
        ensure!(
            history.succeeded && inspect.succeeded,
            "{image} cannot be inspected"
        );
        places.push((format!("the history of {image}"), history.output));
        places.push((format!("the description of {image}"), inspect.output));
    }
    for service in SERVICES.iter().chain(&["migrate"]) {
        let id = trial.compose(&["ps", "--all", "--quiet", service])?;
        let inspect = capture("docker", &["inspect", &id.output])?;
        ensure!(inspect.succeeded, "{service} cannot be inspected");
        places.push((format!("the container of {service}"), inspect.output));
    }
    for (place, text) in &places {
        if trial
            .secrets
            .iter()
            .any(|secret| text.contains(secret.as_str()))
        {
            bail!("a secret is in {place}");
        }
    }
    say(&format!("    looked in {} places", places.len()));
    Ok(())
}
