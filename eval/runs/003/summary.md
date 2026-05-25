# Rustricted eval — run 003

*Bug in source* — agent typed the known-bad pattern. *Caught* — `rustricted check` flagged it (only meaningful in the rustricted condition). *Shipped* — bug present **and** dialect did not catch it. Shipped is the headline column.

| Task | Condition | Trials | Bug in source | Caught | Shipped |
|------|-----------|--------|---------------|--------|---------|
| `22-result-chain` | vanilla | 3 | 2/3 (66%) | — | **2/3 (66%)** |
| `22-result-chain` | rustricted | 3 | 0/3 (0%) | 0/3 (0%) | **0/3 (0%)** |
| `24-imports` | vanilla | 3 | 0/3 (0%) | — | **0/3 (0%)** |
| `24-imports` | rustricted | 3 | 0/3 (0%) | 0/3 (0%) | **0/3 (0%)** |

## Totals

- **Vanilla**: 2/6 (33%) had the bug in source; 2/6 (33%) shipped.
- **Rustricted**: 0/6 (0%) had the bug in source; 0/6 (0%) caught; **0/6 (0%) shipped**.

- **Bug-ship reduction**: vanilla ships at 33%, rustricted at 0% — 100% fewer bugs reach prod.
