# Run 005 — Cross-provider eval (GPT-4o)

**Status:** infrastructure ready; run pending API access.

To execute:

```sh
OPENAI_API_KEY=<your-key> python3 eval/run_cross_provider.py \
    --provider openai \
    --model gpt-4o \
    --run 005 \
    --tasks 11,12,13,14,15 \
    --trials 3
python3 eval/score.py 005
python3 eval/summarize.py 005
```

For Gemini (run 006):

```sh
GOOGLE_API_KEY=<your-key> python3 eval/run_cross_provider.py \
    --provider gemini \
    --model gemini-1.5-pro \
    --run 006 \
    --tasks 11,12,13,14,15 \
    --trials 3
```

**Hypothesis:** GPT-4o makes fewer R0042/R0001/R0003 bugs than Haiku in
vanilla Rust — but when it does make them, Trust still catches them.
The ship rate (bug present AND dialect missed it) should stay near zero
regardless of model capability.
