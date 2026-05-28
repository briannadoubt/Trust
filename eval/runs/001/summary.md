# Trust eval — run 001

*Bug in source* — agent typed the known-bad pattern. *Caught* — `trust check` flagged it (only meaningful in the trust condition). *Shipped* — bug present **and** dialect did not catch it. Shipped is the headline column.

| Task | Condition | Trials | Bug in source | Caught | Shipped |
|------|-----------|--------|---------------|--------|---------|
| `01-duration` | vanilla | 1 | 1/1 (100%) | — | **1/1 (100%)** |
| `01-duration` | trust | 1 | 0/1 (0%) | 0/1 (0%) | **0/1 (0%)** |
| `02-area` | vanilla | 1 | 1/1 (100%) | — | **1/1 (100%)** |
| `02-area` | trust | 1 | 0/1 (0%) | 0/1 (0%) | **0/1 (0%)** |
| `03-config` | vanilla | 1 | 0/1 (0%) | — | **0/1 (0%)** |
| `03-config` | trust | 1 | 0/1 (0%) | 0/1 (0%) | **0/1 (0%)** |
| `04-cast` | vanilla | 1 | 0/1 (0%) | — | **0/1 (0%)** |
| `04-cast` | trust | 1 | 0/1 (0%) | 0/1 (0%) | **0/1 (0%)** |
| `05-glob` | vanilla | 1 | 0/1 (0%) | — | **0/1 (0%)** |
| `05-glob` | trust | 1 | 0/1 (0%) | 0/1 (0%) | **0/1 (0%)** |

## Totals

- **Vanilla**: 2/5 (40%) had the bug in source; 2/5 (40%) shipped.
- **Trust**: 0/5 (0%) had the bug in source; 0/5 (0%) caught; **0/5 (0%) shipped**.

- **Bug-ship reduction**: vanilla ships at 40%, trust at 0% — 100% fewer bugs reach prod.
