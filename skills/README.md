# skills/

Packaged Claude Code skills for working with Trust. Each subdirectory is a
self-contained skill: a `SKILL.md` whose frontmatter description auto-triggers
it when the task matches.

- **writing-trust/** — makes an agent fluent in the Trust dialect on first
  contact: setup, syntax, the build/fix iteration loop, and the full rule table.

Install paths:

1. **Per-project:** copy into a project's `.claude/skills/` and it auto-loads
   for sessions in that project.
2. **Personal (all projects):** `cp -r skills/writing-trust ~/.claude/skills/`
3. **Plugin/marketplace:** packaged install coming with RT-98.
