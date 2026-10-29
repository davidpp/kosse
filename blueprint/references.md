# References and provenance

## Owner preferences

Read 2026-10-09 and re-checked 2026-10-28 from the maintainer's personal knowledge base (stack and coding-setup preference notes; not part of this repository).

Rust backend is explicit owner direction. Maintained preferences select Bun/Vite/React, TanStack Router and Query, Tailwind v4/shadcn, Oxlint/Oxfmt, Lefthook, strict types, full setup and real entrypoint verification. Cloudflare is a hosting preference, not a reason to add hosting to a local tool.

Owner-directed deviation (2026-10-28): because Kossé is resident and must stay nimble, the web UI drops shadcn/ui and web fonts and runs React APIs on the pre-1.0 [TanStack Redact](https://tanstack.com/redact/latest) runtime. TanStack Router was measured against the bundle budget in S01b (~28 kB gzipped) and not adopted. [Rune](https://github.com/anomalyco/rune) was evaluated for a native window and is watched for later; OpenTUI/ratatui terminal UIs were considered and not chosen. Release automation specifics are not an established universal preference.

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
- https://tanstack.com/query/latest/docs/framework/react/overview
- https://tanstack.com/redact/latest/docs/overview and https://tanstack.com/redact/latest/docs/installation
- https://docs.rs/wry/latest/wry/ and https://docs.rs/tao/latest/tao/ (dashboard window)
- https://oxc.rs/docs/guide/usage/oxlint
- https://oxc.rs/docs/guide/usage/oxfmt
- https://lefthook.dev/

## Optional managed-service companions

- https://github.com/vercel-labs/portless — named .localhost URLs, HTTPS, launch-time port allocation, optional portless.json. Reviewed documentation, not locally tested.
- https://www.localcan.com/docs/configuration/projects — project/service targets and endpoint configuration; centrally stored YAML.

## Inspiration and boundaries

Tray Stack inspired the product concept; PortDoc/Portbook informed the earlier feasibility comparison. PortDoc's “AI Coding Blueprint” names a workflow overlay, but its original upstream source was not confirmed.

The earlier research inspected PortDoc source. Accordingly this project makes no strict clean-room claim. No PortDoc source/assets/skill text are copied into Kossé; implementation agents should work from these independent requirements and licensed library documentation, not consult PortDoc implementation for translation. Obtain appropriate advice before any distribution if provenance concerns arise.

The planning step selected no dependencies and verified no runtime behavior; implementation evidence lives in `docs/evidence/`. The project is MIT-licensed and published at github.com/davidpp/kosse.
