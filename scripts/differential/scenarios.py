"""Request sequences run against both servers. Each scenario registers its own users
through `users()`, so scenarios do not depend on each other or on their order."""

from __future__ import annotations

import json as jsonlib

from harness import Session, multipart

API = "/api/v2"


def users(s: Session, scenario: str, *names: str) -> list[str]:
    """Registers one user per name, unique to the scenario, and returns the user names."""
    token = s.signup_token("root")
    created = []
    for name in names:
        user = f"{scenario}-{name}"
        s.register(user, signup_token=token)
        created.append(user)
    return created


def new_board(s: Session, user: str, title: str = "Board", board_type: str = "P", **extra) -> str:
    body = {"teamId": "0", "type": board_type, "title": title, "properties": {}, "cardProperties": []}
    body.update(extra)
    reply = s.call(f"create board {title}", "POST", f"{API}/boards", body, user=user)
    return reply.json()["id"]


def new_blocks(s: Session, user: str, board: str, blocks: list[dict], name: str = "insert blocks") -> list[dict]:
    reply = s.call(name, "POST", f"{API}/boards/{board}/blocks", blocks, user=user)
    return reply.json()


def block(block_id: str, board: str, block_type: str, title: str = "", parent: str = "", **fields) -> dict:
    return {
        "id": block_id,
        "boardId": board,
        "parentId": parent or board,
        "type": block_type,
        "title": title,
        "schema": 1,
        "fields": fields,
        "createAt": 1,
        "updateAt": 1,
        "deleteAt": 0,
    }


def add_member(s: Session, admin: str, board: str, user: str, role: str) -> None:
    flags = {
        "admin": {"schemeAdmin": True},
        "editor": {"schemeEditor": True},
        "commenter": {"schemeCommenter": True},
        "viewer": {"schemeViewer": True},
    }[role]
    body = {"boardId": board, "userId": s.user_id(user), **flags}
    s.call(f"add {role}", "POST", f"{API}/boards/{board}/members", body, user=admin)


# -- scenarios ------------------------------------------------------------------------


def system(s: Session) -> None:
    s.call("hello", "GET", "/hello", csrf=False)
    ping = s.call("ping", "GET", "/ping", csrf=False, record=False).json()
    s.note("ping members", sorted(ping.keys()))
    s.note("ping version and sku", [ping["version"], ping["sku"], ping["db_type"]])
    s.call("client config", "GET", f"{API}/clientConfig")
    s.call("client config without csrf header", "GET", f"{API}/clientConfig", csrf=False)
    s.call("unknown api path", "GET", f"{API}/no-such-route")
    s.call("wrong method on a known path", "DELETE", f"{API}/clientConfig")
    s.call("index for an app route", "GET", "/board/abc", csrf=False, keep_headers=("Content-Type",))
    s.call("static without slash", "GET", "/static", csrf=False)
    s.call("missing static file", "GET", "/static/nope.js", csrf=False)


def auth(s: Session) -> None:
    (alice,) = users(s, "auth", "alice")
    token = s.signup_token("root")
    s.call("register without token", "POST", f"{API}/register",
           {"username": "auth-x", "email": "auth-x@example.com", "password": "test-password"})
    s.call("register with a wrong token", "POST", f"{API}/register",
           {"username": "auth-x", "email": "auth-x@example.com", "password": "test-password", "token": "nope"})
    for name, body in [
        ("no username", {"username": "", "email": "a@example.com", "password": "test-password"}),
        ("no email", {"username": "auth-y", "email": "", "password": "test-password"}),
        ("bad email", {"username": "auth-y", "email": "not-an-email", "password": "test-password"}),
        ("no password", {"username": "auth-y", "email": "auth-y@example.com", "password": ""}),
        ("short password", {"username": "auth-y", "email": "auth-y@example.com", "password": "1234567"}),
        ("long password", {"username": "auth-y", "email": "auth-y@example.com", "password": "x" * 65}),
        ("taken username", {"username": alice, "email": "other@example.com", "password": "test-password"}),
        ("taken email", {"username": "auth-z", "email": f"{alice}@example.com", "password": "test-password"}),
    ]:
        s.call(f"register: {name}", "POST", f"{API}/register", {**body, "token": token})

    s.call("login", "POST", f"{API}/login", {"type": "normal", "username": alice, "password": "test-password"})
    s.call("login by email", "POST", f"{API}/login",
           {"type": "normal", "email": f"{alice}@example.com", "password": "test-password"})
    s.call("login: wrong password", "POST", f"{API}/login", {"type": "normal", "username": alice, "password": "nope"})
    s.call("login: unknown user", "POST", f"{API}/login", {"type": "normal", "username": "nobody", "password": "x"})
    s.call("login: wrong type", "POST", f"{API}/login", {"type": "sso", "username": alice, "password": "test-password"})
    s.call("login: empty body", "POST", f"{API}/login", {})

    s.call("me", "GET", f"{API}/users/me", user=alice)
    s.call("me without session", "GET", f"{API}/users/me")
    s.call("me with a bad token", "GET", f"{API}/users/me", token="not-a-token")
    s.call("my memberships", "GET", f"{API}/users/me/memberships", user=alice)
    s.call("my config", "GET", f"{API}/users/me/config", user=alice)
    alice_id = s.user_id(alice)
    s.call("user by id", "GET", f"{API}/users/{alice_id}", user=alice)
    s.call("unknown user", "GET", f"{API}/users/unknown-user", user=alice)
    s.call("users by id", "POST", f"{API}/users", [alice_id], user=alice)
    s.call("users by id: empty list", "POST", f"{API}/users", [], user=alice)
    s.call("update my config", "PUT", f"{API}/users/{alice_id}/config",
           {"updatedFields": {"welcomePageViewed": "1", "theme": "dark"}, "deletedFields": []}, user=alice)
    s.call("delete a config entry", "PUT", f"{API}/users/{alice_id}/config",
           {"updatedFields": {}, "deletedFields": ["theme"]}, user=alice)
    s.call("my config afterwards", "GET", f"{API}/users/me/config", user=alice)
    s.call("update another user's config", "PUT", f"{API}/users/{s.user_id('root')}/config",
           {"updatedFields": {"x": "1"}, "deletedFields": []}, user=alice)

    s.call("change password: wrong old password", "POST", f"{API}/users/{alice_id}/changepassword",
           {"oldPassword": "nope", "newPassword": "new-password"}, user=alice)
    s.call("change password: short new password", "POST", f"{API}/users/{alice_id}/changepassword",
           {"oldPassword": "test-password", "newPassword": "short"}, user=alice)
    s.call("change password", "POST", f"{API}/users/{alice_id}/changepassword",
           {"oldPassword": "test-password", "newPassword": "new-password"}, user=alice)
    s.call("login with the new password", "POST", f"{API}/login",
           {"type": "normal", "username": alice, "password": "new-password"})
    s.call("login with the old password", "POST", f"{API}/login",
           {"type": "normal", "username": alice, "password": "test-password"})

    s.call("logout", "POST", f"{API}/logout", user=alice)
    s.call("me after logout", "GET", f"{API}/users/me", user=alice)


def teams(s: Session) -> None:
    (alice,) = users(s, "teams", "alice")
    s.call("teams", "GET", f"{API}/teams", user=alice)
    s.call("team 0", "GET", f"{API}/teams/0", user=alice)
    s.call("some other team id", "GET", f"{API}/teams/anything", user=alice)
    s.call("team without session", "GET", f"{API}/teams/0")
    users_reply = s.call("team users", "GET", f"{API}/teams/0/users", user=alice, record=False)
    s.note("team users include me", any(u["username"] == alice for u in users_reply.json()))
    s.call("team users search", "GET", f"{API}/teams/0/users?search=teams-ali", user=alice)
    s.call("team users search without match", "GET", f"{API}/teams/0/users?search=zzzz-no-such", user=alice)
    s.call("team users by id", "POST", f"{API}/teams/0/users", [s.user_id(alice)], user=alice)
    s.call("team users by id: empty", "POST", f"{API}/teams/0/users", [], user=alice)



def boards(s: Session) -> None:
    alice, bob = users(s, "boards", "alice", "bob")
    s.call("create: no type", "POST", f"{API}/boards", {"teamId": "0", "title": "x"}, user=alice)
    s.call("create: bad type", "POST", f"{API}/boards", {"teamId": "0", "type": "X", "title": "x"}, user=alice)
    s.call("create: no team", "POST", f"{API}/boards", {"type": "P", "title": "x"}, user=alice)
    s.call("create: without session", "POST", f"{API}/boards", {"teamId": "0", "type": "P"})
    s.call("create: malformed body", "POST", f"{API}/boards", raw_body=b"{nope", user=alice)
    board = new_board(
        s, alice, "Roadmap", "P",
        description="plans", icon="🎯", showDescription=True,
        properties={"color": "blue"},
        cardProperties=[{"id": "prop1", "name": "Status", "type": "select", "options": []}],
    )
    open_board = new_board(s, alice, "Open board", "O")

    s.call("get", "GET", f"{API}/boards/{board}", user=alice)
    s.call("get: non-member, private", "GET", f"{API}/boards/{board}", user=bob)
    s.call("get: non-member, open", "GET", f"{API}/boards/{open_board}", user=bob)
    s.call("get: anonymous", "GET", f"{API}/boards/{board}")
    s.call("get: unknown board", "GET", f"{API}/boards/unknown-board", user=alice)
    s.call("list: member", "GET", f"{API}/teams/0/boards", user=alice)
    s.call("list: other user", "GET", f"{API}/teams/0/boards", user=bob)
    s.call("metadata", "GET", f"{API}/boards/{board}/metadata", user=alice)

    s.call("patch", "PATCH", f"{API}/boards/{board}", {
        "title": "Roadmap 2", "description": "new plans", "icon": "🚀", "showDescription": False,
        "updatedProperties": {"color": "red", "size": 3}, "deletedProperties": [],
        "updatedCardProperties": [{"id": "prop2", "name": "Owner", "type": "person", "options": []}],
        "deletedCardProperties": [],
    }, user=alice)
    s.call("patch: delete properties", "PATCH", f"{API}/boards/{board}",
           {"deletedProperties": ["size"], "deletedCardProperties": ["prop1"]}, user=alice)
    s.call("patch: non-member", "PATCH", f"{API}/boards/{board}", {"title": "hijack"}, user=bob)
    s.call("patch: bad type", "PATCH", f"{API}/boards/{board}", {"type": "Z"}, user=alice)
    s.call("patch: to open", "PATCH", f"{API}/boards/{board}", {"type": "O"}, user=alice)
    s.call("patch: back to private", "PATCH", f"{API}/boards/{board}", {"type": "P"}, user=alice)
    s.call("patch: minimum role", "PATCH", f"{API}/boards/{open_board}", {"minimumRole": "editor"}, user=alice)
    s.call("patch: unknown board", "PATCH", f"{API}/boards/unknown-board", {"title": "x"}, user=alice)
    s.call("patch: malformed body", "PATCH", f"{API}/boards/{board}", raw_body=b"[", user=alice)
    s.call("get after patches", "GET", f"{API}/boards/{board}", user=alice)

    new_blocks(s, alice, board, [block("c1", board, "card", "A card", icon="x")], name="insert a card")
    duplicate = s.call("duplicate", "POST", f"{API}/boards/{board}/duplicate", user=alice)
    s.call("duplicate as template", "POST", f"{API}/boards/{board}/duplicate?asTemplate=true", user=alice)
    s.call("duplicate: non-member", "POST", f"{API}/boards/{board}/duplicate", user=bob)
    s.call("duplicate: unknown board", "POST", f"{API}/boards/unknown-board/duplicate", user=alice)
    copy_id = duplicate.json()["boards"][0]["id"]
    s.call("blocks of the copy", "GET", f"{API}/boards/{copy_id}/blocks", user=alice)

    s.call("delete: non-member", "DELETE", f"{API}/boards/{board}", user=bob)
    s.call("delete", "DELETE", f"{API}/boards/{board}", user=alice)
    s.call("get deleted", "GET", f"{API}/boards/{board}", user=alice)
    s.call("delete again", "DELETE", f"{API}/boards/{board}", user=alice)
    s.call("undelete: other user", "POST", f"{API}/boards/{board}/undelete", user=bob)
    s.call("undelete", "POST", f"{API}/boards/{board}/undelete", user=alice)
    s.call("get undeleted", "GET", f"{API}/boards/{board}", user=alice)
    s.call("blocks after undelete", "GET", f"{API}/boards/{board}/blocks", user=alice)
    s.call("list at the end", "GET", f"{API}/teams/0/boards", user=alice)


def members(s: Session) -> None:
    alice, bob, carol = users(s, "members", "alice", "bob", "carol")
    board = new_board(s, alice, "Team board", "P")
    open_board = new_board(s, alice, "Open board", "O")
    bob_id, carol_id, alice_id = s.user_id(bob), s.user_id(carol), s.user_id(alice)

    s.call("list", "GET", f"{API}/boards/{board}/members", user=alice)
    s.call("list: non-member", "GET", f"{API}/boards/{board}/members", user=bob)
    s.call("add: non-member adds", "POST", f"{API}/boards/{board}/members",
           {"boardId": board, "userId": carol_id, "schemeEditor": True}, user=bob)
    s.call("add editor", "POST", f"{API}/boards/{board}/members",
           {"boardId": board, "userId": bob_id, "schemeEditor": True}, user=alice)
    s.call("add: no user", "POST", f"{API}/boards/{board}/members", {"boardId": board}, user=alice)
    s.call("add: unknown user", "POST", f"{API}/boards/{board}/members",
           {"boardId": board, "userId": "unknown-user", "schemeViewer": True}, user=alice)
    s.call("add: editor adds", "POST", f"{API}/boards/{board}/members",
           {"boardId": board, "userId": carol_id, "schemeViewer": True}, user=bob)
    s.call("add viewer", "POST", f"{API}/boards/{board}/members",
           {"boardId": board, "userId": carol_id, "schemeViewer": True}, user=alice)
    s.call("add: already a member", "POST", f"{API}/boards/{board}/members",
           {"boardId": board, "userId": carol_id, "schemeAdmin": True}, user=alice)
    s.call("list after adding", "GET", f"{API}/boards/{board}/members", user=bob)
    s.call("memberships of the editor", "GET", f"{API}/users/me/memberships", user=bob)

    s.call("update: to commenter", "PUT", f"{API}/boards/{board}/members/{carol_id}",
           {"boardId": board, "userId": carol_id, "schemeCommenter": True}, user=alice)
    s.call("update: by editor", "PUT", f"{API}/boards/{board}/members/{carol_id}",
           {"boardId": board, "userId": carol_id, "schemeAdmin": True}, user=bob)
    s.call("update: demote the last admin", "PUT", f"{API}/boards/{board}/members/{alice_id}",
           {"boardId": board, "userId": alice_id, "schemeEditor": True}, user=alice)
    s.call("update: unknown member", "PUT", f"{API}/boards/{board}/members/unknown-user",
           {"boardId": board, "userId": "unknown-user", "schemeEditor": True}, user=alice)

    s.call("delete: by editor", "DELETE", f"{API}/boards/{board}/members/{carol_id}", user=bob)
    s.call("delete: the last admin", "DELETE", f"{API}/boards/{board}/members/{alice_id}", user=alice)
    s.call("delete", "DELETE", f"{API}/boards/{board}/members/{carol_id}", user=alice)
    s.call("delete: not a member", "DELETE", f"{API}/boards/{board}/members/{carol_id}", user=alice)

    s.call("join: private board", "POST", f"{API}/boards/{board}/join", user=carol)
    s.call("join: open board", "POST", f"{API}/boards/{open_board}/join", user=carol)
    s.call("join: again", "POST", f"{API}/boards/{open_board}/join", user=carol)
    s.call("join: unknown board", "POST", f"{API}/boards/unknown-board/join", user=carol)
    s.call("leave", "POST", f"{API}/boards/{open_board}/leave", user=carol)
    s.call("leave: not a member", "POST", f"{API}/boards/{open_board}/leave", user=carol)
    s.call("leave: the last admin", "POST", f"{API}/boards/{open_board}/leave", user=alice)
    s.call("members at the end", "GET", f"{API}/boards/{board}/members", user=alice)


def blocks(s: Session) -> None:
    alice, bob, carol = users(s, "blocks", "alice", "bob", "carol")
    board = new_board(s, alice, "Blocks", "P")
    add_member(s, alice, board, bob, "commenter")
    path = f"{API}/boards/{board}/blocks"

    s.call("list: empty", "GET", path, user=alice)
    created = new_blocks(s, alice, board, [
        block("card1", board, "card", "First card", icon="x", contentOrder=["text1"]),
        block("text1", board, "text", "Some text", parent="card1"),
        block("view1", board, "view", "Board view", viewType="board"),
    ])
    ids = {b["title"]: b["id"] for b in created}
    card, text, view = ids["First card"], ids["Some text"], ids["Board view"]

    s.call("list", "GET", path, user=alice)
    s.call("list: by type", "GET", f"{path}?type=card", user=alice)
    s.call("list: by parent", "GET", f"{path}?parent_id={card}", user=alice)
    s.call("list: by parent and type", "GET", f"{path}?parent_id={card}&type=text", user=alice)
    s.call("list: unknown type", "GET", f"{path}?type=h1", user=alice)
    s.call("list: all", "GET", f"{path}?all=true", user=alice)
    s.call("list: one block", "GET", f"{path}?block_id={card}", user=alice)
    s.call("list: unknown block id", "GET", f"{path}?block_id=unknown-block", user=alice)
    s.call("list: paged", "GET", f"{path}?page=0&per_page=2", user=alice, record=False)
    s.call("list: commenter", "GET", path, user=bob)
    s.call("list: non-member", "GET", path, user=carol)
    s.call("list: anonymous", "GET", path)
    s.call("list: unknown board", "GET", f"{API}/boards/unknown-board/blocks", user=alice)

    s.call("insert: empty list", "POST", path, [], user=alice)
    s.call("insert: malformed body", "POST", path, raw_body=b"{", user=alice)
    s.call("insert: no type", "POST", path, [{"id": "x1", "boardId": board, "createAt": 1, "updateAt": 1}], user=alice)
    s.call("insert: no createAt", "POST", path,
           [{"id": "x1", "boardId": board, "type": "card", "updateAt": 1}], user=alice)
    s.call("insert: no updateAt", "POST", path,
           [{"id": "x1", "boardId": board, "type": "card", "createAt": 1}], user=alice)
    s.call("insert: other board id", "POST", path, [block("x1", "another-board", "card", "x")], user=alice)
    s.call("insert: long title", "POST", path, [block("x1", board, "card", "t" * 65_536)], user=alice)
    s.call("insert: non-member", "POST", path, [block("x1", board, "card", "x")], user=carol)
    s.call("insert: commenter inserts a card", "POST", path, [block("x1", board, "card", "x")], user=bob)
    s.call("insert: commenter inserts a comment", "POST", path,
           [block("cm1", board, "comment", "A comment", parent=card)], user=bob)
    s.call("insert: keeping ids", "POST", f"{path}?disable_notify=true",
           [block("x2", board, "card", "Quiet card")], user=alice)

    s.call("patch", "PATCH", f"{path}/{card}",
           {"title": "Renamed card", "updatedFields": {"icon": "y", "extra": [1, 2]}, "deletedFields": ["contentOrder"]},
           user=alice)
    s.call("patch: commenter", "PATCH", f"{path}/{card}", {"title": "hijack"}, user=bob)
    s.call("patch: non-member", "PATCH", f"{path}/{card}", {"title": "hijack"}, user=carol)
    s.call("patch: unknown block", "PATCH", f"{path}/unknown-block", {"title": "x"}, user=alice)
    s.call("patch: malformed body", "PATCH", f"{path}/{card}", raw_body=b"{", user=alice)
    s.call("patch: long title", "PATCH", f"{path}/{card}", {"title": "t" * 65_536}, user=alice)
    s.call("patch several", "PATCH", path,
           {"block_ids": [card, text], "block_patches": [{"title": "Card again"}, {"title": "Text again"}]}, user=alice)
    s.call("patch several: unknown block", "PATCH", path,
           {"block_ids": [card, "unknown-block"], "block_patches": [{"title": "a"}, {"title": "b"}]}, user=alice)
    s.call("patch several: commenter", "PATCH", path,
           {"block_ids": [card], "block_patches": [{"title": "hijack"}]}, user=bob)
    s.call("list after patches", "GET", path, user=alice)

    s.call("duplicate", "POST", f"{path}/{card}/duplicate", user=alice)
    s.call("duplicate as template", "POST", f"{path}/{card}/duplicate?asTemplate=true", user=alice)
    s.call("duplicate: commenter", "POST", f"{path}/{card}/duplicate", user=bob)
    s.call("duplicate: unknown block", "POST", f"{path}/unknown-block/duplicate", user=alice)

    s.call("delete: commenter", "DELETE", f"{path}/{view}", user=bob)
    s.call("delete: non-member", "DELETE", f"{path}/{view}", user=carol)
    s.call("delete", "DELETE", f"{path}/{view}", user=alice)
    s.call("delete: again", "DELETE", f"{path}/{view}", user=alice)
    s.call("delete: unknown block", "DELETE", f"{path}/unknown-block", user=alice)
    s.call("list: views after delete", "GET", f"{path}?type=view", user=alice)
    s.call("undelete: commenter", "POST", f"{path}/{view}/undelete", user=bob)
    s.call("undelete", "POST", f"{path}/{view}/undelete", user=alice)
    s.call("undelete: again", "POST", f"{path}/{view}/undelete", user=alice)
    s.call("undelete: unknown block", "POST", f"{path}/unknown-block/undelete", user=alice)
    s.call("list: views after undelete", "GET", f"{path}?type=view", user=alice)

    s.call("delete a card with content", "DELETE", f"{path}/{card}", user=alice)
    s.call("list: children of the deleted card", "GET", f"{path}?parent_id={card}", user=alice)


def boards_and_blocks(s: Session) -> None:
    alice, bob = users(s, "bab", "alice", "bob")
    path = f"{API}/boards-and-blocks"
    body = {
        "boards": [
            {"id": "board-a", "teamId": "0", "type": "P", "title": "Board A"},
            {"id": "board-b", "teamId": "0", "type": "O", "title": "Board B"},
        ],
        "blocks": [
            block("blk-a", "board-a", "card", "Card A"),
            block("blk-b", "board-b", "card", "Card B"),
        ],
    }
    created = s.call("create", "POST", path, body, user=alice).json()
    board_ids = {b["title"]: b["id"] for b in created["boards"]}
    block_ids = {b["title"]: b["id"] for b in created["blocks"]}
    board_a, board_b = board_ids["Board A"], board_ids["Board B"]
    card_a, card_b = block_ids["Card A"], block_ids["Card B"]

    s.call("create: no boards", "POST", path, {"boards": [], "blocks": []}, user=alice)
    s.call("create: block of another board", "POST", path,
           {"boards": [{"id": "b1", "teamId": "0", "type": "P"}], "blocks": [block("k1", "elsewhere", "card")]}, user=alice)
    s.call("create: boards of two teams", "POST", path,
           {"boards": [{"id": "b1", "teamId": "0", "type": "P"}, {"id": "b2", "teamId": "1", "type": "P"}], "blocks": []},
           user=alice)
    s.call("create: board without type", "POST", path,
           {"boards": [{"id": "b1", "teamId": "0"}], "blocks": []}, user=alice)
    s.call("create: malformed body", "POST", path, raw_body=b"{", user=alice)
    s.call("create: without session", "POST", path, body)

    s.call("patch", "PATCH", path, {
        "boardIDs": [board_a, board_b], "boardPatches": [{"title": "Board A2"}, {"description": "about B"}],
        "blockIDs": [card_a, card_b], "blockPatches": [{"title": "Card A2"}, {"updatedFields": {"icon": "z"}}],
    }, user=alice)
    s.call("patch: counts differ", "PATCH", path, {
        "boardIDs": [board_a], "boardPatches": [], "blockIDs": [card_a], "blockPatches": [{"title": "x"}],
    }, user=alice)
    s.call("patch: no boards", "PATCH", path,
           {"boardIDs": [], "boardPatches": [], "blockIDs": [card_a], "blockPatches": [{"title": "x"}]}, user=alice)
    s.call("patch: block of an unlisted board", "PATCH", path, {
        "boardIDs": [board_a], "boardPatches": [{"title": "x"}], "blockIDs": [card_b], "blockPatches": [{"title": "x"}],
    }, user=alice)
    s.call("patch: non-member", "PATCH", path, {
        "boardIDs": [board_a], "boardPatches": [{"title": "hijack"}], "blockIDs": [card_a], "blockPatches": [{"title": "x"}],
    }, user=bob)
    s.call("patch: unknown board", "PATCH", path, {
        "boardIDs": ["unknown-board"], "boardPatches": [{"title": "x"}], "blockIDs": [], "blockPatches": [],
    }, user=alice)
    s.call("board a after patch", "GET", f"{API}/boards/{board_a}", user=alice)
    s.call("blocks of b after patch", "GET", f"{API}/boards/{board_b}/blocks", user=alice)

    s.call("delete: non-member", "DELETE", path, {"boards": [board_a], "blocks": [card_a]}, user=bob)
    s.call("delete: no boards", "DELETE", path, {"boards": [], "blocks": [card_a]}, user=alice)
    s.call("delete: block of an unlisted board", "DELETE", path, {"boards": [board_a], "blocks": [card_b]}, user=alice)
    s.call("delete: unknown board", "DELETE", path, {"boards": ["unknown-board"], "blocks": []}, user=alice)
    s.call("delete", "DELETE", path, {"boards": [board_a, board_b], "blocks": [card_a, card_b]}, user=alice)
    s.call("board a after delete", "GET", f"{API}/boards/{board_a}", user=alice)


def cards(s: Session) -> None:
    alice, bob = users(s, "cards", "alice", "bob")
    board = new_board(s, alice, "Cards", "P")
    add_member(s, alice, board, bob, "viewer")
    path = f"{API}/boards/{board}/cards"

    first = s.call("create", "POST", path,
                   {"title": "Card one", "icon": "🃏", "contentOrder": ["a", "b"], "properties": {"p1": "v1"}},
                   user=alice).json()
    s.call("create: quiet", "POST", f"{path}?disable_notify=true", {"title": "Card two"}, user=alice)
    s.call("create: viewer", "POST", path, {"title": "x"}, user=bob)
    s.call("create: malformed body", "POST", path, raw_body=b"{", user=alice)
    s.call("create: unknown board", "POST", f"{API}/boards/unknown-board/cards", {"title": "x"}, user=alice)
    s.call("list", "GET", path, user=alice)
    s.call("list: viewer", "GET", path, user=bob)
    s.call("list: paged", "GET", f"{path}?page=0&per_page=1", user=alice, record=False)
    # The original writes the error and then, lacking a return, the list after it.
    bad_page = s.call("list: bad page", "GET", f"{path}?page=x", user=alice, record=False)
    text = bad_page.body if isinstance(bad_page.body, str) else jsonlib.dumps(bad_page.body, separators=(",", ":"))
    s.note("list: bad page", [bad_page.status, text[: text.index("}") + 1] if "}" in text else text,
                              "followed by more" if "}" in text and len(text) > text.index("}") + 1 else "alone"])
    s.call("list: unknown board", "GET", f"{API}/boards/unknown-board/cards", user=alice)
    card = first["id"]
    s.call("get", "GET", f"{API}/cards/{card}", user=alice)
    s.call("get: viewer", "GET", f"{API}/cards/{card}", user=bob)
    s.call("get: unknown card", "GET", f"{API}/cards/unknown-card", user=alice)
    s.call("patch", "PATCH", f"{API}/cards/{card}",
           {"title": "Card one, renamed", "icon": "🂡", "contentOrder": ["b", "a"], "updatedProperties": {"p2": "v2"}},
           user=alice)
    s.call("patch: viewer", "PATCH", f"{API}/cards/{card}", {"title": "hijack"}, user=bob)
    s.call("patch: unknown card", "PATCH", f"{API}/cards/unknown-card", {"title": "x"}, user=alice)
    s.call("patch: malformed body", "PATCH", f"{API}/cards/{card}", raw_body=b"{", user=alice)
    s.call("get after patch", "GET", f"{API}/cards/{card}", user=alice)

    # Content blocks of the card, and moving one.
    content = new_blocks(s, alice, board, [
        block("t1", board, "text", "one", parent=card),
        block("t2", board, "text", "two", parent=card),
        block("t3", board, "text", "three", parent=card),
    ], name="insert content")
    ids = {b["title"]: b["id"] for b in content}
    s.call("set content order", "PATCH", f"{API}/cards/{card}",
           {"contentOrder": [ids["one"], ids["two"], ids["three"]]}, user=alice)
    move = f"{API}/content-blocks"
    s.call("move after", "POST", f"{move}/{ids['one']}/moveto/after/{ids['three']}", user=alice)
    s.call("card after move", "GET", f"{API}/cards/{card}", user=alice)
    s.call("move before", "POST", f"{move}/{ids['three']}/moveto/before/{ids['two']}", user=alice)
    s.call("card after second move", "GET", f"{API}/cards/{card}", user=alice)
    s.call("move: bad direction", "POST", f"{move}/{ids['one']}/moveto/sideways/{ids['two']}", user=alice)
    s.call("move: unknown block", "POST", f"{move}/unknown-block/moveto/after/{ids['two']}", user=alice)
    s.call("move: unknown target", "POST", f"{move}/{ids['one']}/moveto/after/unknown-block", user=alice)
    s.call("move: viewer", "POST", f"{move}/{ids['one']}/moveto/after/{ids['two']}", user=bob)


def categories(s: Session) -> None:
    alice, bob = users(s, "cat", "alice", "bob")
    board = new_board(s, alice, "Filed board", "P")
    other = new_board(s, alice, "Other board", "P")
    alice_id = s.user_id(alice)
    path = f"{API}/teams/0/categories"

    s.call("list: default only", "GET", path, user=alice)
    s.call("list: other user", "GET", path, user=bob)
    created = s.call("create", "POST", path, {"name": "Projects", "userID": alice_id, "teamID": "0"}, user=alice).json()
    second = s.call("create second", "POST", path,
                    {"name": "Archive", "userID": alice_id, "teamID": "0", "collapsed": True}, user=alice).json()
    s.call("create: no name", "POST", path, {"name": "", "userID": alice_id, "teamID": "0"}, user=alice)
    s.call("create: for another user", "POST", path, {"name": "x", "userID": s.user_id(bob), "teamID": "0"}, user=alice)
    s.call("create: for another team", "POST", path, {"name": "x", "userID": alice_id, "teamID": "1"}, user=alice)
    s.call("create: bad type", "POST", path, {"name": "x", "userID": alice_id, "teamID": "0", "type": "weird"}, user=alice)
    s.call("create: malformed body", "POST", path, raw_body=b"{", user=alice)
    category, archive = created["id"], second["id"]
    listing = s.call("list", "GET", path, user=alice).json()
    default = next(c["id"] for c in listing if c["name"] == "Boards")

    s.call("update", "PUT", f"{path}/{category}",
           {**created, "name": "Projects 2", "collapsed": True}, user=alice)
    s.call("update: id mismatch", "PUT", f"{path}/{category}", {**created, "id": archive}, user=alice)
    s.call("update: other user", "PUT", f"{path}/{category}", {**created, "name": "hijack"}, user=bob)
    s.call("update: unknown category", "PUT", f"{path}/unknown-category",
           {**created, "id": "unknown-category"}, user=alice)
    s.call("update: system category", "PUT", f"{path}/{default}",
           {"id": default, "name": "Renamed", "userID": alice_id, "teamID": "0", "type": "system"}, user=alice)

    s.call("file a board", "POST", f"{path}/{category}/boards/{board}", user=alice)
    s.call("file the second board", "POST", f"{path}/{category}/boards/{other}", user=alice)
    s.call("list after filing", "GET", path, user=alice)
    s.call("hide", "PUT", f"{path}/{category}/boards/{board}/hide", user=alice)
    s.call("hide: other user", "PUT", f"{path}/{category}/boards/{board}/hide", user=bob)
    s.call("list after hiding", "GET", path, user=alice)
    s.call("unhide", "PUT", f"{path}/{category}/boards/{board}/unhide", user=alice)

    s.call("reorder boards", "PUT", f"{path}/{category}/boards/reorder", [other, board], user=alice)
    s.call("reorder boards: missing one", "PUT", f"{path}/{category}/boards/reorder", [other], user=alice)
    s.call("reorder boards: unknown board", "PUT", f"{path}/{category}/boards/reorder", [other, "unknown"], user=alice)
    s.call("reorder boards: other user", "PUT", f"{path}/{category}/boards/reorder", [other, board], user=bob)
    s.call("reorder categories", "PUT", f"{path}/reorder", [archive, category, default], user=alice)
    s.call("reorder categories: missing one", "PUT", f"{path}/reorder", [archive, category], user=alice)
    s.call("reorder categories: unknown id", "PUT", f"{path}/reorder", [archive, category, "unknown"], user=alice)
    s.call("list after reorder", "GET", path, user=alice)

    s.call("delete: other user", "DELETE", f"{path}/{category}", user=bob)
    s.call("delete: system category", "DELETE", f"{path}/{default}", user=alice)
    s.call("delete: unknown category", "DELETE", f"{path}/unknown-category", user=alice)
    s.call("delete", "DELETE", f"{path}/{category}", user=alice)
    s.call("delete: again", "DELETE", f"{path}/{category}", user=alice)
    s.call("update: deleted category", "PUT", f"{path}/{category}", {**created, "name": "back"}, user=alice)
    s.call("list at the end", "GET", path, user=alice)

    # Last, because in the original this request changes the other user's category.
    s.call("file a board: other user's category", "POST", f"{path}/{archive}/boards/{other}", user=bob)
    s.call("list after the attempt on another user's category", "GET", path, user=alice)
    # In the original this moves the board into a category that does not exist.
    s.call("file a board: unknown category", "POST", f"{path}/unknown-category/boards/{other}", user=alice)
    s.call("list after filing into an unknown category", "GET", path, user=alice)


def sharing(s: Session) -> None:
    alice, bob = users(s, "share", "alice", "bob")
    board = new_board(s, alice, "Shared board", "P")
    add_member(s, alice, board, bob, "editor")
    created = new_blocks(s, alice, board, [block("c1", board, "card", "Visible card")])
    path = f"{API}/boards/{board}/sharing"

    s.call("get: none yet", "GET", path, user=alice)
    s.call("post: editor", "POST", path, {"id": board, "enabled": True, "token": "tok-editor"}, user=bob)
    s.call("post", "POST", path, {"id": board, "enabled": True, "token": "share-token-1"}, user=alice)
    s.call("post: malformed body", "POST", path, raw_body=b"{", user=alice)
    s.call("get", "GET", path, user=alice, record=False)
    s.call("get: editor", "GET", path, user=bob, record=False)
    s.call("get: unknown board", "GET", f"{API}/boards/unknown-board/sharing", user=alice)

    s.call("board with the read token", "GET", f"{API}/boards/{board}?read_token=share-token-1")
    s.call("board with a wrong read token", "GET", f"{API}/boards/{board}?read_token=wrong")
    s.call("blocks with the read token", "GET", f"{API}/boards/{board}/blocks?read_token=share-token-1")
    s.call("blocks with a wrong read token", "GET", f"{API}/boards/{board}/blocks?read_token=wrong")
    s.call("members with the read token", "GET", f"{API}/boards/{board}/members?read_token=share-token-1")
    s.call("insert with the read token", "POST", f"{API}/boards/{board}/blocks?read_token=share-token-1",
           [block("x1", board, "card", "x")])
    s.call("patch with the read token", "PATCH",
           f"{API}/boards/{board}/blocks/{created[0]['id']}?read_token=share-token-1", {"title": "hijack"})

    s.call("disable", "POST", path, {"id": board, "enabled": False, "token": "share-token-1"}, user=alice)
    s.call("board with the token of a disabled share", "GET", f"{API}/boards/{board}?read_token=share-token-1")


def subscriptions(s: Session) -> None:
    alice, bob = users(s, "subs", "alice", "bob")
    board = new_board(s, alice, "Subscribed board", "P")
    created = new_blocks(s, alice, board, [block("c1", board, "card", "Watched card")])
    card = created[0]["id"]
    alice_id, bob_id = s.user_id(alice), s.user_id(bob)
    path = f"{API}/subscriptions"

    s.call("list: none", "GET", f"{path}/{alice_id}", user=alice)
    s.call("create", "POST", path,
           {"blockType": "card", "blockId": card, "subscriberType": "user", "subscriberId": alice_id}, user=alice)
    s.call("create: again", "POST", path,
           {"blockType": "card", "blockId": card, "subscriberType": "user", "subscriberId": alice_id}, user=alice)
    s.call("create: for another user", "POST", path,
           {"blockType": "card", "blockId": card, "subscriberType": "user", "subscriberId": bob_id}, user=alice)
    s.call("create: no block", "POST", path,
           {"blockType": "card", "blockId": "", "subscriberType": "user", "subscriberId": alice_id}, user=alice)
    s.call("create: bad subscriber type", "POST", path,
           {"blockType": "card", "blockId": card, "subscriberType": "robot", "subscriberId": alice_id}, user=alice)
    s.call("create: malformed body", "POST", path, raw_body=b"{", user=alice)
    s.call("list", "GET", f"{path}/{alice_id}", user=alice)
    s.call("list: another user's", "GET", f"{path}/{alice_id}", user=bob)
    s.call("delete: another user's", "DELETE", f"{path}/{card}/{alice_id}", user=bob)
    s.call("delete", "DELETE", f"{path}/{card}/{alice_id}", user=alice)
    s.call("delete: again", "DELETE", f"{path}/{card}/{alice_id}", user=alice)
    s.call("list at the end", "GET", f"{path}/{alice_id}", user=alice)


def search(s: Session) -> None:
    alice, bob = users(s, "search", "alice", "bob")
    new_board(s, alice, "Quarterly zebra plan", "P")
    new_board(s, alice, "Open zebra notes", "O")
    new_board(s, bob, "Private zebra diary", "P")
    s.call("team search", "GET", f"{API}/teams/0/boards/search?q=zebra", user=alice)
    s.call("team search: other user", "GET", f"{API}/teams/0/boards/search?q=zebra", user=bob)
    s.call("team search: no match", "GET", f"{API}/teams/0/boards/search?q=giraffe", user=alice)
    s.call("team search: empty term", "GET", f"{API}/teams/0/boards/search?q=", user=alice)
    s.call("team search: no term", "GET", f"{API}/teams/0/boards/search", user=alice)
    s.call("team search: case", "GET", f"{API}/teams/0/boards/search?q=ZEBRA", user=alice)
    s.call("team search: by field", "GET", f"{API}/teams/0/boards/search?q=zebra&field=title", user=alice)
    s.call("team search: bad field", "GET", f"{API}/teams/0/boards/search?q=zebra&field=nope", user=alice)
    s.call("team search: anonymous", "GET", f"{API}/teams/0/boards/search?q=zebra")
    s.call("all search", "GET", f"{API}/boards/search?q=zebra", user=alice)
    s.call("all search: empty term", "GET", f"{API}/boards/search?q=", user=alice)
    s.call("linkable search", "GET", f"{API}/teams/0/boards/search/linkable?q=zebra", user=alice)
    s.call("channels", "GET", f"{API}/teams/0/channels?search=x", user=alice)
    s.call("channel", "GET", f"{API}/teams/0/channels/some-channel", user=alice)
    s.call("statistics", "GET", f"{API}/statistics", user=alice)
    s.call("compliance boards", "GET", f"{API}/admin/boards", user=alice)
    s.call("compliance boards history", "GET", f"{API}/admin/boards_history?modified_since=1", user=alice)
    s.call("compliance blocks history", "GET", f"{API}/admin/blocks_history?modified_since=1", user=alice)


def templates(s: Session) -> None:
    (alice,) = users(s, "tpl", "alice")
    listing = s.call("templates", "GET", f"{API}/teams/0/templates", user=alice, record=False).json()
    s.note("template titles", sorted(b["title"] for b in listing))
    s.note("template versions", sorted({b["templateVersion"] for b in listing}))
    s.note("template members", sorted(listing[0].keys()))
    welcome = next(b for b in listing if b["title"] == "Welcome to Boards!")
    blocks_reply = s.call("welcome blocks", "GET", f"{API}/boards/{welcome['id']}/blocks", user=alice, record=False)
    counts: dict[str, int] = {}
    for item in blocks_reply.json():
        counts[item["type"]] = counts.get(item["type"], 0) + 1
    s.note("welcome template block counts", counts)
    s.call("templates: anonymous", "GET", f"{API}/teams/0/templates")
    custom = new_board(s, alice, "My template", "P", isTemplate=True)
    s.call("templates include mine", "GET", f"{API}/teams/0/templates", user=alice, record=False)
    s.call("my template", "GET", f"{API}/boards/{custom}", user=alice)
    onboard = s.call("onboard", "POST", f"{API}/teams/0/onboard", user=alice).json()
    s.call("onboarding board", "GET", f"{API}/boards/{onboard['boardID']}", user=alice)
    tour = s.call("onboarding blocks", "GET", f"{API}/boards/{onboard['boardID']}/blocks", user=alice, record=False)
    s.note("onboarding board block count", len(tour.json()))
    s.call("onboard again", "POST", f"{API}/teams/0/onboard", user=alice, record=False)
    s.call("categories after onboarding", "GET", f"{API}/teams/0/categories", user=alice)
    s.call("config after onboarding", "GET", f"{API}/users/me/config", user=alice)


PNG = bytes.fromhex(
    "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c489"
    "0000000d49444154789c6360000002000001e221bc330000000049454e44ae426082"
)


def files(s: Session) -> None:
    alice, bob = users(s, "files", "alice", "bob")
    board = new_board(s, alice, "Board with files", "P")
    upload = f"{API}/teams/0/{board}/files"
    body, content_type = multipart("file", "pixel.png", PNG, "image/png")
    reply = s.call("upload", "POST", upload, raw_body=body, content_type=content_type, user=alice)
    file_id = reply.json()["fileId"]
    s.note("uploaded name has the extension", file_id.endswith(".png"))
    body, content_type = multipart("file", "photo.JPEG", PNG, "image/jpeg")
    s.call("upload: jpeg extension", "POST", upload, raw_body=body, content_type=content_type, user=alice)
    body, content_type = multipart("file", "notes.txt", b"hello", "text/plain")
    text_id = s.call("upload: text", "POST", upload, raw_body=body, content_type=content_type, user=alice).json()["fileId"]
    body, content_type = multipart("other", "pixel.png", PNG, "image/png")
    s.call("upload: wrong field", "POST", upload, raw_body=body, content_type=content_type, user=alice)
    body, content_type = multipart("file", "pixel.png", PNG, "image/png")
    s.call("upload: non-member", "POST", upload, raw_body=body, content_type=content_type, user=bob)
    s.call("upload: anonymous", "POST", upload, raw_body=body, content_type=content_type)
    s.call("upload: unknown board", "POST", f"{API}/teams/0/unknown-board/files",
           raw_body=body, content_type=content_type, user=alice)
    s.call("upload: not multipart", "POST", upload, {"file": "x"}, user=alice)

    download = f"{API}/files/teams/0/{board}/{file_id}"
    headers = ("Content-Type", "Content-Disposition", "Cache-Control", "X-Content-Type-Options",
               "Content-Security-Policy", "Accept-Ranges", "Content-Length")
    s.call("download", "GET", download, user=alice, keep_headers=headers)
    s.call("download: text", "GET", f"{API}/files/teams/0/{board}/{text_id}", user=alice, keep_headers=headers)
    s.call("download: range", "GET", download, user=alice, extra_headers={"Range": "bytes=0-7"},
           keep_headers=("Content-Range", "Content-Length"))
    s.call("download: range past the end", "GET", download, user=alice, extra_headers={"Range": "bytes=5000-"},
           keep_headers=("Content-Range",))
    s.call("download: non-member", "GET", download, user=bob)
    s.call("download: anonymous", "GET", download)
    s.call("download: unknown file", "GET", f"{API}/files/teams/0/{board}/7unknownfile.png", user=alice)
    s.call("download: traversal", "GET", f"{API}/files/teams/0/{board}/..%2F..%2Fconfig.json", user=alice)
    s.call("info", "GET", f"{download}/info", user=alice)
    s.call("info: non-member", "GET", f"{download}/info", user=bob)
    s.call("info: unknown file", "GET", f"{API}/files/teams/0/{board}/7unknownfile.png/info", user=alice)

    # A shared board serves its files to holders of the read token.
    s.call("share", "POST", f"{API}/boards/{board}/sharing", {"id": board, "enabled": True, "token": "file-token"},
           user=alice)
    s.call("download with the read token", "GET", f"{download}?read_token=file-token", keep_headers=("Content-Type",))
    s.call("download with a wrong read token", "GET", f"{download}?read_token=wrong")

    # An image block that points at the file, then deleting it.
    created = new_blocks(s, alice, board, [
        block("card1", board, "card", "Card with image"),
        block("img1", board, "image", "", parent="card1", fileId=file_id),
    ], name="insert an image block")
    image = next(b for b in created if b["type"] == "image")
    s.call("delete the image block", "DELETE", f"{API}/boards/{board}/blocks/{image['id']}", user=alice)
    s.call("blocks after deleting the image", "GET", f"{API}/boards/{board}/blocks", user=alice)
    s.call("duplicate the board with files", "POST", f"{API}/boards/{board}/duplicate", user=alice, record=False)


def archive(s: Session) -> None:
    import io
    import json as jsonlib
    import zipfile

    alice, bob = users(s, "arch", "alice", "bob")
    board = new_board(s, alice, "Exported board", "P", description="to export",
                      cardProperties=[{"id": "p1", "name": "Status", "type": "select", "options": []}])
    body, content_type = multipart("file", "pixel.png", PNG, "image/png")
    file_id = s.call("upload", "POST", f"{API}/teams/0/{board}/files", raw_body=body, content_type=content_type,
                     user=alice, record=False).json()["fileId"]
    new_blocks(s, alice, board, [
        block("card1", board, "card", "Exported card", icon="x"),
        block("text1", board, "text", "Exported text", parent="card1"),
        block("img1", board, "image", "", parent="card1", fileId=file_id),
        block("view1", board, "view", "Exported view", viewType="table"),
    ], name="insert content")

    def summary(raw: bytes) -> dict:
        with zipfile.ZipFile(io.BytesIO(raw)) as bundle:
            names = bundle.namelist()
            lines = []
            for name in names:
                if name.endswith(".jsonl"):
                    for line in bundle.read(name).decode().splitlines():
                        if line.strip():
                            lines.append(jsonlib.loads(line))
            version = jsonlib.loads(bundle.read("version.json")) if "version.json" in names else None
        return {"names": sorted(names), "version": version, "lines": lines}

    exported = s.call("export", "GET", f"{API}/boards/{board}/archive/export", user=alice, record=False,
                      keep_headers=("Content-Type", "Content-Disposition"))
    s.note("export status and type", [exported.status, exported.headers.get("Content-Type", "")])
    assert isinstance(exported.body, bytes), f"export answered {exported.body!r}"
    s.note("export content", summary(exported.body))
    s.call("export: non-member", "GET", f"{API}/boards/{board}/archive/export", user=bob)
    s.call("export: anonymous", "GET", f"{API}/boards/{board}/archive/export")
    s.call("export: unknown board", "GET", f"{API}/boards/unknown-board/archive/export", user=alice)
    team_export = s.call("team export", "GET", f"{API}/teams/0/archive/export", user=alice, record=False)
    s.note("team export status and type", [team_export.status, team_export.headers.get("Content-Type", "")])
    if isinstance(team_export.body, bytes) and team_export.status == 200:
        s.note("team export entries", len(summary(team_export.body)["names"]))

    body, content_type = multipart("file", "export.boardarchive", exported.body)
    s.call("import", "POST", f"{API}/teams/0/archive/import", raw_body=body, content_type=content_type, user=bob)
    s.call("import: anonymous", "POST", f"{API}/teams/0/archive/import", raw_body=body, content_type=content_type)
    body, content_type = multipart("file", "broken.boardarchive", b"this is not an archive")
    s.call("import: not an archive", "POST", f"{API}/teams/0/archive/import", raw_body=body, content_type=content_type,
           user=bob)
    body, content_type = multipart("other", "export.boardarchive", exported.body)
    s.call("import: wrong field", "POST", f"{API}/teams/0/archive/import", raw_body=body, content_type=content_type,
           user=bob)
    imported = s.call("boards after import", "GET", f"{API}/teams/0/boards", user=bob).json()
    copy = next(b for b in imported if b["title"] == "Exported board")
    s.call("imported blocks", "GET", f"{API}/boards/{copy['id']}/blocks", user=bob)
    s.call("imported members", "GET", f"{API}/boards/{copy['id']}/members", user=bob)
    image = next(b for b in s.call("imported image block", "GET", f"{API}/boards/{copy['id']}/blocks?type=image",
                                   user=bob, record=False).json())
    s.call("imported file", "GET", f"{API}/files/teams/0/{copy['id']}/{image['fields']['fileId']}", user=bob)

    # The original's older, line-based archive format.
    legacy_lines = [
        {"version": 1, "date": 1},
        {"type": "block", "data": {"id": "legacy-board", "type": "board", "title": "Legacy board", "schema": 1,
                                   "fields": {"icon": "x", "cardProperties": []}, "createAt": 1, "updateAt": 1,
                                   "deleteAt": 0, "parentId": "", "rootId": "legacy-board"}},
        {"type": "block", "data": {"id": "legacy-card", "type": "card", "title": "Legacy card", "schema": 1,
                                   "fields": {}, "createAt": 1, "updateAt": 1, "deleteAt": 0,
                                   "parentId": "legacy-board", "rootId": "legacy-board"}},
    ]
    legacy = "\n".join(jsonlib.dumps(line) for line in legacy_lines).encode()
    body, content_type = multipart("file", "legacy.boardarchive", legacy)
    s.call("import: legacy format", "POST", f"{API}/teams/0/archive/import", raw_body=body, content_type=content_type,
           user=bob)
    titles = sorted(b["title"] for b in s.call("boards after legacy import", "GET", f"{API}/teams/0/boards", user=bob,
                                                record=False).json())
    s.note("board titles after legacy import", titles)


SCENARIOS = [
    system, auth, teams, boards, members, blocks, boards_and_blocks, cards, categories, sharing, subscriptions,
    search, templates, files, archive,
]
