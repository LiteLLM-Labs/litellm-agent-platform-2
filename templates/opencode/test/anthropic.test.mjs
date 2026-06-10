import assert from "node:assert/strict";
import test from "node:test";

import { translateOpencodeEvent } from "../src/anthropic.mjs";

const ctx = { sessionId: "ses_123", model: "claude-sonnet-4-6" };

test("message deltas still translate to agent.message", () => {
  assert.deepEqual(
    translateOpencodeEvent(
      {
        type: "message.part.delta",
        properties: {
          sessionID: "ses_123",
          delta: { text: "hello" },
        },
      },
      ctx,
    ),
    {
      event: "agent.message",
      data: {
        sessionID: "ses_123",
        content: [{ type: "text", text: "hello" }],
        model: "claude-sonnet-4-6",
      },
    },
  );
});

test("reasoning part deltas translate to agent.thinking", () => {
  assert.deepEqual(
    translateOpencodeEvent(
      {
        type: "message.part.delta",
        properties: {
          sessionID: "ses_123",
          part: { type: "reasoning" },
          delta: { text: "I should inspect the code." },
        },
      },
      ctx,
    ),
    {
      event: "agent.thinking",
      data: {
        sessionID: "ses_123",
        thinking: "I should inspect the code.",
        content: [{ type: "thinking", text: "I should inspect the code." }],
        model: "claude-sonnet-4-6",
      },
    },
  );
});

test("thinking delta events translate to agent.thinking", () => {
  assert.deepEqual(
    translateOpencodeEvent(
      {
        type: "thinking_delta",
        properties: {
          sessionID: "ses_123",
          delta: { thinking: "Need a minimal patch." },
        },
      },
      ctx,
    ),
    {
      event: "agent.thinking",
      data: {
        sessionID: "ses_123",
        thinking: "Need a minimal patch.",
        content: [{ type: "thinking", text: "Need a minimal patch." }],
        model: "claude-sonnet-4-6",
      },
    },
  );
});

test("reasoning delta strings translate to agent.thinking", () => {
  assert.deepEqual(
    translateOpencodeEvent(
      {
        type: "reasoning-delta",
        properties: {
          sessionID: "ses_123",
          delta: "Try the narrow fix first.",
        },
      },
      ctx,
    ),
    {
      event: "agent.thinking",
      data: {
        sessionID: "ses_123",
        thinking: "Try the narrow fix first.",
        content: [{ type: "thinking", text: "Try the narrow fix first." }],
        model: "claude-sonnet-4-6",
      },
    },
  );
});

test("pending tool updates include stable id and name without empty input", () => {
  assert.deepEqual(
    translateOpencodeEvent(
      {
        type: "message.part.updated",
        properties: {
          sessionID: "ses_123",
          part: {
            id: "part_tool_1",
            type: "tool",
            tool: "sandbox_exec",
            state: {
              status: "pending",
              input: {},
            },
          },
        },
      },
      ctx,
    ),
    {
      event: "agent.tool_use",
      data: {
        id: "part_tool_1",
        name: "sandbox_exec",
        tool: "sandbox_exec",
        status: "pending",
      },
    },
  );
});

test("running tool updates include the current input", () => {
  assert.deepEqual(
    translateOpencodeEvent(
      {
        type: "message.part.updated",
        properties: {
          sessionID: "ses_123",
          part: {
            id: "part_tool_1",
            type: "tool",
            tool: "sandbox_exec",
            state: {
              status: "running",
              input: { command: "echo \"hello world\"" },
            },
          },
        },
      },
      ctx,
    ),
    {
      event: "agent.tool_use",
      data: {
        id: "part_tool_1",
        name: "sandbox_exec",
        tool: "sandbox_exec",
        input: { command: "echo \"hello world\"" },
        status: "running",
      },
    },
  );
});

test("completed tool updates translate to agent.tool_result with output", () => {
  assert.deepEqual(
    translateOpencodeEvent(
      {
        type: "message.part.updated",
        properties: {
          sessionID: "ses_123",
          part: {
            id: "part_tool_1",
            type: "tool",
            tool: "sandbox_exec",
            state: {
              status: "completed",
              input: { command: "echo \"hello world\"" },
              output: "hello world\n",
            },
          },
        },
      },
      ctx,
    ),
    {
      event: "agent.tool_result",
      data: {
        tool_use_id: "part_tool_1",
        name: "sandbox_exec",
        tool: "sandbox_exec",
        content: [{ type: "text", text: "hello world\n" }],
        output: "hello world\n",
      },
    },
  );
});

test("events for another session are dropped", () => {
  assert.equal(
    translateOpencodeEvent(
      {
        type: "thinking_delta",
        properties: {
          sessionID: "ses_other",
          delta: { thinking: "not this session" },
        },
      },
      ctx,
    ),
    null,
  );
});

test("message.part.delta with raw ids carries them through", () => {
  const out = translateOpencodeEvent(
    {
      type: "message.part.delta",
      properties: {
        id: "ev_001",
        messageID: "msg_001",
        partID: "part_001",
        sessionID: "ses_123",
        delta: { text: "hi" },
      },
    },
    ctx,
  );
  assert.equal(out.event, "agent.message");
  assert.equal(out.data.id, "ev_001");
  assert.equal(out.data.messageID, "msg_001");
  assert.equal(out.data.partID, "part_001");
  assert.equal(out.data.sessionID, "ses_123");
  assert.deepEqual(out.data.content, [{ type: "text", text: "hi" }]);
});

test("two deltas with different ids are distinct events", () => {
  const make = (id, text) =>
    translateOpencodeEvent(
      {
        type: "message.part.delta",
        properties: { id, sessionID: "ses_123", delta: { text } },
      },
      ctx,
    );
  const a = make("ev_001", "foo");
  const b = make("ev_002", "bar");
  assert.notEqual(a.data.id, b.data.id);
  assert.notEqual(a.data.content[0].text, b.data.content[0].text);
});

test("message.part.updated text returns null (no double-send)", () => {
  assert.equal(
    translateOpencodeEvent(
      {
        type: "message.part.updated",
        properties: {
          sessionID: "ses_123",
          part: { type: "text", text: "full message" },
        },
      },
      ctx,
    ),
    null,
  );
});
