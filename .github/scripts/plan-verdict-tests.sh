#!/usr/bin/env bash
# The table tests of plan.sh and verdict.sh, on repositories made for each case and
# a stand-in for `gh`. A push to master cannot be run before the workflow is there:
# this is where its rules are tried.
#
#   plan-verdict-tests.sh
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
failed=0

# The stand-in for `gh api`: the count of FAKE_RUNS, when the question is for the
# passed pull request runs of FAKE_SHA; 0 for any other question; fails when
# FAKE_RUNS is `fail`.
mkdir "$WORK/bin"
cat >"$WORK/bin/gh" <<'EOF'
#!/usr/bin/env bash
[[ ${FAKE_RUNS:-fail} == fail ]] && exit 1
case "$*" in
  *"event=pull_request"*"status=success"*"head_sha=$FAKE_SHA"*) echo "$FAKE_RUNS" ;;
  *) echo 0 ;;
esac
EOF
chmod +x "$WORK/bin/gh"

RUST='"rust-postgres","rust-sqlite"'
WEB='"web-checks","web-e2e","web-e2e-apple"'
MERGED='"web-build","web-e2e","web-e2e-apple","web-e2e-live"'

# A new repository with one commit on master.
repo() {
  local dir="$WORK/repo-$1"
  git init --quiet --initial-branch master "$dir"
  cd "$dir"
  git config user.name test
  git config user.email test@example.invalid
  put README 'one'
  put apps/web/src/app.ts 'one'
  put crates/model/src/lib.rs 'one'
  git add --all && git commit --quiet --message base
}

# Writes $2 to the file $1.
put() {
  mkdir -p "$(dirname "$1")"
  printf '%s\n' "$2" >"$1"
}

# On a branch from master, changes the files given as path=text pairs (or removes
# them, given as -path) and commits; then merges it into master, as GitHub's test
# merge or a merge commit does.
merge_change() {
  git checkout --quiet -b "change-$RANDOM"
  change "$@"
  git checkout --quiet master
  git merge --quiet --no-ff --no-edit "@{-1}"
}

change() {
  local pair
  for pair in "$@"; do
    case $pair in
      -*) git rm --quiet "${pair#-}" ;;
      *) put "${pair%%=*}" "${pair#*=}" ;;
    esac
  done
  git add --all
  git commit --quiet --allow-empty --message change
}

# Runs plan.sh here and compares its list with the expected one.
expect_plan() {
  local name=$1 want="[$2]" got
  shift 2
  got=$(env PATH="$WORK/bin:$PATH" GITHUB_OUTPUT=/dev/stdout REPOSITORY=fvoci/test "$@" \
    bash "$HERE/plan.sh" | sed -n 's/^skip=//p')
  if [[ $got == "$want" ]]; then
    echo "ok    plan: $name"
  else
    echo "FAIL  plan: $name: skip $got, not $want"
    failed=1
  fi
}

repo web
merge_change apps/web/src/app.ts=two 'apps/web/src/a name
with a new line.ts=x'
expect_plan "a pull request of the web app's files skips the Rust tests" "$RUST" \
  EVENT=pull_request

repo rust
merge_change crates/model/src/lib.rs=two xtask/src/main.rs=x
expect_plan "a pull request of .rs files skips the web app's tests" "$WEB" EVENT=pull_request

repo rust-data
merge_change crates/model/src/lib.rs=two crates/collab/tests/data/made-here.json=x
expect_plan "data under crates/ (the web app's tests read some) skips nothing" "" \
  EVENT=pull_request

repo xtask-exceptions
merge_change apps/web/src/app.ts=two xtask/exceptions/names.txt=x
expect_plan "xtask/exceptions/ (the web app reads it) skips nothing" "" EVENT=pull_request

repo cargo
merge_change Cargo.lock=x
expect_plan "Cargo.lock skips nothing" "" EVENT=pull_request

repo both
merge_change apps/web/src/app.ts=two crates/model/src/lib.rs=two
expect_plan "a pull request of both skips nothing" "" EVENT=pull_request

repo workflow
merge_change .github/workflows/ci.yml=x
expect_plan "the workflow skips nothing" "" EVENT=pull_request

repo moved
git checkout --quiet -b moved
git mv apps/web/src/app.ts crates/model/src/app.rs
git commit --quiet --message moved
git checkout --quiet master
git merge --quiet --no-ff --no-edit moved
expect_plan "a file moved from the web app to Rust counts on both sides" "" EVENT=pull_request

repo removed
merge_change -crates/model/src/lib.rs
expect_plan "a removed .rs file counts as Rust" "$WEB" EVENT=pull_request

repo empty
merge_change
expect_plan "a pull request that changes nothing skips nothing" "" EVENT=pull_request

repo root
expect_plan "a commit without a parent skips nothing" "" EVENT=pull_request

repo dispatch
merge_change apps/web/src/app.ts=two
expect_plan "a run by hand skips nothing" "" EVENT=workflow_dispatch
expect_plan "a run without an event skips nothing" "" EVENT=

# Pushes to master: BEFORE is master before the push.
repo pushed
before=$(git rev-parse HEAD)
merge_change apps/web/src/app.ts=two
branch=$(git rev-parse HEAD^2)
expect_plan "a merge whose branch passed skips what it tested" "$RUST,$MERGED" \
  EVENT=push BEFORE="$before" FAKE_RUNS=1 FAKE_SHA="$branch"
expect_plan "a merge whose branch did not pass skips by its files only" "$RUST" \
  EVENT=push BEFORE="$before" FAKE_RUNS=0 FAKE_SHA="$branch"
expect_plan "a merge when GitHub does not answer skips by its files only" "$RUST" \
  EVENT=push BEFORE="$before" FAKE_RUNS=fail FAKE_SHA="$branch"
expect_plan "a merge whose branch's runs were of another commit skips by its files only" \
  "$RUST" EVENT=push BEFORE="$before" FAKE_RUNS=1 FAKE_SHA="$before"

repo pushed-both
before=$(git rev-parse HEAD)
merge_change apps/web/src/app.ts=two crates/model/src/lib.rs=two
expect_plan "a merge of both whose branch passed skips the end-to-end tests" "$MERGED" \
  EVENT=push BEFORE="$before" FAKE_RUNS=1 FAKE_SHA="$(git rev-parse HEAD^2)"

repo behind
git checkout --quiet -b old
change apps/web/src/app.ts=two
git checkout --quiet master
change README=moved
before=$(git rev-parse HEAD)
git merge --quiet --no-ff --no-edit old
expect_plan "a merge whose branch had not master in it skips by its files only" "$RUST" \
  EVENT=push BEFORE="$before" FAKE_RUNS=1 FAKE_SHA="$(git rev-parse HEAD^2)"

repo two-merges
before=$(git rev-parse HEAD)
merge_change apps/web/src/app.ts=two
merge_change apps/web/src/app.ts=three
expect_plan "two merges pushed at once skip by their files only" "$RUST" \
  EVENT=push BEFORE="$before" FAKE_RUNS=1 FAKE_SHA="$(git rev-parse HEAD^2)"

repo no-merge
before=$(git rev-parse HEAD)
change crates/model/src/lib.rs=two
expect_plan "a commit pushed without a merge skips by its files only" "$WEB" \
  EVENT=push BEFORE="$before" FAKE_RUNS=1 FAKE_SHA="$(git rev-parse HEAD)"

repo new-branch
expect_plan "a push git cannot compare skips nothing" "" \
  EVENT=push BEFORE=0000000000000000000000000000000000000000 FAKE_RUNS=fail

# The verdict, on the `needs` of ci-done as GitHub gives it.
expect_verdict() {
  local name=$1 want=$2 needs=$3 got=0
  NEEDS=$needs bash "$HERE/verdict.sh" >/dev/null || got=$?
  if [[ $got == "$want" ]]; then
    echo "ok    verdict: $name"
  else
    echo "FAIL  verdict: $name: exit $got, not $want"
    failed=1
  fi
}

# `needs` with the plan's result and list, and the given results of two jobs.
needs() {
  jq --null-input --compact-output --arg plan "$1" --arg skip "$2" --arg a "$3" --arg b "$4" \
    '{plan: {result: $plan, outputs: (if $skip == "-" then {} else {skip: $skip} end)},
      "rust-sqlite": {result: $a, outputs: {}}, "web-e2e": {result: $b, outputs: {}}}'
}

expect_verdict "all passed" 0 "$(needs success '[]' success success)"
expect_verdict "one failed" 1 "$(needs success '[]' success failure)"
expect_verdict "one was cancelled" 1 "$(needs success '[]' cancelled success)"
expect_verdict "one skipped by the plan" 0 "$(needs success '["web-e2e"]' success skipped)"
expect_verdict "one skipped that the plan ran" 1 "$(needs success '["rust-sqlite"]' success skipped)"
expect_verdict "one skipped by the plan ran anyway" 0 "$(needs success '["web-e2e"]' success success)"
expect_verdict "the plan failed" 1 "$(needs failure - skipped skipped)"
expect_verdict "the plan was cancelled" 1 "$(needs cancelled - skipped skipped)"
expect_verdict "the plan gave no list" 1 "$(needs success - success success)"
expect_verdict "the plan's list is not JSON" 1 "$(needs success 'web-e2e' success skipped)"
expect_verdict "the plan names a job not waited for" 1 "$(needs success '["web-e2e","nothing"]' success skipped)"
expect_verdict "the plan skips itself" 1 "$(needs success '["plan"]' success success)"

exit "$failed"
