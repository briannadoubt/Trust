# Run 005 — Cross-provider eval (GPT-4o)

**Status:** complete. See [summary.md](summary.md) for the scored table.

Reproduce / re-run:

```sh
OPENAI_API_KEY=<your-key> python3 eval/run_cross_provider.py \
    --provider openai \
    --model gpt-4o \
    --run 005 \
    --tasks 11-pipeline,12-result-chain,13-numeric,14-imports,15-many-points \
    --trials 3
python3 eval/score.py 005
python3 eval/summarize.py 005
```

For Gemini (run 006):

```sh
GOOGLE_API_KEY=<your-key> python3 eval/run_cross_provider.py \
    --provider gemini \
    --model gemini-2.5-flash \
    --run 006 \
    --tasks 11-pipeline,12-result-chain,13-numeric,14-imports,15-many-points \
    --trials 3
```

Task ids are the full string ids from `eval/tasks.toml` (`11-pipeline`,
not `11`). The runner hard-errors on an unknown id rather than skipping it
(RT-51); existing `.rs` files are reused, so re-running is incremental.

`score.py` needs a release `trust` binary built under the wrapper — see
the header of `eval/score.py` for the one-liner. It refuses to run rather
than silently scoring 0 if the binary is missing (RT-53).

**Hypothesis:** GPT-4o makes fewer R0042/R0001/R0003 bugs than Haiku in
vanilla Rust — but when it does make them, Trust still catches them.
The ship rate (bug present AND dialect missed it) should stay near zero
regardless of model capability.
