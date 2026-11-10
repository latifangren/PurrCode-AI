---
name: webapp-testing
description: Web application testing, headless browser validation via Playwright MCP, and endpoint reachability.
---

# WebApp Testing Skill

## Testing Methodology
1. **Bounded Reachability:** Validate HTTP status codes and response headers first.
2. **Headless Execution:** Use Playwright or headless browser tools via MCP; never steal window focus.
3. **Causal Evidence:** Assert actual DOM state changes following user interactions rather than passive page loads.
