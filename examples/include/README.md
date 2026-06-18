# `include` example

`uv run pytest` produces the same output as `pytest`, so its wrapper filter
reuses pytest's compaction macro instead of copying it.

```
lib/pytest.lf     defines `compact-pytest` (and is a runnable pytest filter)
uv-pytest.lf      includes lib/pytest.lf, reuses `compact-pytest`
```

Run it:

```sh
lowfat filter uv-pytest.lf --sub run < sample-pytest.txt
```

The wrapper keeps the verdicts and the summary banner, dropping the noise — all
from a macro defined in another file. See `--explain` for per-stage counts:

```sh
lowfat filter uv-pytest.lf --sub run --explain < sample-pytest.txt
```

See [docs/PLUGINS.md](../../docs/PLUGINS.md#sharing-macros-include) for the rules
(relative paths, transitive includes, local override, collision errors).
