use serde::Serialize;

use crate::sdk::agents::{CLAUDE_MANAGED_AGENTS, CURSOR, HERMES, OPENCODE};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RuntimeTool {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub enabled_by_default: bool,
}

pub fn runtime_tools(runtime: &str) -> &'static [RuntimeTool] {
    match runtime {
        CLAUDE_MANAGED_AGENTS | "claude_agents" => &CLAUDE_MANAGED_TOOLS,
        CURSOR | OPENCODE | HERMES => &[],
        _ => &[],
    }
}

const CLAUDE_MANAGED_TOOLS: [RuntimeTool; 8] = [
    RuntimeTool {
        id: "bash",
        name: "Shell",
        description: "Run shell commands in the agent environment.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "read",
        name: "Read files",
        description: "Read files from the agent environment.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "write",
        name: "Write files",
        description: "Create or overwrite files in the agent environment.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "edit",
        name: "Edit files",
        description: "Patch existing files in the agent environment.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "glob",
        name: "Find files",
        description: "Find files by glob pattern.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "grep",
        name: "Search files",
        description: "Search file contents by regular expression.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "web_fetch",
        name: "Fetch URL",
        description: "Fetch content from a URL.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "web_search",
        name: "Web search",
        description: "Search the web for information.",
        enabled_by_default: true,
    },
];
