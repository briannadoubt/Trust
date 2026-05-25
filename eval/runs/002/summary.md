# Rustricted eval — run 002

*Bug in source* — agent typed the known-bad pattern. *Caught* — `rustricted check` flagged it (only meaningful in the rustricted condition). *Shipped* — bug present **and** dialect did not catch it. Shipped is the headline column.

| Task | Condition | Trials | Bug in source | Caught | Shipped |
|------|-----------|--------|---------------|--------|---------|
| `11-pipeline` | vanilla | 3 | 3/3 (100%) | — | **3/3 (100%)** |
| `11-pipeline` | rustricted | 3 | 3/3 (100%) | 3/3 (100%) | **0/3 (0%)** |
| `12-result-chain` | vanilla | 3 | 0/3 (0%) | — | **0/3 (0%)** |
| `12-result-chain` | rustricted | 3 | 0/3 (0%) | 0/3 (0%) | **0/3 (0%)** |
| `13-numeric` | vanilla | 3 | 3/3 (100%) | — | **3/3 (100%)** |
| `13-numeric` | rustricted | 3 | 3/3 (100%) | 3/3 (100%) | **0/3 (0%)** |
| `14-imports` | vanilla | 3 | 0/3 (0%) | — | **0/3 (0%)** |
| `14-imports` | rustricted | 3 | 0/3 (0%) | 0/3 (0%) | **0/3 (0%)** |
| `15-many-points` | vanilla | 3 | 3/3 (100%) | — | **3/3 (100%)** |
| `15-many-points` | rustricted | 3 | 3/3 (100%) | 3/3 (100%) | **0/3 (0%)** |

## Totals

- **Vanilla**: 9/15 (60%) had the bug in source; 9/15 (60%) shipped.
- **Rustricted**: 9/15 (60%) had the bug in source; 9/15 (60%) caught; **0/15 (0%) shipped**.

- **Bug-ship reduction**: vanilla ships at 60%, rustricted at 0% — 100% fewer bugs reach prod.
