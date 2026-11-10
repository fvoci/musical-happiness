#!/usr/bin/env python3
"""Database round trip: what one server wrote, another server must read back unchanged.

    roundtrip.py populate <base-url> <snapshot.json>   # write data, record what the API shows
    roundtrip.py verify   <base-url> <snapshot.json>   # read the same again, compare

`populate` runs against the first server on an empty database. That server is then stopped
and the second one started on the same database and files directory; `verify` signs in
with the same passwords, checks that the old session tokens still work, and compares
every recorded answer byte for byte (lists sorted, nothing normalized: identifiers and
timestamps come from the same database rows and must be equal).
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).parent))

from harness import Session, multipart  # noqa: E402
from scenarios import API, PNG, block  # noqa: E402

# User preferences are filed under a category whose value is the product name, which the
# port changed on purpose (root PORTING.md, decision 1); they do not carry over.
NOT_CARRIED_OVER = {"alice config"}


def canonical(value: Any) -> Any:
    if isinstance(value, dict):
        return {key: canonical(item) for key, item in sorted(value.items())}
    if isinstance(value, list):
        items = [canonical(item) for item in value]
        if items and all(isinstance(item, dict) for item in items):
            items.sort(key=lambda item: json.dumps(item, sort_keys=True))
        return items
    if isinstance(value, bytes):
        return {"sha256": hashlib.sha256(value).hexdigest(), "length": len(value)}
    return value


def observe(s: Session, paths: dict[str, tuple[str, str | None]]) -> dict[str, Any]:
    seen = {}
    for name, (path, user) in paths.items():
        reply = s.call(name, "GET", path, user=user, record=False)
        seen[name] = {"status": reply.status, "body": canonical(reply.body)}
    return seen


def populate(base_url: str, snapshot_path: Path) -> None:
    s = Session(base_url, "writer")
    s.register("alice")
    s.register("bob", signup_token=s.signup_token("alice"))
    alice_id, bob_id = s.user_id("alice"), s.user_id("bob")

    board = s.call("board", "POST", f"{API}/boards", {
        "teamId": "0", "type": "P", "title": "Round trip", "description": "written by the first server",
        "icon": "🧭", "showDescription": True, "properties": {"k": "v", "n": 3},
        "cardProperties": [{"id": "p1", "name": "Status", "type": "select",
                            "options": [{"id": "o1", "value": "Open", "color": "propColorBlue"}]}],
    }, user="alice", record=False).json()["id"]
    open_board = s.call("open board", "POST", f"{API}/boards",
                        {"teamId": "0", "type": "O", "title": "Open", "properties": {}, "cardProperties": []},
                        user="alice", record=False).json()["id"]
    s.call("member", "POST", f"{API}/boards/{board}/members",
           {"boardId": board, "userId": bob_id, "schemeCommenter": True}, user="alice", record=False)

    body, content_type = multipart("file", "pixel.png", PNG, "image/png")
    file_id = s.call("file", "POST", f"{API}/teams/0/{board}/files", raw_body=body, content_type=content_type,
                     user="alice", record=False).json()["fileId"]
    created = s.call("blocks", "POST", f"{API}/boards/{board}/blocks", [
        block("card", board, "card", "Card with ünïcödé and 한글", icon="🃏", properties={"p1": "o1"}, contentOrder=[]),
        block("text", board, "text", "## heading\n\nbody", parent="card"),
        block("image", board, "image", "", parent="card", fileId=file_id),
        block("view", board, "view", "Table", viewType="table", sortOptions=[], visiblePropertyIds=["p1"]),
        block("gone", board, "card", "Deleted card"),
    ], user="alice", record=False).json()
    by_title = {b["title"]: b["id"] for b in created}
    card = by_title["Card with ünïcödé and 한글"]
    s.call("patch", "PATCH", f"{API}/boards/{board}/blocks/{card}",
           {"updatedFields": {"contentOrder": [by_title["## heading\n\nbody"]]}}, user="alice", record=False)
    s.call("comment", "POST", f"{API}/boards/{board}/blocks",
           [block("comment", board, "comment", "a comment by bob", parent=card)], user="bob", record=False)
    s.call("delete", "DELETE", f"{API}/boards/{board}/blocks/{by_title['Deleted card']}", user="alice", record=False)

    category = s.call("category", "POST", f"{API}/teams/0/categories",
                      {"name": "Projects", "userID": alice_id, "teamID": "0"}, user="alice", record=False).json()["id"]
    s.call("file board", "POST", f"{API}/teams/0/categories/{category}/boards/{board}", user="alice", record=False)
    s.call("share", "POST", f"{API}/boards/{board}/sharing", {"id": board, "enabled": True, "token": "round-trip-token"},
           user="alice", record=False)
    s.call("subscribe", "POST", f"{API}/subscriptions",
           {"blockType": "card", "blockId": card, "subscriberType": "user", "subscriberId": alice_id},
           user="alice", record=False)
    s.call("config", "PUT", f"{API}/users/{alice_id}/config",
           {"updatedFields": {"welcomePageViewed": "1"}, "deletedFields": []}, user="alice", record=False)

    paths: dict[str, tuple[str, str | None]] = {
        "alice": (f"{API}/users/me", "alice"),
        "bob as seen by alice": (f"{API}/users/{bob_id}", "alice"),
        "team": (f"{API}/teams/0", "alice"),
        "team users": (f"{API}/teams/0/users", "alice"),
        "alice boards": (f"{API}/teams/0/boards", "alice"),
        "bob boards": (f"{API}/teams/0/boards", "bob"),
        "board": (f"{API}/boards/{board}", "alice"),
        "open board as bob": (f"{API}/boards/{open_board}", "bob"),
        "blocks": (f"{API}/boards/{board}/blocks?all=true", "alice"),
        "cards": (f"{API}/boards/{board}/cards", "alice"),
        "members": (f"{API}/boards/{board}/members", "alice"),
        "alice memberships": (f"{API}/users/me/memberships", "alice"),
        "categories": (f"{API}/teams/0/categories", "alice"),
        "sharing": (f"{API}/boards/{board}/sharing", "alice"),
        "board by read token": (f"{API}/boards/{board}?read_token=round-trip-token", None),
        "blocks by read token": (f"{API}/boards/{board}/blocks?read_token=round-trip-token", None),
        "subscriptions": (f"{API}/subscriptions/{alice_id}", "alice"),
        "file": (f"{API}/files/teams/0/{board}/{file_id}", "alice"),
        "file info": (f"{API}/files/teams/0/{board}/{file_id}/info", "alice"),
        "templates": (f"{API}/teams/0/templates", "alice"),
        "search": (f"{API}/teams/0/boards/search?q=Round", "alice"),
        "alice config": (f"{API}/users/me/config", "alice"),
    }
    snapshot = {"tokens": s.tokens, "paths": {name: list(value) for name, value in paths.items()},
                "seen": observe(s, paths), "board": board, "card": card}
    snapshot_path.write_text(json.dumps(snapshot, ensure_ascii=False, indent=1))
    print(f"populate: wrote {len(paths)} observations to {snapshot_path}")


def verify(base_url: str, snapshot_path: Path) -> int:
    snapshot = json.loads(snapshot_path.read_text())
    s = Session(base_url, "reader")
    # The sessions the first server created must still be valid.
    s.tokens = dict(snapshot["tokens"])
    paths = {name: (value[0], value[1]) for name, value in snapshot["paths"].items()}
    seen = observe(s, paths)

    failures = 0
    for name, before in snapshot["seen"].items():
        after = seen[name]
        if before == after:
            continue
        if name in NOT_CARRIED_OVER:
            print(f"  registered: {name}: not carried over between the servers (see the root PORTING.md, decision 1)")
            continue
        failures += 1
        print(f"  DIFF {name}:\n    before: {json.dumps(before, ensure_ascii=False)[:600]}\n    after:  {json.dumps(after, ensure_ascii=False)[:600]}")

    # The passwords the first server hashed must still sign in, and the second server
    # must be able to write to what the first one created.
    fresh = Session(base_url, "reader")
    for user in ("alice", "bob"):
        try:
            fresh.login(user)
        except AssertionError as error:
            failures += 1
            print(f"  DIFF login {user}: {error}")
    if "alice" in fresh.tokens:
        reply = fresh.call("write", "PATCH", f"{API}/boards/{snapshot['board']}/blocks/{snapshot['card']}",
                           {"title": "patched by the second server"}, user="alice", record=False)
        if reply.status != 200:
            failures += 1
            print(f"  DIFF write after the switch: {reply.status} {reply.body}")
        reply = fresh.call("insert", "POST", f"{API}/boards/{snapshot['board']}/blocks",
                           [block("n", snapshot["board"], "card", "inserted by the second server")],
                           user="alice", record=False)
        if reply.status != 200:
            failures += 1
            print(f"  DIFF insert after the switch: {reply.status} {reply.body}")
    print(f"verify: {len(snapshot['seen'])} observations compared, {failures} differences")
    return 1 if failures else 0


def main() -> int:
    if len(sys.argv) != 4 or sys.argv[1] not in ("populate", "verify"):
        print(__doc__)
        return 2
    command, base_url, snapshot_path = sys.argv[1], sys.argv[2], Path(sys.argv[3])
    if command == "populate":
        populate(base_url, snapshot_path)
        return 0
    return verify(base_url, snapshot_path)


if __name__ == "__main__":
    sys.exit(main())
