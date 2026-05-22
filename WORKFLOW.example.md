---
tracker:
  kind: linear
  project_slug_id: CAD
  token: "$LINEAR_API_KEY"

poll:
  interval_ms: 5000

workspace:
  root: "/tmp/cadenza/workspaces"

codex:
  command: "codex app-server --listen stdio://"
  schema_sha256: "TODO-generate-and-pin"
  turn_timeout_ms: 600000

orchestrator:
  max_concurrent_agents: 2
  active_states: ["todo", "in progress"]
  terminal_states: ["done", "canceled", "duplicate"]

hooks:
  after_create:
    timeout_ms: 30000
    command: "git init"
---
You are working on Linear issue {{ issue.identifier }}: {{ issue.title }}.

Rules:
- Work only inside the assigned workspace.
- Use available tools for tracker writes; do not assume the orchestrator writes tickets for you.
- Summarize any handoff state clearly.

Issue description:
{{ issue.description | default("", true) }}
