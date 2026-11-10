---
name: doctor
description: Load ONLY when diagnosing PurrCode AI installation, configuration, provider reachability, 64K context window gates, or MCP child process health. Do NOT load for ordinary repository code bugs, user application tests, or third-party project troubleshooting.
---

# Doctor Skill

## Runtime Health Checks
1. **Configuration Validity:** Validate JSONC syntax and provider declarations.
2. **Endpoint Reachability:** Ping configured endpoints (`/v1/models` or tags).
3. **Context Window Check:** Ensure context window meets or exceeds the 64K token minimum requirement.
4. **Tool Integrity:** Verify shell environment and MCP child process health.
