# Named rules example

`filter.lf` handles `describe` with three named rules, then a catch-all.
Each rule lists what it matches as fields. The first rule whose fields all
hold runs.

```text
describe-json     describe with -o json or --output json → raw
describe-failed   describe that exited non-zero          → raw
describe          any other describe                     → drop + head
```

Every block below is a [runme](https://runme.dev) cell. Run one with
`runme run <name>` from this folder, or copy the command.

Plain `describe` hits the last named rule:

```sh {"name":"named-rules-default"}
lowfat filter filter.lf --sub describe < sample-describe.txt
```

With `-o json`, the first rule wins and the output passes through:

```sh {"name":"named-rules-json"}
lowfat filter filter.lf --sub describe --args="-o json" < sample-describe.txt
```

A failed command skips to `describe-failed`:

```sh {"name":"named-rules-failed"}
lowfat filter filter.lf --sub describe --exit 1 < sample-describe.txt
```

`--explain` prints the name of the rule that ran:

```sh {"name":"named-rules-explain"}
lowfat filter filter.lf --sub describe --explain < sample-describe.txt
```

See [docs/PLUGINS.md](../../docs/PLUGINS.md#named-rules--rule-name) for the
field reference.
