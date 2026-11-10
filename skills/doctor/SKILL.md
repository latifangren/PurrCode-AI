---
name: doctor
description: Diagnosing PurrCode AI runtime issues, provider health, model context bounds, and environment readiness.
---

# Doctor Skill

## Runtime Health Checks
1. **Configuration Validity:** Validate JSONC syntax and provider declarations.
2. **Endpoint Reachability:** Ping configured endpoints (`/v1/models` or tags).
3. **Context Window Check:** Ensure context window meets or exceeds the 64K token minimum requirement.
4. **Tool Integrity:** Verify shell environment and MCP child process health.
