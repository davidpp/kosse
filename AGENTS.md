# Stackscan agent entry point

Read [blueprint/README.md](blueprint/README.md), then the product, architecture, verification, and loop documents before implementation. Read the selected issue with `kabane context <id>`.

## Authority and scope

- User instructions win. Blueprint owns requirements/contracts; Kabane owns execution state and evidence.
- Work only in this repository and dedicated test/install sandboxes. Preserve unrelated WIP.
- This setup authorizes preparing the project and plan, not automatically executing every child now. A later explicit implementation-loop invocation authorizes bounded local implementation and tests.
- Do not copy PortDoc source, assets, text, or skills. Consult library documentation and independent requirements. This context has seen PortDoc source; do not claim strict clean-room provenance.
- Personal preferences were read from Second Brain's maintained stacks/coding notes. See blueprint references; Rust remains an explicit user choice.
- Consult current library docs before selecting versions or scaffolding. Use supported generators. Use Impeccable for UI work and applicable React/browser skills.

## Working conventions

Rust expected failures use typed Result; no panics on process races or unreadable metadata. Strict TypeScript, no `any` escape hatches or ignored type errors. Keep modules small without speculative plugin layers.
All views use the same service model and action policy. One scanner per running application; no scanner in components or request handlers.
Never stop a real user service during tests. Fixture processes are recorded and cleaned up by identity, never by broad process-name matching.
Local hooks/CI use the same non-mutating checks. Do not claim placeholder commands work.
No commits, remote creation, push, publishing, signing, global agent changes, or real login-item installation without explicit approval. Stage explicit paths only if staging is authorized.
Record evidence and blockers in Kabane. Do not maintain a second checked-off backlog in Markdown.

## Commands

None exist yet. The scaffold issue must implement and verify the command contract in blueprint/architecture.md, then replace this section with actual commands. Until then, do not report build/test/install completion.
