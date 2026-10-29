# Nisaba — instrucciones para agentes (Codex, Claude Code u otros)

Empieza siempre por `HANDOFF.md`: estado, comandos y siguientes pasos.

- Spec (autoridad): `docs/superpowers/specs/2026-10-29-nisaba-design.md`. La §14 recoge decisiones de implementación que prevalecen sobre secciones anteriores.
- Plan A (núcleo + CLI + MCP, hecho): `docs/superpowers/plans/2026-10-29-nisaba-v0.1-nucleo-cli-mcp.md`.
- Plan B (GUI, por hacer): `docs/superpowers/plans/2026-10-29-nisaba-v0.1-gui.md`.

Reglas:
- Nunca escribir, mover ni borrar en `~/.claude` ni `~/.codex`. Nisaba solo lee esas carpetas.
- Fixtures inventados a mano; nunca copiar sesiones reales al repo (será público).
- `cargo test --workspace` en verde y sin warnings antes de cada commit. Commits en español, convencionales.
- Haz `git add` solo de los archivos que tocaste. No hagas push ni merge sin que Brian lo pida.
- `stdout` de `nisaba mcp` es solo JSON-RPC.
- Brian aprende construyendo: explica el porqué de cada decisión, desde cero y sin condescendencia.
