---
name: webapp-testing
description: Load when verifying runnable web applications, HTTP/SSE endpoints, or interactive browser UIs via headless browser MCP tools (Playwright) and reachability probes. Do NOT load for CLI-only unit tests, static documentation, or when the user explicitly opts out of testing.
---

# WebApp Testing Skill

## Testing Methodology
1. **Bounded Reachability:** Validate HTTP status codes and response headers first.
2. **Headless Execution:** Use Playwright or headless browser tools via MCP; never steal window focus.
3. **Causal Evidence:** Assert actual DOM state changes following user interactions rather than passive page loads.
