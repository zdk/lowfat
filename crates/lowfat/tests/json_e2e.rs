//! End-to-end JSON check for the real run path.
//!
//! Stub commands print JSON fixtures. `lowfat <cmd>` filters them through
//! the bundled filters and the plugins in `test-fixtures/`, and every
//! result is parsed again. A filter may shrink JSON, but what reaches the
//! agent must still parse the way the raw output did. With `LOWFAT_PIPED`
//! set, JSON must come out byte-exact.
#![cfg(unix)]

use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// How the raw fixture parses — the filtered output must parse the same way.
#[derive(Clone, Copy)]
enum Kind {
    Single,
    NdJson,
    /// Pretty documents printed back to back (`go list -json`).
    Stream,
    /// JSON with warning lines around it.
    Text,
}

fn pod(i: usize) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": format!("web-{i}"),
            "annotations": {"last-applied": "{\"kind\": \"Pod\", \"x\": [1, 2, 3]}"},
            "managedFields": [{"manager": "kubectl", "fieldsV1": {"f:metadata": {}}}],
            "labels": {"app": "web"},
        },
        "spec": {"containers": [{
            "name": "c",
            "env": (0..12).map(|j| json!({"name": format!("E{j}"), "value": "v"})).collect::<Vec<_>>(),
        }]},
        "status": {"phase": "Running"},
    })
}

fn fixtures() -> Vec<(&'static str, Kind, String)> {
    let pretty = |v: &Value| serde_json::to_string_pretty(v).unwrap() + "\n";
    let big = json!({"kind": "List", "items": (0..300).map(pod).collect::<Vec<_>>()});
    let deep = (0..12).fold(
        json!({"leaf": true}),
        |acc, n| json!({"level": n, "child": acc}),
    );
    let tricky = json!({
        "unicode": "héllo — 日本語 🎉 end",
        "quote": "say \"hi\" \\ back",
        "newline": "a\nb\tc",
        "long": "x".repeat(5000),
        "big": u64::MAX,
        "float": 1e-7,
        "deep": deep,
        "empty": [{}, [], "", null],
        // Strings that look like lines a filter would keep or drop.
        "lines": ["error: boom", "managedFields:", "# comment", "---", "diff --git a b"],
        "wide": (0..60).map(|i| (format!("key{i:02}"), json!(i))).collect::<serde_json::Map<_, _>>(),
        "mixed": [1, "two", {"three": 3}, [4], null, true],
    });
    let ndjson: String = (0..600)
        .map(|i| json!({"Action": "output", "Package": format!("p/{}", i % 7)}).to_string() + "\n")
        .collect();
    let stream: String = (0..40).map(|i| pretty(&pod(i))).collect();
    vec![
        ("small_pretty", Kind::Single, pretty(&pod(0))),
        ("big_pretty", Kind::Single, pretty(&big)),
        ("big_min", Kind::Single, big.to_string() + "\n"),
        (
            "array_pretty",
            Kind::Single,
            pretty(&json!((0..200).map(pod).collect::<Vec<_>>())),
        ),
        ("tricky", Kind::Single, pretty(&tricky)),
        ("empty_obj", Kind::Single, "{}\n".into()),
        ("empty_arr", Kind::Single, "[]\n".into()),
        ("ndjson", Kind::NdJson, ndjson),
        ("stream", Kind::Stream, stream),
        (
            "trailer_warn",
            Kind::Text,
            pretty(&big) + "Warning: flag is deprecated\n",
        ),
        (
            "trailer_bracket",
            Kind::Text,
            pretty(&big) + "[WARN] flag is deprecated\n",
        ),
        (
            "leading_warn",
            Kind::Text,
            "Warning: flag is deprecated\n".to_string() + &pretty(&big),
        ),
    ]
}

/// Bundled filters, plus the plugins under `test-fixtures/plugins`.
const COMMANDS: &[(&str, &[&str])] = &[
    ("git", &["status", "diff", "log", "show", "config"]),
    (
        "docker",
        &[
            "ps", "images", "logs", "build", "pull", "compose", "inspect",
        ],
    ),
    ("ls", &[""]),
    ("find", &[""]),
    ("grep", &[""]),
    ("tree", &[""]),
    ("kubectl", &["get", "describe", "logs", "apply"]),
    ("go", &["test", "build", "list"]),
    ("cargo", &["build", "test", "metadata"]),
    ("npm", &["install", "ls", "audit", "view"]),
];

const FLAGS: [&[&str]; 2] = [&[], &["-o", "json"]];

fn all_values_parse(text: &str) -> bool {
    let mut n = 0;
    for v in serde_json::Deserializer::from_str(text).into_iter::<Value>() {
        if v.is_err() {
            return false;
        }
        n += 1;
    }
    n > 0
}

/// Empty string when `out` still parses the way the raw did.
fn check(kind: Kind, out: &str) -> &'static str {
    if out.trim().is_empty() {
        return "empty output";
    }
    let ok = match kind {
        Kind::Single => serde_json::from_str::<Value>(out).is_ok(),
        Kind::NdJson => out
            .lines()
            .filter(|l| !l.trim().is_empty())
            .all(|l| serde_json::from_str::<Value>(l).is_ok()),
        Kind::Stream => all_values_parse(out),
        // The document between the warning lines must still parse.
        Kind::Text => out
            .lines()
            .position(|l| l.starts_with(['{', '[']))
            .is_some_and(|i| {
                let body = out.lines().skip(i).collect::<Vec<_>>().join("\n");
                let mut it = serde_json::Deserializer::from_str(&body).into_iter::<Value>();
                matches!(it.next(), Some(Ok(_)))
            }),
    };
    if ok {
        ""
    } else {
        "invalid JSON"
    }
}

struct Case {
    cmd: &'static str,
    sub: &'static str,
    flags: &'static [&'static str],
    level: &'static str,
    fixture: usize,
    piped: bool,
    exit: i32,
}

fn run_case(c: &Case, fx: &[(&str, Kind, String)], tmp: &Path, path: &str) -> Option<String> {
    let (name, kind, raw) = &fx[c.fixture];
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lowfat"));
    cmd.arg(c.cmd);
    if !c.sub.is_empty() {
        cmd.arg(c.sub);
    }
    cmd.args(c.flags)
        .current_dir(tmp)
        .env("PATH", path)
        .env("HOME", tmp)
        .env("LOWFAT_HOME", tmp.join("home"))
        .env("LOWFAT_DATA", tmp.join("data"))
        .env("LOWFAT_LEVEL", c.level)
        .env("STUB_FX", tmp.join(name))
        .env("STUB_EXIT", c.exit.to_string())
        .env_remove("LOWFAT_PIPED");
    if c.piped {
        cmd.env("LOWFAT_PIPED", "1");
    }
    let output = cmd.output().expect("run lowfat");
    let out = String::from_utf8_lossy(&output.stdout);

    let err = if output.status.code() != Some(c.exit) {
        "exit code changed"
    } else if c.piped && out != *raw {
        "piped output not byte-exact"
    } else {
        check(*kind, &out)
    };
    (!err.is_empty()).then(|| {
        let head: String = out.chars().take(80).collect();
        format!(
            "{err}: {} {} {:?} level={} piped={} exit={} fixture={name} -> {head:?}",
            c.cmd, c.sub, c.flags, c.level, c.piped, c.exit
        )
    })
}

#[test]
fn json_stays_parsable_through_every_filter() {
    let tmp = tempfile::tempdir().unwrap();
    let tmp = tmp.path();
    let fx = fixtures();

    // Stub commands print the fixture named by $STUB_FX.
    let bin = tmp.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for (name, _, text) in &fx {
        std::fs::write(tmp.join(name), text).unwrap();
    }
    for (cmd, _) in COMMANDS {
        let stub = bin.join(cmd);
        std::fs::write(&stub, "#!/bin/sh\ncat \"$STUB_FX\"\nexit \"$STUB_EXIT\"\n").unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());

    // Own home and data dirs, so the user's plugins and history stay untouched.
    std::fs::create_dir_all(tmp.join("home")).unwrap();
    let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/plugins");
    std::os::unix::fs::symlink(plugins, tmp.join("home/plugins")).unwrap();

    let mut cases = Vec::new();
    for (cmd, subs) in COMMANDS {
        for sub in *subs {
            for fixture in 0..fx.len() {
                for flags in FLAGS {
                    for level in ["ultra", "full", "lite"] {
                        let (piped, exit) = (false, 0);
                        cases.push(Case {
                            cmd,
                            sub,
                            flags,
                            level,
                            fixture,
                            piped,
                            exit,
                        });
                    }
                }
                let (flags, level) = (FLAGS[0], "ultra");
                cases.push(Case {
                    cmd,
                    sub,
                    flags,
                    level,
                    fixture,
                    piped: true,
                    exit: 0,
                });
                cases.push(Case {
                    cmd,
                    sub,
                    flags,
                    level,
                    fixture,
                    piped: false,
                    exit: 1,
                });
            }
        }
    }

    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                while let Some(case) = cases.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Some(f) = run_case(case, &fx, tmp, &path) {
                        failures.lock().unwrap().push(f);
                    }
                }
            });
        }
    });

    let failures = failures.into_inner().unwrap();
    assert!(
        failures.is_empty(),
        "{} of {} runs broke JSON:\n{}",
        failures.len(),
        cases.len(),
        failures
            .iter()
            .take(15)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
