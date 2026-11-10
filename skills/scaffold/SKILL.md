---
name: scaffold
description: Guidelines for greenfield project bootstrapping, modular layout, and workspace structure.
---

# Scaffold Skill

## Greenfield Rules
1. **Workspace First:** Use workspace layouts with one-way dependency chains.
2. **File Size Limit:** Keep each source file under 800–1000 lines; split into submodules early before complexity grows.
3. **No Hardcode:** Externalize all endpoints, models, and credentials into configuration and environment variables.
