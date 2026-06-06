import base64
import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer

from lap import APIError, LAP, LAPError, OPENCODE


class RecordingHandler(BaseHTTPRequestHandler):
    requests = []
    responses = []

    def do_POST(self):
        self._record()

    def do_GET(self):
        self._record()

    def log_message(self, *_args):
        pass

    def _record(self):
        length = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(length).decode("utf-8") if length else ""
        self.requests.append(
            {
                "method": self.command,
                "path": self.path,
                "headers": dict(self.headers),
                "body": json.loads(body) if body else None,
            }
        )
        response = self.responses.pop(0)
        self.send_response(response.get("status", 200))
        for name, value in response.get("headers", {}).items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(response.get("body", b""))


class ServerCase(unittest.TestCase):
    def setUp(self):
        RecordingHandler.requests = []
        RecordingHandler.responses = []
        self.server = HTTPServer(("127.0.0.1", 0), RecordingHandler)
        self.thread = threading.Thread(target=self.server.serve_forever)
        self.thread.start()
        self.base_url = f"http://127.0.0.1:{self.server.server_port}"

    def tearDown(self):
        self.server.shutdown()
        self.thread.join()
        self.server.server_close()

    def enqueue_json(self, payload, status=200):
        RecordingHandler.responses.append(
            {
                "status": status,
                "headers": {"Content-Type": "application/json"},
                "body": json.dumps(payload).encode("utf-8"),
            }
        )

    def enqueue_sse(self, text):
        RecordingHandler.responses.append(
            {
                "headers": {"Content-Type": "text/event-stream"},
                "body": text.encode("utf-8"),
            }
        )

    def client(self):
        return LAP(anthropic_api_key="sk-ant-test", anthropic_base_url=self.base_url)

    def opencode_client(self, **kwargs):
        return LAP(opencode_base_url=self.base_url, **kwargs)


class LapSdkTests(ServerCase):
    def header(self, request, name):
        headers = {key.lower(): value for key, value in request["headers"].items()}
        return headers[name.lower()]

    def test_create_agent_uses_anthropic_managed_agents_runtime(self):
        self.enqueue_json({"id": "agent_123", "version": 1})

        agent = self.client().beta.agents.create(
            lap_agent_runtime="claude_managed_agents",
            name="Coding Assistant",
            model="claude-opus-4-8",
            system="Write clean code.",
            tools=[{"type": "agent_toolset_20260401"}],
        )

        self.assertEqual(agent.id, "agent_123")
        self.assertEqual(agent.version, 1)
        request = RecordingHandler.requests[0]
        self.assertEqual(request["method"], "POST")
        self.assertEqual(request["path"], "/v1/agents")
        self.assertEqual(self.header(request, "X-Api-Key"), "sk-ant-test")
        self.assertEqual(self.header(request, "anthropic-beta"), "managed-agents-2026-04-01")
        self.assertNotIn("lap_agent_runtime", request["body"])
        self.assertEqual(request["body"]["model"], "claude-opus-4-8")

    def test_create_environment_and_session_then_send_events(self):
        self.enqueue_json({"id": "env_123"})
        self.enqueue_json({"id": "sesn_123", "title": "Quickstart session"})
        self.enqueue_json({"data": []})
        client = self.client()

        environment = client.beta.environments.create(
            lap_agent_runtime="claude_managed_agents",
            name="quickstart-env",
            config={"type": "cloud", "networking": {"type": "unrestricted"}},
        )
        session = client.beta.sessions.create(
            agent="agent_123",
            environment_id=environment.id,
            title="Quickstart session",
        )
        client.beta.sessions.events.send(
            session.id,
            events=[
                {
                    "type": "user.message",
                    "content": [{"type": "text", "text": "Create fibonacci.txt"}],
                }
            ],
        )

        self.assertEqual(RecordingHandler.requests[0]["path"], "/v1/environments")
        self.assertEqual(RecordingHandler.requests[1]["path"], "/v1/sessions")
        self.assertEqual(RecordingHandler.requests[1]["body"]["agent"], "agent_123")
        self.assertEqual(RecordingHandler.requests[1]["body"]["environment_id"], "env_123")
        self.assertEqual(
            RecordingHandler.requests[2]["path"],
            "/v1/sessions/sesn_123/events",
        )
        self.assertEqual(RecordingHandler.requests[2]["body"]["events"][0]["type"], "user.message")

    def test_stream_parses_sse_events_as_attribute_objects(self):
        self.enqueue_json({"id": "sesn_123"})
        self.enqueue_sse(
            'event: agent.message\n'
            'data: {"content":[{"type":"text","text":"hello"}]}\n\n'
            'data: {"type":"session.status_idle"}\n\n'
        )
        client = self.client()
        session = client.beta.sessions.create(
            agent="agent_123",
            environment_id="env_123",
            title="Quickstart session",
        )

        with client.beta.sessions.events.stream(session.id) as stream:
            events = list(stream)

        self.assertEqual(events[0].type, "agent.message")
        self.assertEqual(events[0].content[0].text, "hello")
        self.assertEqual(events[1].type, "session.status_idle")
        self.assertEqual(
            RecordingHandler.requests[1]["path"],
            "/v1/sessions/sesn_123/events/stream",
        )

    def test_unsupported_runtime_fails_before_network(self):
        with self.assertRaises(LAPError):
            self.client().beta.agents.create(
                lap_agent_runtime="cursor",
                name="Cursor Agent",
                model="composer-2",
                system="Write code.",
                tools=[],
            )
        self.assertEqual(RecordingHandler.requests, [])

    def test_provider_error_includes_status_and_body(self):
        self.enqueue_json({"error": {"message": "bad key"}}, status=401)

        with self.assertRaises(APIError) as error:
            self.client().beta.agents.create(
                lap_agent_runtime="claude_managed_agents",
                name="Coding Assistant",
                model="claude-opus-4-8",
                system="Write clean code.",
                tools=[],
            )

        self.assertEqual(error.exception.status, 401)
        self.assertIn("bad key", error.exception.body)

    def test_opencode_health_uses_basic_auth_when_configured(self):
        self.enqueue_json({"healthy": True, "version": "1.0.0"})
        expected_auth = "Basic " + base64.b64encode(b"opencode:pw").decode("ascii")

        health = self.opencode_client(opencode_password="pw").opencode.health()

        self.assertTrue(health.healthy)
        self.assertEqual(health.version, "1.0.0")
        request = RecordingHandler.requests[0]
        self.assertEqual(request["method"], "GET")
        self.assertEqual(request["path"], "/global/health")
        self.assertEqual(self.header(request, "authorization"), expected_auth)

    def test_opencode_sessions_prompt_uses_server_api(self):
        self.enqueue_json({"id": "sesn_123", "title": "Quickstart"})
        self.enqueue_json(
            {
                "info": {"id": "msg_123", "role": "assistant"},
                "parts": [{"type": "text", "text": "done"}],
            }
        )
        client = self.opencode_client()

        session = client.opencode.sessions.create(title="Quickstart")
        response = client.opencode.sessions.prompt(
            session.id,
            text="Create fibonacci.txt",
            agent="build",
            model={"providerID": "anthropic", "modelID": "claude-3-5-sonnet-20241022"},
        )

        self.assertEqual(response.info.id, "msg_123")
        self.assertEqual(response.parts[0].text, "done")
        create_request = RecordingHandler.requests[0]
        prompt_request = RecordingHandler.requests[1]
        self.assertEqual(create_request["method"], "POST")
        self.assertEqual(create_request["path"], "/session")
        self.assertEqual(create_request["body"], {"title": "Quickstart"})
        self.assertEqual(prompt_request["method"], "POST")
        self.assertEqual(prompt_request["path"], "/session/sesn_123/message")
        self.assertEqual(
            prompt_request["body"]["parts"],
            [{"type": "text", "text": "Create fibonacci.txt"}],
        )
        self.assertEqual(prompt_request["body"]["agent"], "build")
        self.assertEqual(
            prompt_request["body"]["model"],
            {"providerID": "anthropic", "modelID": "claude-3-5-sonnet-20241022"},
        )
        self.assertNotIn("x-api-key", {key.lower() for key in prompt_request["headers"]})

    def test_beta_sessions_can_route_events_to_opencode(self):
        self.enqueue_json({"id": "sesn_123", "title": "Quickstart"})
        self.enqueue_json(
            {
                "info": {"id": "msg_123", "role": "assistant"},
                "parts": [{"type": "text", "text": "done"}],
            }
        )
        self.enqueue_sse(
            'event: server.connected\n'
            'data: {"version":"1.0.0"}\n\n'
            'data: {"type":"session.idle","sessionID":"sesn_123"}\n\n'
        )
        client = self.opencode_client()

        session = client.beta.sessions.create(
            lap_agent_runtime=OPENCODE,
            title="Quickstart",
        )
        client.beta.sessions.events.send(
            session.id,
            events=[
                {
                    "type": "user.message",
                    "content": [{"type": "text", "text": "Create fibonacci.txt"}],
                }
            ],
            model={"providerID": "anthropic", "modelID": "claude-3-5-sonnet-20241022"},
        )
        with client.beta.sessions.events.stream(session.id) as stream:
            events = list(stream)

        self.assertEqual(RecordingHandler.requests[0]["path"], "/session")
        self.assertEqual(RecordingHandler.requests[0]["body"], {"title": "Quickstart"})
        self.assertEqual(RecordingHandler.requests[1]["path"], "/session/sesn_123/message")
        self.assertEqual(
            RecordingHandler.requests[1]["body"]["parts"],
            [{"type": "text", "text": "Create fibonacci.txt"}],
        )
        self.assertEqual(RecordingHandler.requests[2]["path"], "/event")
        self.assertEqual(events[0].type, "server.connected")
        self.assertEqual(events[0].version, "1.0.0")
        self.assertEqual(events[1].type, "session.idle")

    def test_opencode_agent_create_fails_before_network(self):
        with self.assertRaises(LAPError):
            self.opencode_client().beta.agents.create(
                lap_agent_runtime=OPENCODE,
                name="Unsupported",
            )
        self.assertEqual(RecordingHandler.requests, [])


if __name__ == "__main__":
    unittest.main()
