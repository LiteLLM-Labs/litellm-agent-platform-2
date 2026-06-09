import json
import os
import queue
import sqlite3
import threading
import time
import uuid
from pathlib import Path
from typing import Any

from fastapi import FastAPI, Header, HTTPException, Request
from fastapi.responses import JSONResponse, StreamingResponse
from pydantic import BaseModel

try:
    from deepagents import create_deep_agent
except Exception:  # pragma: no cover - surfaced by /health
    create_deep_agent = None


PORT = int(os.environ.get("PORT", "8080"))
DB_PATH = os.environ.get("DB_PATH", "/data/agents.db")
DEFAULT_MODEL = os.environ.get("DEFAULT_MODEL", "anthropic:claude-sonnet-4-5")
RUNTIME_API_KEY = os.environ.get("RUNTIME_API_KEY")


app = FastAPI(title="DeepAgents Anthropic Managed Agents bridge")
state_lock = threading.Lock()
run_queues: dict[str, "queue.Queue[dict[str, Any]]"] = {}
active_runs: dict[str, bool] = {}


def now_ms() -> int:
    return int(time.time() * 1000)


def new_id(prefix: str) -> str:
    return f"{prefix}_{uuid.uuid4().hex}"


def db() -> sqlite3.Connection:
    Path(DB_PATH).parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(DB_PATH, check_same_thread=False)
    conn.row_factory = sqlite3.Row
    return conn


def init_db() -> None:
    with db() as conn:
        conn.executescript(
            """
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS agents (
              id TEXT PRIMARY KEY,
              name TEXT,
              description TEXT,
              model TEXT,
              system TEXT,
              tools TEXT,
              mcp_servers TEXT,
              metadata TEXT,
              created_at INTEGER,
              updated_at INTEGER
            );
            CREATE TABLE IF NOT EXISTS environments (
              id TEXT PRIMARY KEY,
              name TEXT,
              config TEXT,
              description TEXT,
              created_at INTEGER
            );
            CREATE TABLE IF NOT EXISTS sessions (
              id TEXT PRIMARY KEY,
              agent_id TEXT,
              environment_id TEXT,
              title TEXT,
              status TEXT,
              created_at INTEGER,
              updated_at INTEGER
            );
            CREATE TABLE IF NOT EXISTS events (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              session_id TEXT,
              event TEXT,
              data TEXT,
              created_at INTEGER
            );
            """
        )


init_db()


class CreateAgentRequest(BaseModel):
    name: str | None = None
    model: str | dict[str, Any] | None = None
    system: str | None = None
    system_prompt: str | None = None
    description: str | None = None
    tools: list[dict[str, Any]] | None = None
    mcp_servers: list[dict[str, Any]] | None = None
    metadata: dict[str, Any] | None = None


class CreateEnvironmentRequest(BaseModel):
    name: str | None = None
    config: dict[str, Any] | None = None
    description: str | None = None


class CreateSessionRequest(BaseModel):
    agent: str
    environment_id: str | None = None
    title: str | None = None
    metadata: dict[str, Any] | None = None


class SendEventsRequest(BaseModel):
    events: list[dict[str, Any]]


@app.middleware("http")
async def require_runtime_key(request: Request, call_next):
    if request.url.path == "/health":
        return await call_next(request)
    api_key = request.headers.get("x-api-key")
    if RUNTIME_API_KEY:
        if api_key != RUNTIME_API_KEY:
            return JSONResponse(
                status_code=401,
                content={"error": "invalid runtime api key"},
            )
    elif not api_key:
        return JSONResponse(
            status_code=401,
            content={"error": "x-api-key is required"},
        )
    return await call_next(request)


@app.get("/health")
def health() -> dict[str, Any]:
    return {
        "ok": True,
        "deepagents": create_deep_agent is not None,
        "anthropic_api_key": bool(os.environ.get("ANTHROPIC_API_KEY")),
    }


def model_id(model: Any) -> str:
    if isinstance(model, str) and model.strip():
        return model.strip()
    if isinstance(model, dict) and isinstance(model.get("id"), str):
        return model["id"].strip()
    return DEFAULT_MODEL


def json_dumps(value: Any) -> str:
    return json.dumps(value, separators=(",", ":"))


def parse_json(value: str | None, fallback: Any) -> Any:
    if not value:
        return fallback
    try:
        return json.loads(value)
    except json.JSONDecodeError:
        return fallback


def row_to_agent(row: sqlite3.Row) -> dict[str, Any]:
    return {
        "id": row["id"],
        "type": "agent",
        "name": row["name"],
        "description": row["description"],
        "model": {"id": row["model"] or DEFAULT_MODEL},
        "system": row["system"] or "",
        "tools": parse_json(row["tools"], []),
        "mcp_servers": parse_json(row["mcp_servers"], []),
        "metadata": parse_json(row["metadata"], None),
        "version": 1,
        "created_at": row["created_at"],
        "updated_at": row["updated_at"],
    }


def row_to_environment(row: sqlite3.Row) -> dict[str, Any]:
    return {
        "id": row["id"],
        "type": "environment",
        "name": row["name"],
        "config": parse_json(row["config"], {}),
        "description": row["description"],
        "created_at": row["created_at"],
    }


def row_to_session(row: sqlite3.Row) -> dict[str, Any]:
    return {
        "id": row["id"],
        "type": "session",
        "agent": row["agent_id"],
        "environment_id": row["environment_id"],
        "title": row["title"],
        "status": row["status"],
        "created_at": row["created_at"],
        "updated_at": row["updated_at"],
    }


def get_agent(agent_id: str) -> sqlite3.Row | None:
    with db() as conn:
        return conn.execute("SELECT * FROM agents WHERE id = ?", (agent_id,)).fetchone()


def get_session(session_id: str) -> sqlite3.Row | None:
    with db() as conn:
        return conn.execute("SELECT * FROM sessions WHERE id = ?", (session_id,)).fetchone()


def append_event(session_id: str, event: str, data: dict[str, Any]) -> dict[str, Any]:
    with db() as conn:
        cursor = conn.execute(
            "INSERT INTO events (session_id, event, data, created_at) VALUES (?, ?, ?, ?)",
            (session_id, event, json_dumps(data), now_ms()),
        )
        event_id = cursor.lastrowid
    record = {"id": event_id, "event": event, "data": data}
    with state_lock:
        q = run_queues.get(session_id)
    if q:
        q.put(record)
    return record


def set_session_status(session_id: str, status: str) -> None:
    with db() as conn:
        conn.execute(
            "UPDATE sessions SET status = ?, updated_at = ? WHERE id = ?",
            (status, now_ms(), session_id),
        )


def list_events(session_id: str) -> list[dict[str, Any]]:
    with db() as conn:
        rows = conn.execute(
            "SELECT id, event, data FROM events WHERE session_id = ? ORDER BY id ASC",
            (session_id,),
        ).fetchall()
    return [
        {"id": row["id"], "event": row["event"], "data": parse_json(row["data"], {})}
        for row in rows
    ]


def user_text(events: list[dict[str, Any]]) -> str:
    chunks: list[str] = []
    for event in events:
        if event.get("type") != "user.message":
            continue
        content = event.get("content")
        if isinstance(content, str):
            chunks.append(content)
        elif isinstance(content, list):
            for item in content:
                if isinstance(item, str):
                    chunks.append(item)
                elif isinstance(item, dict) and item.get("type") == "text":
                    chunks.append(str(item.get("text") or ""))
    return "\n".join(chunk for chunk in chunks if chunk).strip()


def message_text(message: Any) -> str:
    content = getattr(message, "content", None)
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        text: list[str] = []
        for item in content:
            if isinstance(item, str):
                text.append(item)
            elif isinstance(item, dict):
                text.append(str(item.get("text") or item.get("content") or ""))
        return "".join(text)
    return ""


def messages_from_update(update: Any) -> list[Any]:
    if not isinstance(update, dict):
        return []
    messages: list[Any] = []
    for value in update.values():
        if not isinstance(value, dict):
            continue
        raw = value.get("messages")
        if raw is None:
            continue
        raw = getattr(raw, "value", raw)
        if isinstance(raw, list):
            messages.extend(raw)
    return messages


def normalize_model_for_deepagents(model: str) -> str:
    if model.startswith("anthropic/"):
        return "anthropic:" + model.split("/", 1)[1]
    if model.startswith("openai/"):
        return "openai:" + model.split("/", 1)[1]
    return model


def final_text_from_result(result: Any) -> str:
    if not isinstance(result, dict):
        return ""
    messages = result.get("messages") or []
    for message in reversed(messages):
        text = message_text(message)
        if text:
            return text
    return ""


def build_agent(row: sqlite3.Row):
    if create_deep_agent is None:
        raise RuntimeError("deepagents package is not importable")
    return create_deep_agent(
        model=normalize_model_for_deepagents(row["model"] or DEFAULT_MODEL),
        tools=[],
        system_prompt=row["system"] or "You are a helpful assistant.",
    )


def run_agent(session_id: str, prompt: str) -> None:
    session = get_session(session_id)
    if not session:
        append_event(session_id, "session.error", {"error": {"message": "session not found"}})
        return
    agent_row = get_agent(session["agent_id"])
    if not agent_row:
        append_event(session_id, "session.error", {"error": {"message": "agent not found"}})
        return
    with state_lock:
        active_runs[session_id] = True
    set_session_status(session_id, "running")
    append_event(session_id, "session.status_running", {})
    emitted = False
    try:
        agent = build_agent(agent_row)
        payload = {"messages": [{"role": "user", "content": prompt}]}
        for chunk in agent.stream(payload, stream_mode="updates"):
            for message in messages_from_update(chunk):
                text = message_text(message)
                if text:
                    emitted = True
                    append_event(
                        session_id,
                        "agent.message",
                        {"content": [{"type": "text", "text": text}], "model": agent_row["model"]},
                    )
        if not emitted:
            append_event(
                session_id,
                "agent.message",
                {
                    "content": [{
                        "type": "text",
                        "text": "DeepAgents completed without emitting message text.",
                    }],
                    "model": agent_row["model"],
                },
            )
        append_event(
            session_id,
            "session.status_idle",
            {"stop_reason": {"type": "end_turn"}},
        )
        set_session_status(session_id, "idle")
    except Exception as exc:
        append_event(session_id, "session.error", {"error": {"message": str(exc)}})
        set_session_status(session_id, "error")
    finally:
        with state_lock:
            active_runs[session_id] = False
            q = run_queues.get(session_id)
        if q:
            q.put({"event": "__done__", "data": {}})


@app.post("/v1/agents")
def create_agent(input: CreateAgentRequest) -> dict[str, Any]:
    agent_id = new_id("agt")
    timestamp = now_ms()
    system = input.system_prompt if input.system_prompt is not None else input.system
    with db() as conn:
        conn.execute(
            """
            INSERT INTO agents
              (id, name, description, model, system, tools, mcp_servers, metadata, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                agent_id,
                input.name,
                input.description,
                model_id(input.model),
                system or "",
                json_dumps(input.tools or []),
                json_dumps(input.mcp_servers or []),
                json_dumps(input.metadata) if input.metadata is not None else None,
                timestamp,
                timestamp,
            ),
        )
        row = conn.execute("SELECT * FROM agents WHERE id = ?", (agent_id,)).fetchone()
    return row_to_agent(row)


@app.get("/v1/agents")
def list_agents() -> dict[str, Any]:
    with db() as conn:
        rows = conn.execute("SELECT * FROM agents ORDER BY created_at ASC").fetchall()
    return {"data": [row_to_agent(row) for row in rows]}


@app.get("/v1/agents/{agent_id}")
def read_agent(agent_id: str) -> dict[str, Any]:
    row = get_agent(agent_id)
    if not row:
        raise HTTPException(status_code=404, detail="agent not found")
    return row_to_agent(row)


@app.patch("/v1/agents/{agent_id}")
def update_agent(agent_id: str, input: CreateAgentRequest) -> dict[str, Any]:
    existing = get_agent(agent_id)
    if not existing:
        raise HTTPException(status_code=404, detail="agent not found")
    system = input.system_prompt if input.system_prompt is not None else input.system
    with db() as conn:
        conn.execute(
            """
            UPDATE agents SET
              name = COALESCE(?, name),
              description = COALESCE(?, description),
              model = COALESCE(?, model),
              system = COALESCE(?, system),
              tools = COALESCE(?, tools),
              mcp_servers = COALESCE(?, mcp_servers),
              metadata = COALESCE(?, metadata),
              updated_at = ?
            WHERE id = ?
            """,
            (
                input.name,
                input.description,
                model_id(input.model) if input.model is not None else None,
                system,
                json_dumps(input.tools) if input.tools is not None else None,
                json_dumps(input.mcp_servers) if input.mcp_servers is not None else None,
                json_dumps(input.metadata) if input.metadata is not None else None,
                now_ms(),
                agent_id,
            ),
        )
        row = conn.execute("SELECT * FROM agents WHERE id = ?", (agent_id,)).fetchone()
    return row_to_agent(row)


@app.post("/v1/environments")
def create_environment(input: CreateEnvironmentRequest) -> dict[str, Any]:
    env_id = new_id("env")
    with db() as conn:
        conn.execute(
            """
            INSERT INTO environments (id, name, config, description, created_at)
            VALUES (?, ?, ?, ?, ?)
            """,
            (
                env_id,
                input.name,
                json_dumps(input.config or {}),
                input.description,
                now_ms(),
            ),
        )
        row = conn.execute("SELECT * FROM environments WHERE id = ?", (env_id,)).fetchone()
    return row_to_environment(row)


@app.post("/v1/sessions")
def create_session(input: CreateSessionRequest) -> dict[str, Any]:
    if not get_agent(input.agent):
        raise HTTPException(status_code=400, detail="unknown agent")
    session_id = new_id("ses")
    timestamp = now_ms()
    with db() as conn:
        conn.execute(
            """
            INSERT INTO sessions
              (id, agent_id, environment_id, title, status, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            (
                session_id,
                input.agent,
                input.environment_id,
                input.title or "DeepAgents session",
                "idle",
                timestamp,
                timestamp,
            ),
        )
        row = conn.execute("SELECT * FROM sessions WHERE id = ?", (session_id,)).fetchone()
    return row_to_session(row)


@app.post("/v1/sessions/{session_id}/events")
def send_events(session_id: str, input: SendEventsRequest) -> dict[str, Any]:
    if not get_session(session_id):
        raise HTTPException(status_code=404, detail="session not found")
    prompt = user_text(input.events)
    if not prompt:
        raise HTTPException(status_code=400, detail="no user.message text")
    with state_lock:
        if active_runs.get(session_id):
            raise HTTPException(status_code=409, detail="session already running")
        run_queues.setdefault(session_id, queue.Queue())
    thread = threading.Thread(target=run_agent, args=(session_id, prompt), daemon=True)
    thread.start()
    return JSONResponse(status_code=202, content={"ok": True})


@app.get("/v1/sessions/{session_id}/events")
def get_events(session_id: str) -> dict[str, Any]:
    if not get_session(session_id):
        raise HTTPException(status_code=404, detail="session not found")
    return {"data": [{"type": item["event"], **item["data"]} for item in list_events(session_id)]}


@app.post("/v1/sessions/{session_id}/abort")
def abort_session(session_id: str) -> dict[str, Any]:
    if not get_session(session_id):
        raise HTTPException(status_code=404, detail="session not found")
    append_event(
        session_id,
        "session.error",
        {"error": {"message": "interrupt is not supported by this DeepAgents template"}},
    )
    set_session_status(session_id, "error")
    with state_lock:
        active_runs[session_id] = False
        q = run_queues.get(session_id)
    if q:
        q.put({"event": "__done__", "data": {}})
    return {"aborted": False}


def sse_frame(item: dict[str, Any]) -> str:
    return f"event: {item['event']}\ndata: {json_dumps(item['data'])}\n\n"


@app.get("/v1/sessions/{session_id}/events/stream")
def stream_events(session_id: str, x_api_key: str | None = Header(default=None)):
    if not get_session(session_id):
        raise HTTPException(status_code=404, detail="session not found")

    def generate():
        replayed = list_events(session_id)
        seen_ids = {item["id"] for item in replayed if item.get("id") is not None}
        for item in replayed:
            yield sse_frame(item)
        with state_lock:
            q = run_queues.setdefault(session_id, queue.Queue())
        idle_loops = 0
        while True:
            try:
                item = q.get(timeout=1.0)
            except queue.Empty:
                with state_lock:
                    running = active_runs.get(session_id, False)
                if not running:
                    current = list_events(session_id)
                    unseen = [
                        item
                        for item in current
                        if item.get("id") is not None and item["id"] not in seen_ids
                    ]
                    if unseen:
                        for replay in unseen:
                            seen_ids.add(replay["id"])
                            yield sse_frame(replay)
                    idle_loops += 1
                    if idle_loops >= 2:
                        break
                continue
            if item["event"] == "__done__":
                current = list_events(session_id)
                for replay in current:
                    if replay.get("id") is not None and replay["id"] not in seen_ids:
                        seen_ids.add(replay["id"])
                        yield sse_frame(replay)
                break
            if item.get("id") is None or item["id"] not in seen_ids:
                if item.get("id") is not None:
                    seen_ids.add(item["id"])
                yield sse_frame(item)

    return StreamingResponse(generate(), media_type="text/event-stream")
