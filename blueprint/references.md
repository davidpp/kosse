# References and provenance

## Owner preferences

Read 2026-10-09 from the local Second Brain replica after its REST endpoint was unavailable:
- `~/Documents/second-brain/knowledge/preferences/_overview.md`
- `~/Documents/second-brain/knowledge/preferences/stacks.md`
- `~/Documents/second-brain/knowledge/preferences/coding.md`

Rust backend is explicit owner direction. Maintained preferences select Bun/Vite/React, TanStack Router and Query, Tailwind v4/shadcn, Oxlint/Oxfmt, Lefthook, strict types, full setup and real entrypoint verification. Cloudflare is a hosting preference, not a reason to add hosting to a local tool. Release automation specifics are not an established universal preference.

## Candidate library documentation

Resolve exact current APIs/versions at implementation; this is not a tested dependency lock:
- https://docs.rs/listeners/latest/listeners/
- https://docs.rs/netstat2/latest/netstat2/
- https://docs.rs/sysinfo/latest/sysinfo/
- https://docs.rs/service-manager/latest/service_manager/
- https://docs.rs/auto-launch/latest/auto_launch/
- https://docs.rs/tray-icon/latest/tray_icon/
- https://docs.rs/axum/latest/axum/
- https://docs.rs/ts-rs/latest/ts_rs/
- https://vite.dev/guide/
- https://tanstack.com/router/latest/docs/framework/react/overview
- https://tanstack.com/query/latest/docs/framework/react/overview
- https://ui.shadcn.com/docs/installation/vite
- https://oxc.rs/docs/guide/usage/oxlint
- https://oxc.rs/docs/guide/usage/oxfmt
- https://lefthook.dev/

## Optional managed-service companions

- https://github.com/vercel-labs/portless — named .localhost URLs, HTTPS, launch-time port allocation, optional portless.json. Reviewed documentation, not locally tested.
- https://www.localcan.com/docs/configuration/projects — project/service targets and endpoint configuration; centrally stored YAML.

## Inspiration and boundaries

Tray Stack inspired the product concept; PortDoc/Portbook informed the earlier feasibility comparison. PortDoc's “AI Coding Blueprint” names a workflow overlay, but its original upstream source was not confirmed.

The earlier research inspected PortDoc source. Accordingly this project makes no strict clean-room claim. No PortDoc source/assets/skill text are copied into Stackscan; implementation agents should work from these independent requirements and licensed library documentation, not consult PortDoc implementation for translation. Obtain appropriate advice before any distribution if provenance concerns arise.

The current project is planning only. No application license selected, remote created, dependency installed, or build/runtime behavior verified by this planning step.
