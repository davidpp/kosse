# Stackscan

A local homepage and native macOS tray menu for the development services you already run: discover, identify, open, inspect, stop.

**Current state:** planning repository only. Application code, build commands and installation are not implemented yet. No remote, release or startup service has been created.

- [Blueprint](blueprint/README.md)
- [Implementation plan](blueprint/implementation-plan.md)
- [Loop instructions](blueprint/loop.md)
- Kabane scope: `jake://scope/stackscan`
- Epic: **JSTA-1**

Start an implementation session in this repository, read `AGENTS.md`, then run `kabane context JSTA-1`. Follow dependency-ready children, not the epic itself.

Stack: Rust backend; Bun, Vite/React, TanStack Router/Query, Tailwind v4 and shadcn/ui frontend. macOS arm64 first. No cloud runtime.

This is an independently authored product. It is not a PortDoc fork or a certified clean-room rewrite. No PortDoc source, assets, or workflow skill files are included.
