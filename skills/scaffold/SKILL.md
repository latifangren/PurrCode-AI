---
name: scaffold
description: Load when bootstrapping a brand-new greenfield project, adding a new Cargo workspace crate, or establishing modular directory layouts. Do NOT load for bug fixes in existing files, verification-only runs, or read-only audits.
---

# Scaffold Skill

## Greenfield Rules
1. **Workspace First:** Use workspace layouts with one-way dependency chains.
2. **File Size Limit:** Keep each source file under 800–1000 lines; split into submodules early before complexity grows.
3. **No Hardcode:** Externalize all endpoints, models, and credentials into configuration and environment variables.
