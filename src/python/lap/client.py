from __future__ import annotations

import base64
import json
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any, Iterable, Iterator


MANAGED_AGENTS_BETA = "managed-agents-2026-04-01"
ANTHROPIC_VERSION = "2023-06-01"
CLAUDE_MANAGED_AGENTS = "claude_managed_agents"
OPENCODE = "opencode"


class LAPError(Exception):
    pass


class APIError(LAPError):
    def __init__(self, status: int, body: str):
        super().__init__(f"provider request failed with status {status}: {body}")
        self.status = status
        self.body = body


class AttrObject:
    def __init__(self, value: dict[str, Any]):
        self._value = value

    def __getattr__(self, name: str) -> Any:
        try:
            return wrap(self._value[name])
        except KeyError as error:
            raise AttributeError(name) from error

    def __getitem__(self, name: str) -> Any:
        return wrap(self._value[name])

    def __contains__(self, name: str) -> bool:
        return name in self._value

    def __repr__(self) -> str:
        return repr(self._value)

    def to_dict(self) -> dict[str, Any]:
        return self._value


def wrap(value: Any) -> Any:
    if isinstance(value, dict):
        return AttrObject(value)
    if isinstance(value, list):
        return [wrap(item) for item in value]
    return value


@dataclass(frozen=True)
class _RuntimeConfig:
    base_url: str
    kind: str
    api_key: str | None = None
    username: str | None = None
    password: str | None = None


class LAP:
    def __init__(
        self,
        *,
        anthropic_api_key: str | None = None,
        anthropic_base_url: str = "https://api.anthropic.com",
        opencode_base_url: str | None = None,
        opencode_username: str = "opencode",
        opencode_password: str | None = None,
        timeout: float = 60.0,
    ):
        runtimes = {}
        if anthropic_api_key:
            runtimes[CLAUDE_MANAGED_AGENTS] = _RuntimeConfig(
                base_url=anthropic_base_url.rstrip("/"),
                kind=CLAUDE_MANAGED_AGENTS,
                api_key=anthropic_api_key,
            )
        if opencode_base_url:
            runtimes[OPENCODE] = _RuntimeConfig(
                base_url=opencode_base_url.rstrip("/"),
                kind=OPENCODE,
                username=opencode_username,
                password=opencode_password,
            )
        self._transport = _Transport(runtimes, timeout)
        self.beta = _Beta(self._transport)
        self.opencode = _OpenCode(self._transport)


class _Beta:
    def __init__(self, transport: "_Transport"):
        self.agents = _Agents(transport)
        self.environments = _Environments(transport)
        self.sessions = _Sessions(transport)


class _Agents:
    def __init__(self, transport: "_Transport"):
        self._transport = transport

    def create(self, *, lap_agent_runtime: str, **params: Any) -> AttrObject:
        runtime = parse_runtime(lap_agent_runtime)
        if self._transport.kind(runtime) == OPENCODE:
            raise LAPError("agents.create is not supported for opencode")
        return self._transport.request(runtime, "POST", "/v1/agents", params)


class _Environments:
    def __init__(self, transport: "_Transport"):
        self._transport = transport

    def create(self, *, lap_agent_runtime: str, **params: Any) -> AttrObject:
        runtime = parse_runtime(lap_agent_runtime)
        if self._transport.kind(runtime) == OPENCODE:
            raise LAPError("environments.create is not supported for opencode")
        return self._transport.request(runtime, "POST", "/v1/environments", params)


class _Sessions:
    def __init__(self, transport: "_Transport"):
        self._transport = transport
        self.events = _SessionEvents(transport)

    def create(
        self,
        *,
        title: str,
        agent: str | dict[str, Any] | None = None,
        environment_id: str | None = None,
        lap_agent_runtime: str | None = None,
        **params: Any,
    ) -> AttrObject:
        runtime = (
            parse_runtime(lap_agent_runtime)
            if lap_agent_runtime
            else self._transport.default_runtime()
        )
        if self._transport.kind(runtime) == OPENCODE:
            body = {"title": title, **params}
            session = self._transport.request(runtime, "POST", "/session", body)
            self._transport.remember_session(session.id, runtime)
            return session
        if agent is None or environment_id is None:
            raise LAPError(
                "agent and environment_id are required for claude_managed_agents sessions"
            )
        body = {"agent": agent, "environment_id": environment_id, "title": title, **params}
        session = self._transport.request(runtime, "POST", "/v1/sessions", body)
        self._transport.remember_session(session.id, runtime)
        return session


class _SessionEvents:
    def __init__(self, transport: "_Transport"):
        self._transport = transport

    def send(
        self,
        session_id: str,
        *,
        events: list[dict[str, Any]] | None = None,
        **params: Any,
    ) -> AttrObject:
        runtime = self._transport.runtime_for_session(session_id)
        if self._transport.kind(runtime) == OPENCODE:
            body = opencode_prompt_body(events, params)
            return self._transport.request(
                runtime,
                "POST",
                f"/session/{quote_path(session_id)}/message",
                body,
            )
        if events is None:
            raise LAPError("events are required")
        return self._transport.request(
            runtime,
            "POST",
            f"/v1/sessions/{session_id}/events",
            {"events": events},
        )

    def stream(self, session_id: str) -> "EventStream":
        runtime = self._transport.runtime_for_session(session_id)
        if self._transport.kind(runtime) == OPENCODE:
            return self._transport.stream(runtime, "/event")
        return self._transport.stream(runtime, f"/v1/sessions/{session_id}/events/stream")


class _OpenCode:
    def __init__(self, transport: "_Transport"):
        self.health = _OpenCodeHealth(transport).get
        self.agents = _OpenCodeAgents(transport)
        self.sessions = _OpenCodeSessions(transport)
        self.events = _OpenCodeEvents(transport)


class _OpenCodeHealth:
    def __init__(self, transport: "_Transport"):
        self._transport = transport

    def get(self) -> AttrObject:
        return self._transport.request(OPENCODE, "GET", "/global/health")


class _OpenCodeAgents:
    def __init__(self, transport: "_Transport"):
        self._transport = transport

    def list(self) -> list[Any]:
        return self._transport.request(OPENCODE, "GET", "/agent")


class _OpenCodeSessions:
    def __init__(self, transport: "_Transport"):
        self._transport = transport

    def list(self) -> list[Any]:
        return self._transport.request(OPENCODE, "GET", "/session")

    def create(
        self,
        *,
        title: str | None = None,
        parent_id: str | None = None,
        **params: Any,
    ) -> AttrObject:
        body = {**params}
        if title is not None:
            body["title"] = title
        if parent_id is not None:
            body["parentID"] = parent_id
        session = self._transport.request(OPENCODE, "POST", "/session", body)
        self._transport.remember_session(session.id, OPENCODE)
        return session

    def get(self, session_id: str) -> AttrObject:
        return self._transport.request(OPENCODE, "GET", f"/session/{quote_path(session_id)}")

    def messages(self, session_id: str, *, limit: int | None = None) -> list[Any]:
        path = f"/session/{quote_path(session_id)}/message"
        if limit is not None:
            path = with_query(path, {"limit": limit})
        return self._transport.request(OPENCODE, "GET", path)

    def prompt(
        self,
        session_id: str,
        *,
        text: str | None = None,
        parts: list[dict[str, Any]] | None = None,
        **params: Any,
    ) -> AttrObject:
        body = opencode_prompt_body(None, {**params, **text_or_parts(text, parts)})
        return self._transport.request(
            OPENCODE,
            "POST",
            f"/session/{quote_path(session_id)}/message",
            body,
        )

    def prompt_async(
        self,
        session_id: str,
        *,
        text: str | None = None,
        parts: list[dict[str, Any]] | None = None,
        **params: Any,
    ) -> AttrObject:
        body = opencode_prompt_body(None, {**params, **text_or_parts(text, parts)})
        return self._transport.request(
            OPENCODE,
            "POST",
            f"/session/{quote_path(session_id)}/prompt_async",
            body,
        )

    def abort(self, session_id: str) -> AttrObject:
        return self._transport.request(OPENCODE, "POST", f"/session/{quote_path(session_id)}/abort")


class _OpenCodeEvents:
    def __init__(self, transport: "_Transport"):
        self._transport = transport

    def stream(self) -> "EventStream":
        return self._transport.stream(OPENCODE, "/event")


class _Transport:
    def __init__(self, runtimes: dict[str, _RuntimeConfig], timeout: float):
        self._runtimes = runtimes
        self._timeout = timeout
        self._session_runtimes: dict[str, str] = {}

    def default_runtime(self) -> str:
        if len(self._runtimes) == 1:
            return next(iter(self._runtimes))
        if not self._runtimes:
            raise LAPError("no agent runtimes configured")
        raise LAPError("lap_agent_runtime is required when multiple runtimes are configured")

    def runtime_for_session(self, session_id: str) -> str:
        return self._session_runtimes.get(session_id, self.default_runtime())

    def remember_session(self, session_id: str, runtime: str) -> None:
        self._session_runtimes[session_id] = runtime

    def kind(self, runtime: str) -> str:
        return self._runtime(runtime).kind

    def request(
        self,
        runtime: str,
        method: str,
        path: str,
        body: dict[str, Any] | None = None,
    ) -> AttrObject:
        return wrap(self.request_raw(runtime, method, path, body))

    def request_raw(
        self,
        runtime: str,
        method: str,
        path: str,
        body: dict[str, Any] | None = None,
    ) -> Any:
        data = None if body is None else json.dumps(body).encode("utf-8")
        request = urllib.request.Request(
            self._url(runtime, path),
            data=data,
            method=method,
            headers=self._headers(runtime),
        )
        with self._open(request) as response:
            payload = response.read().decode("utf-8")
        if not payload.strip():
            return {}
        return json.loads(payload)

    def stream(self, runtime: str, path: str) -> "EventStream":
        request = urllib.request.Request(
            self._url(runtime, path),
            method="GET",
            headers={**self._headers(runtime), "Accept": "text/event-stream"},
        )
        return EventStream(self._open(request))

    def _url(self, runtime: str, path: str) -> str:
        config = self._runtime(runtime)
        return f"{config.base_url}{path}"

    def _headers(self, runtime: str) -> dict[str, str]:
        config = self._runtime(runtime)
        if config.kind == CLAUDE_MANAGED_AGENTS:
            return {
                "Content-Type": "application/json",
                "X-Api-Key": config.api_key or "",
                "anthropic-version": ANTHROPIC_VERSION,
                "anthropic-beta": MANAGED_AGENTS_BETA,
            }
        if config.kind == OPENCODE:
            headers = {"Content-Type": "application/json"}
            if config.password:
                userpass = f"{config.username or 'opencode'}:{config.password}".encode("utf-8")
                headers["Authorization"] = f"Basic {base64.b64encode(userpass).decode('ascii')}"
            return headers
        raise LAPError(f"unsupported runtime kind: {config.kind}")

    def _runtime(self, runtime: str) -> _RuntimeConfig:
        config = self._runtimes.get(runtime)
        if config is None:
            raise LAPError(f"{runtime} runtime is not configured")
        return config

    def _open(self, request: urllib.request.Request):
        try:
            return urllib.request.urlopen(request, timeout=self._timeout)
        except urllib.error.HTTPError as error:
            body = error.read().decode("utf-8", errors="replace")
            raise APIError(error.code, body) from error


class EventStream:
    def __init__(self, response: Any):
        self._response = response

    def __enter__(self) -> "EventStream":
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def __iter__(self) -> Iterator[AttrObject]:
        yield from parse_sse(self._response)

    def close(self) -> None:
        self._response.close()


def parse_runtime(runtime: str) -> str:
    if runtime not in {CLAUDE_MANAGED_AGENTS, OPENCODE}:
        raise LAPError(f"unsupported lap_agent_runtime: {runtime}")
    return runtime


def quote_path(value: str) -> str:
    return urllib.parse.quote(value, safe="")


def with_query(path: str, query: dict[str, Any]) -> str:
    return f"{path}?{urllib.parse.urlencode(query)}"


def text_or_parts(text: str | None, parts: list[dict[str, Any]] | None) -> dict[str, Any]:
    if parts is not None and text is not None:
        raise LAPError("pass either text or parts, not both")
    if parts is not None:
        return {"parts": parts}
    if text is not None:
        return {"parts": [{"type": "text", "text": text}]}
    return {}


def opencode_prompt_body(
    events: list[dict[str, Any]] | None,
    params: dict[str, Any],
) -> dict[str, Any]:
    body = {**params}
    if events is not None:
        if "parts" in body:
            raise LAPError("pass either events or parts, not both")
        body["parts"] = opencode_parts_from_events(events)
    if "parts" not in body:
        raise LAPError("OpenCode prompts require parts")
    return body


def opencode_parts_from_events(events: list[dict[str, Any]]) -> list[dict[str, Any]]:
    parts: list[dict[str, Any]] = []
    for event in events:
        if event.get("type") != "user.message":
            continue
        content = event.get("content", [])
        if isinstance(content, str):
            parts.append({"type": "text", "text": content})
            continue
        for block in content:
            if isinstance(block, str):
                parts.append({"type": "text", "text": block})
            elif isinstance(block, dict) and block.get("type") == "text":
                parts.append({"type": "text", "text": block.get("text", "")})
            elif isinstance(block, dict):
                parts.append(block)
    if not parts:
        raise LAPError("OpenCode prompts require at least one user.message content part")
    return parts


def parse_sse(lines: Iterable[bytes]) -> Iterator[AttrObject]:
    event_name = None
    data_lines: list[str] = []
    for raw_line in lines:
        line = raw_line.decode("utf-8").rstrip("\r\n")
        if not line:
            if data_lines:
                yield parse_sse_event(event_name, data_lines)
            event_name = None
            data_lines = []
            continue
        if line.startswith(":"):
            continue
        field, _, value = line.partition(":")
        if value.startswith(" "):
            value = value[1:]
        if field == "event":
            event_name = value
        elif field == "data":
            data_lines.append(value)
    if data_lines:
        yield parse_sse_event(event_name, data_lines)


def parse_sse_event(event_name: str | None, data_lines: list[str]) -> AttrObject:
    payload = "\n".join(data_lines)
    value = json.loads(payload)
    if event_name and isinstance(value, dict) and "type" not in value:
        value["type"] = event_name
    return wrap(value)
