# Rustricted eval — run 001

*Bug in source* — agent typed the known-bad pattern. *Caught* — `rustricted check` flagged it (only meaningful in the rustricted condition). *Shipped* — bug present **and** dialect did not catch it. Shipped is the headline column.

| Task | Condition | Trials | Bug in source | Caught | Shipped |
|------|-----------|--------|---------------|--------|---------|
| `01-duration` | vanilla | 1 | 1/1 (100%) | — | **1/1 (100%)** |
| `01-duration` | rustricted | 1 | 0/1 (0%) | 0/1 (0%) | **0/1 (0%)** |
| `02-area` | vanilla | 1 | 1/1 (100%) | — | **1/1 (100%)** |
| `02-area` | rustricted | 1 | 0/1 (0%) | 0/1 (0%) | **0/1 (0%)** |
| `03-config` | vanilla | 1 | 0/1 (0%) | — | **0/1 (0%)** |
| `03-config` | rustricted | 1 | 0/1 (0%) | 0/1 (0%) | **0/1 (0%)** |
| `04-cast` | vanilla | 1 | 0/1 (0%) | — | **0/1 (0%)** |
| `04-cast` | rustricted | 1 | 0/1 (0%) | 0/1 (0%) | **0/1 (0%)** |
| `05-glob` | vanilla | 1 | 0/1 (0%) | — | **0/1 (0%)** |
| `05-glob` | rustricted | 1 | 0/1 (0%) | 0/1 (0%) | **0/1 (0%)** |

## Totals

- **Vanilla**: 2/5 (40%) had the bug in source; 2/5 (40%) shipped.
- **Rustricted**: 0/5 (0%) had the bug in source; 0/5 (0%) caught; **0/5 (0%) shipped**.

- **Bug-ship reduction**: vanilla ships at 40%, rustricted at 0% — 100% fewer bugs reach prod.
