#!/usr/bin/env bash
# Says which jobs of ci.yml this run may skip, and writes them to the job's output
# `skip` as a JSON list of job ids. A job runs unless its id is on the list, so an
# empty or missing output runs everything; `ci-done` passes a skipped job only when
# its id is on the list (verdict.sh).
#
#   EVENT=<github.event_name> BEFORE=<github.event.before> plan.sh
#
# Run in a checkout of the run's commit: for a pull request its test merge and that
# merge's parents, for a push the history of master. A push also needs REPOSITORY
# and GH_TOKEN (a token that may read the repository's runs).
#
# What a run changes:
# - a pull request: what its test merge changes in the commit it was merged into
#   (`HEAD^1`): the base branch, or for a layer of a stack the test merge of the
#   layer below;
# - a push to master: what the push changes (`BEFORE..HEAD`);
# - a run by hand: everything.
# When git cannot say, or nothing changed, nothing is skipped.
#
# The rules are wide: a job is skipped only when no file it could test changed.
# - Every file under apps/web/: the Rust tests are skipped. No cargo test reads the
#   web app; `cargo xtask check` does (names), and it always runs.
# - Every file a `.rs` under crates/ or xtask/src/: the web app's check, unit tests,
#   stories and the end-to-end tests of the build are skipped. Its build and
#   `e2e:live`, which runs the server, are not. The web app reads files of crates/
#   and xtask/ that are not Rust (crates/collab/tests/data/, xtask/exceptions/) and
#   the root's (deny.toml): those, like any other file, run everything.
# - A push that is one merge, whose branch had master in it and passed a pull
#   request's run of this workflow: the merged tree is that branch's, which that run
#   tested. Only the jobs that save the caches run, by the rules above.
set -euo pipefail

skip=()

# Adds the skips of the files changed from $1 to $2.
skip_by_files() {
  local list web_only=true rust_only=true any=false path
  list=$(mktemp)
  if ! git diff --name-only -z --no-renames "$1" "$2" -- >"$list"; then
    echo "plan: git cannot say what changed from $1 to $2: nothing is skipped"
    return 0
  fi
  while IFS= read -r -d '' path; do
    any=true
    case $path in apps/web/*) ;; *) web_only=false ;; esac
    case $path in crates/*.rs | xtask/src/*.rs) ;; *) rust_only=false ;; esac
  done <"$list"
  echo "plan: $(tr -cd '\0' <"$list" | wc -c) files changed; all the web app's:" \
    "$web_only; all Rust: $rust_only"
  if ! $any; then return 0; fi
  if $web_only; then skip+=(rust-sqlite rust-postgres); fi
  if $rust_only; then skip+=(web-checks web-e2e web-e2e-apple); fi
}

# Whether HEAD is one merge whose tree a pull request's run of this workflow tested.
tested_merge() {
  local commit first second more runs
  read -r commit first second more < <(git rev-list --parents --max-count=1 HEAD)
  if [[ -z $second || -n $more || $first != "$BEFORE" ]]; then
    echo "plan: the push is not one merge onto the master before it"
    return 1
  fi
  if ! git merge-base --is-ancestor "$first" "$second"; then
    echo "plan: the merged branch did not have master in it"
    return 1
  fi
  if ! runs=$(gh api --jq .total_count \
    "repos/$REPOSITORY/actions/workflows/ci.yml/runs?event=pull_request&status=success&head_sha=$second&per_page=1"); then
    echo "plan: GitHub did not say whether $second passed"
    return 1
  fi
  if ((runs == 0)); then
    echo "plan: no pull request's run passed on $second"
    return 1
  fi
  echo "plan: $commit has the tree of $second, which a pull request's run passed"
}

case ${EVENT:-} in
  pull_request)
    skip_by_files HEAD^1 HEAD
    ;;
  push)
    skip_by_files "$BEFORE" HEAD
    if tested_merge; then skip+=(web-build web-e2e web-e2e-apple web-e2e-live); fi
    ;;
  *)
    echo "plan: a run of ${EVENT:-no event} runs everything"
    ;;
esac

json=$(jq --null-input --compact-output '$ARGS.positional | unique' --args "${skip[@]}")
echo "plan: skip $json"
echo "skip=$json" >>"${GITHUB_OUTPUT:-/dev/stdout}"
