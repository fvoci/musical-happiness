#!/usr/bin/env python3
"""Runs every scenario against two servers and reports where their answers differ.

    scripts/differential/run.py --left http://localhost:18101 --right http://localhost:18102

Both servers must be freshly started with empty databases. A difference listed in
`expected.py` is reported as registered; any other difference makes the run fail.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from expected import EXPECTED  # noqa: E402
from harness import Session, compare, run_scenario  # noqa: E402
from scenarios import SCENARIOS  # noqa: E402


def bootstrap(base_url: str, label: str) -> str:
    """Registers the first user, `root`, and returns its session token."""
    session = Session(base_url, label)
    session.register("root")
    return session.tokens["root"]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--left", required=True, help="base URL of the reference server")
    parser.add_argument("--right", required=True, help="base URL of the server under test")
    parser.add_argument("--left-label", default="original")
    parser.add_argument("--right-label", default="port")
    parser.add_argument("--only", action="append", default=[], help="run only the named scenario(s)")
    parser.add_argument("--verbose", action="store_true", help="print every recorded step")
    args = parser.parse_args()

    labels = (args.left_label, args.right_label)
    root_tokens = (bootstrap(args.left, labels[0]), bootstrap(args.right, labels[1]))

    unexpected = registered = steps_total = 0
    for scenario in SCENARIOS:
        name = scenario.__name__
        if args.only and name not in args.only:
            continue
        recordings = []
        for base_url, label, root_token in zip((args.left, args.right), labels, root_tokens):
            def run(session: Session, token: str = root_token) -> None:
                session.tokens["root"] = token
                scenario(session)

            recordings.append(run_scenario(run, base_url, label))
        left, right = recordings
        steps_total += len(left)
        if args.verbose:
            for step in left:
                print(f"    {name}: {step.name}: {step.status} {step.request}")
        differences = compare(name, left, right, labels)
        expected_here = [d for d in differences if (name, d.step) in EXPECTED]
        unexpected_here = [d for d in differences if (name, d.step) not in EXPECTED]
        registered += len(expected_here)
        unexpected += len(unexpected_here)
        status = "ok" if not differences else f"{len(unexpected_here)} unexpected, {len(expected_here)} registered"
        print(f"{name}: {len(left)} steps, {status}")
        for difference in unexpected_here:
            print(f"  DIFF {difference.step} [{difference.request}]\n    {difference.detail}")
        for difference in expected_here:
            print(f"  registered: {difference.step} — {EXPECTED[(name, difference.step)]}")

    stale = [key for key in EXPECTED if not args.only or key[0] in args.only]
    print(f"\n{steps_total} steps compared, {unexpected} unexpected differences, {registered} registered differences")
    if not stale and not registered:
        pass
    return 1 if unexpected else 0


if __name__ == "__main__":
    sys.exit(main())
