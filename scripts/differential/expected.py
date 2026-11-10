"""Differences between the original server and the port that are intended.

Keys are (scenario, step name); values say why the answers differ and where the
difference is documented. A difference that is not listed here fails the run. For the
permission matrix the scenario is "permissions" and the step is
"<private|open>: <route>: <role>".
"""

SERVER = "crates/server/PORTING.md"

_PREFERENCES = "preferences are filed under the port's own category name (root PORTING.md, decision 1)"
_DECODER = f"same status; the text of a JSON decoding error is the decoder's ({SERVER}, difference 36)"
_EMPTY = f"the port sends [] and {{}} where the original sends null for unset collections ({SERVER}, differences 29 and 35)"

EXPECTED: dict[tuple[str, str], str] = {
    ("auth", "update my config"): _PREFERENCES,
    ("auth", "delete a config entry"): _PREFERENCES,
    ("auth", "my config afterwards"): _PREFERENCES,
    ("boards", "create: malformed body"): _DECODER,
    ("boards", "patch: malformed body"): _DECODER,
    ("blocks", "undelete: unknown block"):
        f"the original crashes (500); the port answers 404 ({SERVER}, difference 25)",
    ("boards_and_blocks", "create"): _EMPTY,
    ("boards_and_blocks", "patch"): _EMPTY,
    ("boards_and_blocks", "board a after patch"): _EMPTY,
    ("boards_and_blocks", "create: board without type"):
        f"same status; the port refuses the unknown board type first ({SERVER}, difference 33)",
    ("categories", "file a board: other user's category"):
        f"security fix: the original writes into the other user's category and answers 500; the port refuses with 403 ({SERVER}, security fixes)",
    ("categories", "list after the attempt on another user's category"):
        f"security fix: in the original the other user's category now holds the attacker's board ({SERVER}, security fixes)",
    ("categories", "list after filing into an unknown category"):
        f"the original moves the board into the nonexistent category, so it leaves the sidebar; the port writes nothing ({SERVER}, difference 30)",
    ("permissions", "open: members: add admin: editor"):
        f"security fix: an editor of an open board can no longer add an administrator ({SERVER}, security fixes)",
    ("cards", "list: bad page"):
        f"the original writes the 400 and then the list after it; the port answers the 400 alone ({SERVER}, difference 40)",
    ("permissions", "private: subscriptions: create: outsider"):
        f"security fix: subscribing needs read access to the block's board ({SERVER}, security fixes)",
    ("permissions", "open: subscriptions: create: outsider"):
        f"security fix: subscribing needs read access to the block's board ({SERVER}, security fixes)",
    ("cards", "create: malformed body"): _DECODER,
    ("cards", "patch: malformed body"): _DECODER,
    ("permissions", "private: subscriptions: delete: outsider"):
        f"follows from the security fix: the outsider could not subscribe, so there is nothing to end ({SERVER}, security fixes)",
    ("permissions", "open: subscriptions: delete: outsider"):
        f"follows from the security fix: the outsider could not subscribe, so there is nothing to end ({SERVER}, security fixes)",
    ("permissions", "private: blocks: delete another's comment: editor"):
        f"decided fix: a comment is deleted by its author or an admin ({SERVER}, security fixes)",
    ("permissions", "private: cards: get: viewer"):
        f"decided fix: reading one card takes the permission to view the board ({SERVER}, difference 45)",
    ("permissions", "private: cards: get: commenter"):
        f"decided fix: reading one card takes the permission to view the board ({SERVER}, difference 45)",
    ("permissions", "open: blocks: delete another's comment: editor"):
        f"decided fix: a comment is deleted by its author or an admin ({SERVER}, security fixes)",
    ("permissions", "open: cards: get: viewer"):
        f"decided fix: reading one card takes the permission to view the board ({SERVER}, difference 45)",
    ("permissions", "open: cards: get: commenter"):
        f"decided fix: reading one card takes the permission to view the board ({SERVER}, difference 45)",
    ("cards", "get: viewer"):
        f"decided fix: reading one card takes the permission to view the board ({SERVER}, difference 45)",
    ("templates", "config after onboarding"): _PREFERENCES,
    ("files", "download: traversal"):
        f"the original's router cleans the path and ends at the web app; the port refuses the name with a 500 ({SERVER}, difference 65)",
    ("files", "delete the image block"):
        f"the original cannot delete an image block: its SQL names a table that does not exist (500); the port deletes it ({SERVER}, verification notes)",
    ("files", "blocks after deleting the image"):
        f"follows from the difference above: in the original the image block is still there ({SERVER}, verification notes)",
}
