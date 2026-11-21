//! Temporary, removed by the next commit: a test that fails on purpose, to see `ci-done`
//! turn red for it.

#[test]
fn ci_done_turns_red_when_a_test_fails() {
    let jobs_that_passed = 11;
    assert_eq!(jobs_that_passed, 12, "on purpose: `ci-done` must be red");
}
