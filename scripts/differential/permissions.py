#!/usr/bin/env python3
"""Compares, route by route and role by role, the status code two servers answer with.

    scripts/differential/permissions.py --left http://localhost:18101 --right http://localhost:18102

For every route of the standalone API that concerns a board, and for a private and an open
board, the same request is sent as six kinds of caller: anonymous, a signed-in user who is
no member of the board, and a viewer, commenter, editor and admin of it. Every request gets
a board of its own, so that what one caller changes cannot influence the next answer.

Both servers must be freshly started with empty databases. Differences listed in
`expected.py` under the scenario name "permissions" are reported as registered.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import Any, Callable

sys.path.insert(0, str(Path(__file__).parent))

from expected import EXPECTED  # noqa: E402
from harness import Session, multipart  # noqa: E402
from scenarios import API, PNG, block  # noqa: E402

ROLES = ["anonymous", "outsider", "viewer", "commenter", "editor", "admin"]
MEMBER_FLAGS = {
    "viewer": {"schemeViewer": True},
    "commenter": {"schemeCommenter": True},
    "editor": {"schemeEditor": True},
    "admin": {"schemeAdmin": True},
}


class Fixture:
    """A board with one member per role, some blocks, a file and a subscription."""

    def __init__(self, s: Session, board_type: str, ids: dict[str, str]):
        self.s, self.ids = s, ids
        reply = s.call("board", "POST", f"{API}/boards", {"teamId": "0", "type": board_type, "title": "Matrix", "properties": {}, "cardProperties": []},
                       user="owner", record=False)
        self.board = reply.json()["id"]
        for role, flags in MEMBER_FLAGS.items():
            s.call("member", "POST", f"{API}/boards/{self.board}/members",
                   {"boardId": self.board, "userId": ids[role], **flags}, user="owner", record=False)
        created = s.call("blocks", "POST", f"{API}/boards/{self.board}/blocks", [
            block("card", self.board, "card", "card"),
            block("text", self.board, "text", "text", parent="card"),
            block("text2", self.board, "text", "text2", parent="card"),
            block("view", self.board, "view", "view", viewType="board"),
        ], user="owner", record=False).json()
        by_title = {b["title"]: b["id"] for b in created}
        self.card, self.text, self.text2, self.view = (by_title[k] for k in ("card", "text", "text2", "view"))
        comment = s.call("comment", "POST", f"{API}/boards/{self.board}/blocks",
                         [block("comment", self.board, "comment", "comment", parent=self.card)],
                         user="owner", record=False).json()
        self.comment = comment[0]["id"]
        self._file: str | None = None

    @property
    def file(self) -> str:
        if self._file is None:
            body, content_type = multipart("file", "pixel.png", PNG, "image/png")
            reply = self.s.call("file", "POST", f"{API}/teams/0/{self.board}/files", raw_body=body,
                                content_type=content_type, user="owner", record=False)
            self._file = reply.json()["fileId"]
        return self._file

    def owner(self, method: str, path: str, body: Any = None) -> None:
        self.s.call("prepare", method, path, body, user="owner", record=False)


Request = tuple[str, str, Any]  # method, path, JSON body (or a (bytes, content type) pair)


def routes() -> list[tuple[str, Callable[[Fixture, str], Request]]]:
    """Every route with a function that prepares the fixture and returns the request to send."""
    def board(suffix: str = "") -> Callable[[Fixture], str]:
        return lambda fx: f"{API}/boards/{fx.board}{suffix}"

    def simple(method: str, path: Callable[[Fixture], str], body: Callable[[Fixture, str], Any] | None = None):
        return lambda fx, me: (method, path(fx), body(fx, me) if body else None)

    def after(prepare: Callable[[Fixture], None], method: str, path: Callable[[Fixture], str], body: Any = None):
        def build(fx: Fixture, me: str) -> Request:
            prepare(fx)
            return method, path(fx), body
        return build

    def upload(fx: Fixture, me: str) -> Request:
        return "POST", f"{API}/teams/0/{fx.board}/files", multipart("file", "pixel.png", PNG, "image/png")

    def import_archive(fx: Fixture, me: str) -> Request:
        exported = fx.s.call("export", "GET", f"{API}/boards/{fx.board}/archive/export", user="owner", record=False)
        return "POST", f"{API}/teams/0/archive/import", multipart("file", "a.boardarchive", exported.body)

    def member_body(role_flags: dict) -> Callable[[Fixture, str], Any]:
        return lambda fx, me: {"boardId": fx.board, "userId": fx.ids["extra"], **role_flags}

    def subscription(fx: Fixture, me: str) -> Any:
        return {"blockType": "card", "blockId": fx.card, "subscriberType": "user", "subscriberId": me}

    def subscribe_first(fx: Fixture, me: str) -> Request:
        if me:
            fx.s.call("subscribe", "POST", f"{API}/subscriptions", subscription(fx, me),
                      token=fx.s.tokens_by_id[me], record=False)
        return "DELETE", f"{API}/subscriptions/{fx.card}/{me or 'nobody'}", None

    return [
        ("board: get", simple("GET", board())),
        ("board: patch title", simple("PATCH", board(), lambda fx, me: {"title": "renamed"})),
        ("board: patch properties", simple("PATCH", board(), lambda fx, me: {"updatedProperties": {"a": 1}})),
        ("board: patch type", simple("PATCH", board(), lambda fx, me: {"type": "O"})),
        ("board: patch minimum role", simple("PATCH", board(), lambda fx, me: {"minimumRole": "editor"})),
        ("board: delete", simple("DELETE", board())),
        ("board: undelete", after(lambda fx: fx.owner("DELETE", f"{API}/boards/{fx.board}"), "POST", board("/undelete"))),
        ("board: duplicate", simple("POST", board("/duplicate"))),
        ("board: duplicate as template", simple("POST", board("/duplicate?asTemplate=true"))),
        ("board: metadata", simple("GET", board("/metadata"))),
        ("members: list", simple("GET", board("/members"))),
        ("members: add viewer", simple("POST", board("/members"), member_body({"schemeViewer": True}))),
        ("members: add admin", simple("POST", board("/members"), member_body({"schemeAdmin": True}))),
        ("members: update", simple("PUT", lambda fx: f"{API}/boards/{fx.board}/members/{fx.ids['viewer']}",
                                    lambda fx, me: {"boardId": fx.board, "userId": fx.ids["viewer"], "schemeEditor": True})),
        ("members: delete another", simple("DELETE", lambda fx: f"{API}/boards/{fx.board}/members/{fx.ids['viewer']}")),
        ("members: join", simple("POST", board("/join"))),
        ("members: leave", simple("POST", board("/leave"))),
        ("blocks: list", simple("GET", board("/blocks"))),
        ("blocks: list all", simple("GET", board("/blocks?all=true"))),
        ("blocks: list by parent", simple("GET", lambda fx: f"{API}/boards/{fx.board}/blocks?parent_id={fx.card}")),
        ("blocks: insert card", simple("POST", board("/blocks"), lambda fx, me: [block("n", fx.board, "card", "n")])),
        ("blocks: insert view", simple("POST", board("/blocks"), lambda fx, me: [block("n", fx.board, "view", "n")])),
        ("blocks: insert comment", simple("POST", board("/blocks"),
                                          lambda fx, me: [block("n", fx.board, "comment", "n", parent=fx.card)])),
        ("blocks: patch card", simple("PATCH", lambda fx: f"{API}/boards/{fx.board}/blocks/{fx.card}",
                                      lambda fx, me: {"title": "patched"})),
        ("blocks: patch another's comment", simple("PATCH", lambda fx: f"{API}/boards/{fx.board}/blocks/{fx.comment}",
                                                    lambda fx, me: {"title": "patched"})),
        ("blocks: patch several", simple("PATCH", board("/blocks"),
                                         lambda fx, me: {"block_ids": [fx.card], "block_patches": [{"title": "p"}]})),
        ("blocks: delete card", simple("DELETE", lambda fx: f"{API}/boards/{fx.board}/blocks/{fx.card}")),
        ("blocks: delete view", simple("DELETE", lambda fx: f"{API}/boards/{fx.board}/blocks/{fx.view}")),
        ("blocks: delete another's comment", simple("DELETE", lambda fx: f"{API}/boards/{fx.board}/blocks/{fx.comment}")),
        ("blocks: undelete", after(lambda fx: fx.owner("DELETE", f"{API}/boards/{fx.board}/blocks/{fx.view}"),
                                   "POST", lambda fx: f"{API}/boards/{fx.board}/blocks/{fx.view}/undelete")),
        ("blocks: duplicate", simple("POST", lambda fx: f"{API}/boards/{fx.board}/blocks/{fx.card}/duplicate")),
        ("boards-and-blocks: patch", simple("PATCH", lambda fx: f"{API}/boards-and-blocks", lambda fx, me: {
            "boardIDs": [fx.board], "boardPatches": [{"title": "p"}],
            "blockIDs": [fx.card], "blockPatches": [{"title": "p"}]})),
        ("boards-and-blocks: delete", simple("DELETE", lambda fx: f"{API}/boards-and-blocks",
                                             lambda fx, me: {"boards": [fx.board], "blocks": [fx.card]})),
        ("cards: create", simple("POST", board("/cards"), lambda fx, me: {"title": "new"})),
        ("cards: list", simple("GET", board("/cards"))),
        ("cards: get", simple("GET", lambda fx: f"{API}/cards/{fx.card}")),
        ("cards: patch", simple("PATCH", lambda fx: f"{API}/cards/{fx.card}", lambda fx, me: {"title": "patched"})),
        ("content: move", after(
            lambda fx: fx.owner("PATCH", f"{API}/cards/{fx.card}", {"contentOrder": [fx.text, fx.text2]}),
            "POST", lambda fx: f"{API}/content-blocks/{fx.text}/moveto/after/{fx.text2}")),
        ("sharing: get", after(
            lambda fx: fx.owner("POST", f"{API}/boards/{fx.board}/sharing", {"id": fx.board, "enabled": True, "token": "t"}),
            "GET", board("/sharing"))),
        ("sharing: post", simple("POST", board("/sharing"), lambda fx, me: {"id": fx.board, "enabled": True, "token": "t"})),
        ("subscriptions: create", simple("POST", lambda fx: f"{API}/subscriptions", subscription)),
        ("subscriptions: delete", subscribe_first),
        ("files: upload", upload),
        ("files: download", simple("GET", lambda fx: f"{API}/files/teams/0/{fx.board}/{fx.file}")),
        ("files: info", simple("GET", lambda fx: f"{API}/files/teams/0/{fx.board}/{fx.file}/info")),
        ("archive: export board", simple("GET", board("/archive/export"))),
        ("archive: import", import_archive),
        ("archive: export team", simple("GET", lambda fx: f"{API}/teams/0/archive/export")),
        ("categories: list", simple("GET", lambda fx: f"{API}/teams/0/categories")),
        ("team: boards", simple("GET", lambda fx: f"{API}/teams/0/boards")),
        ("team: templates", simple("GET", lambda fx: f"{API}/teams/0/templates")),
        ("team: search", simple("GET", lambda fx: f"{API}/teams/0/boards/search?q=Matrix")),
        ("team: users", simple("GET", lambda fx: f"{API}/teams/0/users")),
    ]


def record(base_url: str, label: str, only: list[str]) -> dict[tuple[str, str, str], int]:
    s = Session(base_url, label)
    s.register("owner")
    token = s.signup_token("owner")
    for name in ["outsider", "viewer", "commenter", "editor", "admin", "extra"]:
        s.register(name, signup_token=token)
    ids = {name: s.user_id(name) for name in s.tokens}
    s.tokens_by_id = {ids[name]: s.tokens[name] for name in ids}  # type: ignore[attr-defined]

    results: dict[tuple[str, str, str], int] = {}
    for board_type in ("P", "O"):
        for name, build in routes():
            if only and not any(part in name for part in only):
                continue
            for role in ROLES:
                fx = Fixture(s, board_type, ids)
                me = "" if role == "anonymous" else ids[role]
                method, path, body = build(fx, me)
                kwargs: dict[str, Any] = {"record": False}
                if role != "anonymous":
                    kwargs["user"] = role
                if isinstance(body, tuple):
                    raw, content_type = body
                    reply = s.call(name, method, path, raw_body=raw, content_type=content_type, **kwargs)
                else:
                    reply = s.call(name, method, path, body, **kwargs)
                results[(board_type, name, role)] = reply.status
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--left", required=True)
    parser.add_argument("--right", required=True)
    parser.add_argument("--left-label", default="original")
    parser.add_argument("--right-label", default="port")
    parser.add_argument("--only", action="append", default=[], help="only routes whose name contains this text")
    parser.add_argument("--table", action="store_true", help="print the whole table of the left server")
    args = parser.parse_args()

    left = record(args.left, args.left_label, args.only)
    right = record(args.right, args.right_label, args.only)

    if args.table:
        for board_type in ("P", "O"):
            print(f"\n{'private' if board_type == 'P' else 'open'} board ({args.left_label})")
            print(f"  {'route':36}" + "".join(f"{role:>11}" for role in ROLES))
            for name in dict.fromkeys(key[1] for key in left):
                print(f"  {name:36}" + "".join(f"{left[(board_type, name, role)]:>11}" for role in ROLES))

    unexpected = registered = 0
    for key in left:
        if left[key] == right.get(key):
            continue
        board_type, name, role = key
        step = f"{'private' if board_type == 'P' else 'open'}: {name}: {role}"
        reason = EXPECTED.get(("permissions", step))
        if reason:
            registered += 1
            print(f"  registered: {step}: {left[key]} vs {right.get(key)} — {reason}")
        else:
            unexpected += 1
            print(f"  DIFF {step}: {args.left_label} {left[key]}, {args.right_label} {right.get(key)}")
    print(f"\n{len(left)} cells compared, {unexpected} unexpected differences, {registered} registered differences")
    return 1 if unexpected else 0


if __name__ == "__main__":
    sys.exit(main())
