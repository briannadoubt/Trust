# Trust eval — run 006

*Bug in source* — agent typed the known-bad pattern. *Caught* — `trust check` flagged it (only meaningful in the trust condition). *Shipped* — bug present **and** dialect did not catch it. Shipped is the headline column.

| Task | Condition | Trials | Bug in source | Caught | Shipped |
|------|-----------|--------|---------------|--------|---------|
| `11-pipeline` | vanilla | 3 | 3/3 (100%) | — | **3/3 (100%)** |
| `11-pipeline` | trust | 3 | 3/3 (100%) | 3/3 (100%) | **0/3 (0%)** |
| `12-result-chain` | vanilla | 3 | 0/3 (0%) | — | **0/3 (0%)** |
| `12-result-chain` | trust | 3 | 0/3 (0%) | 0/3 (0%) | **0/3 (0%)** |
| `13-numeric` | vanilla | 3 | 3/3 (100%) | — | **3/3 (100%)** |
| `13-numeric` | trust | 3 | 0/3 (0%) | 3/3 (100%) | **0/3 (0%)** |
| `14-imports` | vanilla | 3 | 0/3 (0%) | — | **0/3 (0%)** |
| `14-imports` | trust | 2 | 0/2 (0%) | 0/2 (0%) | **0/2 (0%)** |
| `15-many-points` | vanilla | 3 | 3/3 (100%) | — | **3/3 (100%)** |
| `15-many-points` | trust | 3 | 3/3 (100%) | 3/3 (100%) | **0/3 (0%)** |

## Totals

- **Vanilla**: 9/15 (60%) had the bug in source; 9/15 (60%) shipped.
- **Trust**: 6/14 (42%) had the bug in source; 9/14 (64%) caught; **0/14 (0%) shipped**.

- **Bug-ship reduction**: vanilla ships at 60%, trust at 0% — 100% fewer bugs reach prod.
