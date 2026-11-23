# Security policy

## Scope

Security fixes target the current mainline and the most recent published version. Historical test packages (`v1.1.x`) are not patched separately unless the issue affects the current kernel contract or is known to be exploited in the wild.

## Reporting a vulnerability

Please **do not** post reproducible attack details, keys, private logs, or user data in a public issue. Prefer **Security → Report a vulnerability** on the GitHub repository (private vulnerability reporting). If that entry point is unavailable, contact the repository owner privately through their GitHub profile and include a minimal reproduction and impact.

A useful report includes:

- affected version and platform (Linux / Windows, x64);
- trigger path (command, session, plugin, WebUI request);
- expected behavior and actual behavior;
- whether it involves path escape, permission bypass, private-network fetch (SSRF), command execution, or credential disclosure.

## Handling

- After confirmation, fix mainline first, then decide whether a patch release is needed; do not publish details before the fix ships.
- Do not ask reporters for real keys, personal files, or access to third-party systems.
- Public-web-search issues (site parser changes, rate limits, robots/ToS) are treated as functional defects, not security vulnerabilities.

## Security boundary

FeatherAgent is a local agent kernel and listens on loopback by default. Plugins run in separate processes; tool permissions are decided per call by the governance layer. Do not expose `serve` directly to the public internet, and never commit `.env`, `config.json`, or runtime logs containing keys.
