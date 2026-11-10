"""Differential testing harness: run the same request sequence against two servers and
compare what they answer.

A scenario is a Python function that drives one server through a `Session`. Every request
it makes is recorded as a step: name, status code, and the response body after
normalization. Two recordings of the same scenario, one per server, are then compared step
by step.

Normalization replaces what legitimately differs between two runs:

* identifiers (26 base32 characters, with or without a kind prefix) become `<id:N>`, numbered in order
  of first appearance, also when embedded in a longer string such as an error message;
* millisecond timestamps become `<ts>`;
* values of members named like secrets (`token`, `signupToken`, ...) become `<secret>`;
* lists of objects are sorted, because most list endpoints have no defined order.
"""

from __future__ import annotations

import http.client
import json
import re
import urllib.error
import urllib.request
import uuid
from dataclasses import dataclass, field
from typing import Any, Callable

# A kind prefix plus 26 characters, or the bare 26 characters (file-info identifiers).
ID_PATTERN = re.compile(r"(?<![a-z0-9])[7tbcvsukai]?[ybndrfg8ejkmcpqxot1uwisza345h769]{26}(?![a-z0-9])")
SECRET_KEYS = {"token", "signupToken", "signup_token"}
# Anything after 2017 in milliseconds; smaller numbers are counts, sort orders and so on.
TIMESTAMP_FLOOR = 1_500_000_000_000

CSRF_HEADER = {"X-Requested-With": "XMLHttpRequest"}


@dataclass
class Step:
    """One recorded request."""

    name: str
    request: str
    status: int
    body: Any
    content_type: str = ""
    headers: dict[str, str] = field(default_factory=dict)


@dataclass
class Response:
    """What a request returned, before normalization."""

    status: int
    body: Any  # parsed JSON, or text, or bytes
    headers: dict[str, str]

    def json(self) -> Any:
        if isinstance(self.body, (dict, list)):
            return self.body
        raise AssertionError(f"expected a JSON body, got {self.body!r} (status {self.status})")


class Session:
    """Drives one server and records what it answers."""

    def __init__(self, base_url: str, label: str):
        self.base_url = base_url.rstrip("/")
        self.label = label
        self.steps: list[Step] = []
        self.tokens: dict[str, str] = {}
        self._ids: dict[str, str] = {}
        self._sequence = 0

    # -- requests ---------------------------------------------------------------------

    def call(
        self,
        name: str,
        method: str,
        path: str,
        body: Any = None,
        *,
        user: str | None = None,
        token: str | None = None,
        csrf: bool = True,
        raw_body: bytes | None = None,
        content_type: str | None = None,
        record: bool = True,
        keep_headers: tuple[str, ...] = (),
        extra_headers: dict[str, str] | None = None,
    ) -> Response:
        """Sends a request. `path` is relative to the server root, e.g. `/api/v2/boards`."""
        headers: dict[str, str] = {}
        if csrf:
            headers.update(CSRF_HEADER)
        if user is not None:
            token = self.tokens[user]
        if token is not None:
            headers["Authorization"] = f"Bearer {token}"
        data = raw_body
        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        if content_type is not None:
            headers["Content-Type"] = content_type
        if extra_headers:
            headers.update(extra_headers)

        request = urllib.request.Request(self.base_url + path, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=30) as reply:
                status, raw, reply_headers = reply.status, reply.read(), _headers(reply.headers)
        except urllib.error.HTTPError as error:
            try:
                raw = error.read()
            except http.client.IncompleteRead as incomplete:
                # A body shorter than its Content-Length; keep what arrived.
                raw = incomplete.partial
            status, reply_headers = error.code, _headers(error.headers)

        reply_type = reply_headers.get("Content-Type", "")
        parsed: Any
        if "json" in reply_type:
            try:
                parsed = json.loads(raw)
            except json.JSONDecodeError:
                parsed = raw.decode("utf-8", "replace")
        elif reply_type.startswith("text/"):
            parsed = raw.decode("utf-8", "replace")
        else:
            parsed = raw

        response = Response(status, parsed, reply_headers)
        if record:
            kept = {key: reply_headers.get(key, "") for key in keep_headers}
            who = user or ("token" if token else "anonymous")
            self.steps.append(
                Step(
                    name=name,
                    request=f"{method} {self._normalize_text(path)} as {who}",
                    status=status,
                    body=self._normalize_body(parsed, reply_type),
                    content_type=reply_type.split(";")[0],
                    headers={key: self._normalize_text(value) for key, value in kept.items()},
                )
            )
        return response

    def note(self, name: str, value: Any) -> None:
        """Records a derived fact (not a request) as a step, e.g. a websocket message."""
        self.steps.append(Step(name=name, request="(observation)", status=0, body=self._normalize(value)))

    # -- users ------------------------------------------------------------------------

    def unique(self, prefix: str) -> str:
        self._sequence += 1
        return f"{prefix}{self._sequence}"

    def register(self, user: str, *, signup_token: str = "", record: bool = False) -> None:
        """Registers `user` (password `test-password`) and logs in, remembering the token."""
        body = {"username": user, "email": f"{user}@example.com", "password": "test-password"}
        if signup_token:
            body["token"] = signup_token
        reply = self.call(f"register {user}", "POST", "/api/v2/register", body, record=record)
        assert reply.status == 200, f"[{self.label}] register {user}: {reply.status} {reply.body}"
        self.login(user, record=record)

    def login(self, user: str, *, record: bool = False) -> None:
        body = {"type": "normal", "username": user, "password": "test-password"}
        reply = self.call(f"login {user}", "POST", "/api/v2/login", body, record=record)
        assert reply.status == 200, f"[{self.label}] login {user}: {reply.status} {reply.body}"
        self.tokens[user] = reply.json()["token"]

    def signup_token(self, user: str) -> str:
        reply = self.call("get team", "GET", "/api/v2/teams/0", user=user, record=False)
        return reply.json()["signupToken"]

    def user_id(self, user: str) -> str:
        reply = self.call("get me", "GET", "/api/v2/users/me", user=user, record=False)
        return reply.json()["id"]

    # -- normalization ----------------------------------------------------------------

    def _placeholder(self, identifier: str) -> str:
        if identifier not in self._ids:
            self._ids[identifier] = f"<id:{len(self._ids) + 1}>"
        return self._ids[identifier]

    def _normalize_text(self, text: str) -> str:
        return ID_PATTERN.sub(lambda match: self._placeholder(match.group(0)), text)

    def _normalize_body(self, body: Any, content_type: str) -> Any:
        if isinstance(body, bytes):
            return f"<{len(body)} bytes>"
        if isinstance(body, str) and "html" in content_type:
            return "<html>" if "<html" in body.lower() else self._normalize_text(body)
        return self._normalize(body)

    def _normalize(self, value: Any, key: str = "") -> Any:
        if isinstance(value, dict):
            return {k: self._normalize(v, k) for k, v in sorted(value.items())}
        if isinstance(value, list):
            if value and all(isinstance(item, dict) for item in value):
                # Sort by a rendering in which every identifier looks the same, so that the
                # order does not depend on the random identifiers themselves; only then
                # hand out the numbered placeholders.
                value = sorted(value, key=_blind_key)
            return [self._normalize(item, key) for item in value]
        if isinstance(value, str):
            if key in SECRET_KEYS and value:
                return "<secret>"
            return self._normalize_text(value)
        if isinstance(value, bool):
            return value
        if isinstance(value, int) and value >= TIMESTAMP_FLOOR:
            return "<ts>"
        return value


def _headers(message: Any) -> dict[str, str]:
    """Header names are case-insensitive; spell them one way (`Content-Type`)."""
    return {"-".join(part.capitalize() for part in key.split("-")): value for key, value in message.items()}


def _blind_key(value: Any) -> str:
    def blind(item: Any, key: str = "") -> Any:
        if isinstance(item, dict):
            return {k: blind(v, k) for k, v in sorted(item.items())}
        if isinstance(item, list):
            blinded = [blind(v, key) for v in item]
            if blinded and all(isinstance(v, dict) for v in blinded):
                blinded.sort(key=lambda v: json.dumps(v, sort_keys=True))
            return blinded
        if isinstance(item, str):
            return "<secret>" if key in SECRET_KEYS and item else ID_PATTERN.sub("<id>", item)
        if isinstance(item, int) and not isinstance(item, bool) and item >= TIMESTAMP_FLOOR:
            return "<ts>"
        return item

    return json.dumps(blind(value), sort_keys=True, ensure_ascii=False)


Scenario = Callable[[Session], None]


@dataclass
class Difference:
    scenario: str
    step: str
    request: str
    detail: str


def run_scenario(scenario: Scenario, base_url: str, label: str) -> list[Step]:
    session = Session(base_url, label)
    scenario(session)
    return session.steps


def compare(scenario_name: str, left: list[Step], right: list[Step], labels: tuple[str, str]) -> list[Difference]:
    """Returns the differences between two recordings of one scenario."""
    differences: list[Difference] = []
    if len(left) != len(right):
        differences.append(
            Difference(scenario_name, "(all)", "", f"{labels[0]} recorded {len(left)} steps, {labels[1]} {len(right)}")
        )
    for a, b in zip(left, right):
        if a.name != b.name:
            differences.append(Difference(scenario_name, a.name, a.request, f"step order differs: {a.name!r} vs {b.name!r}"))
            continue
        parts = []
        if a.status != b.status:
            parts.append(f"status {a.status} vs {b.status}")
        if a.content_type != b.content_type:
            parts.append(f"content type {a.content_type!r} vs {b.content_type!r}")
        if a.headers != b.headers:
            parts.append(f"headers {a.headers} vs {b.headers}")
        if a.body != b.body:
            parts.append("body:\n" + _body_diff(a.body, b.body, labels))
        if parts:
            differences.append(Difference(scenario_name, a.name, a.request, "; ".join(parts)))
    return differences


def _body_diff(left: Any, right: Any, labels: tuple[str, str], path: str = "$") -> str:
    """A compact description of where two normalized JSON values differ."""
    lines: list[str] = []

    def walk(a: Any, b: Any, where: str) -> None:
        if len(lines) >= 12:
            return
        if isinstance(a, dict) and isinstance(b, dict):
            for key in sorted(set(a) | set(b)):
                if key not in a:
                    lines.append(f"      {where}.{key}: only in {labels[1]}: {_short(b[key])}")
                elif key not in b:
                    lines.append(f"      {where}.{key}: only in {labels[0]}: {_short(a[key])}")
                else:
                    walk(a[key], b[key], f"{where}.{key}")
        elif isinstance(a, list) and isinstance(b, list):
            if len(a) != len(b):
                lines.append(f"      {where}: {len(a)} items vs {len(b)} items")
            for index, (x, y) in enumerate(zip(a, b)):
                walk(x, y, f"{where}[{index}]")
        elif a != b:
            lines.append(f"      {where}: {_short(a)} vs {_short(b)}")

    walk(left, right, path)
    return "\n".join(lines)


def _short(value: Any) -> str:
    text = json.dumps(value, ensure_ascii=False, sort_keys=True)
    return text if len(text) <= 160 else text[:157] + "..."


def multipart(field_name: str, file_name: str, content: bytes, content_type: str = "application/octet-stream") -> tuple[bytes, str]:
    """Builds a multipart/form-data body with one file part. Returns (body, content type)."""
    boundary = uuid.uuid4().hex
    head = (
        f"--{boundary}\r\n"
        f'Content-Disposition: form-data; name="{field_name}"; filename="{file_name}"\r\n'
        f"Content-Type: {content_type}\r\n\r\n"
    ).encode()
    tail = f"\r\n--{boundary}--\r\n".encode()
    return head + content + tail, f"multipart/form-data; boundary={boundary}"
