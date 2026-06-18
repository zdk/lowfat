//! End-to-end tests for shell plugins.
//!
//! Each test creates a real filter.sh script in a temp directory,
//! builds a ProcessFilter, and runs it with realistic FilterInput
//! to verify the full stdin→env→script→stdout pipeline.

use lowfat_core::level::Level;
use lowfat_plugin::plugin::{FilterInput, FilterPlugin, PluginInfo};
use lowfat_runner::process::ProcessFilter;
use std::io::Write;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_plugin(name: &str, script: &str) -> (ProcessFilter, PathBuf) {
    let dir = std::env::temp_dir().join(format!("lowfat-e2e-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("filter.sh");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(script.as_bytes()).unwrap();

    let filter = ProcessFilter {
        info: PluginInfo {
            name: format!("{name}-compact"),
            version: "0.1.0".into(),
            commands: vec![name.into()],
            subcommands: vec![],
        },
        entry: path,
        base_dir: dir.clone(),
    };
    (filter, dir)
}

fn make_input(
    raw: &str,
    command: &str,
    subcommand: &str,
    args: Vec<&str>,
    level: Level,
    exit_code: i32,
) -> FilterInput {
    FilterInput {
        raw: raw.to_string(),
        command: command.to_string(),
        subcommand: subcommand.to_string(),
        args: args.into_iter().map(String::from).collect(),
        level,
        head_limit: level.head_limit(40),
        exit_code,
    }
}

// ---------------------------------------------------------------------------
// LOWFAT_ARGS — verify the full arg string reaches the script
// ---------------------------------------------------------------------------

#[test]
fn args_env_contains_full_argument_string() {
    let script = r#"#!/bin/sh
echo "ARGS=$LOWFAT_ARGS"
"#;
    let (filter, _dir) = temp_plugin("argtest", script);
    let input = make_input(
        "ignored",
        "kubectl",
        "get",
        vec!["get", "pods", "-n", "kube-system", "-o", "wide"],
        Level::Full,
        0,
    );
    let result = filter.filter(&input).unwrap();
    assert!(
        result.text.contains("ARGS=get pods -n kube-system -o wide"),
        "expected full args, got: {}",
        result.text
    );
}

#[test]
fn args_env_empty_when_no_args() {
    let script = r#"#!/bin/sh
echo "ARGS=[$LOWFAT_ARGS]"
"#;
    let (filter, _dir) = temp_plugin("noargs", script);
    let input = make_input("ignored", "git", "", vec![], Level::Full, 0);
    let result = filter.filter(&input).unwrap();
    assert!(
        result.text.contains("ARGS=[]"),
        "expected empty args, got: {}",
        result.text
    );
}

// ---------------------------------------------------------------------------
// Realistic kubectl plugin — resource-type-aware filtering via $LOWFAT_ARGS
// ---------------------------------------------------------------------------

const KUBECTL_SCRIPT: &str = r#"#!/bin/sh
RAW=$(cat)
LEVEL="${LOWFAT_LEVEL:-full}"
SUB="${LOWFAT_SUBCOMMAND}"
ARGS="${LOWFAT_ARGS}"

# Extract resource type from args
RESOURCE=""
for arg in $ARGS; do
  case "$arg" in
    "$SUB") continue ;;
    -*) continue ;;
    *) RESOURCE="$arg"; break ;;
  esac
done

# Passthrough structured output
case "$ARGS" in
  *"-o json"*|*"-o yaml"*)
    echo "$RAW"
    exit 0
    ;;
esac

case "$SUB" in
  get)
    case "$RESOURCE" in
      pods|po)
        if [ "$LEVEL" = "ultra" ]; then
          echo "$RAW" | awk 'NR==1 || !/Running/' | head -n 15
        else
          echo "$RAW" | head -n 30
        fi
        ;;
      events|ev)
        if [ "$LEVEL" = "ultra" ]; then
          echo "$RAW" | awk 'NR==1 || /Warning/' | head -n 15
        else
          echo "$RAW" | tail -n 30
        fi
        ;;
      *)
        echo "$RAW" | head -n 30
        ;;
    esac
    ;;
  *)
    echo "$RAW" | head -n 30
    ;;
esac
"#;

const KUBECTL_PODS_OUTPUT: &str = "\
NAME                    READY   STATUS    RESTARTS   AGE
nginx-abc123            1/1     Running   0          5d
redis-def456            1/1     Running   0          3d
crash-ghi789            0/1     CrashLoopBackOff   5   1h
pending-jkl012          0/1     Pending   0          30m
api-mno345              1/1     Running   0          7d";

const KUBECTL_EVENTS_OUTPUT: &str = "\
LAST SEEN   TYPE      REASON    OBJECT              MESSAGE
5m          Normal    Scheduled pod/nginx-abc123    Successfully assigned
4m          Normal    Pulled    pod/nginx-abc123    Container image pulled
3m          Warning   BackOff   pod/crash-ghi789    Back-off restarting
2m          Warning   Failed    pod/crash-ghi789    Error: CrashLoopBackOff
1m          Normal    Created   pod/api-mno345      Created container";

#[test]
fn kubectl_get_pods_ultra_filters_running() {
    let (filter, _dir) = temp_plugin("kubectl", KUBECTL_SCRIPT);
    let input = make_input(
        KUBECTL_PODS_OUTPUT,
        "kubectl",
        "get",
        vec!["get", "pods"],
        Level::Ultra,
        0,
    );
    let result = filter.filter(&input).unwrap();

    // Header should be present
    assert!(result.text.contains("NAME"), "header missing");
    // Non-Running pods should be present
    assert!(
        result.text.contains("CrashLoopBackOff"),
        "CrashLoop pod missing"
    );
    assert!(result.text.contains("Pending"), "Pending pod missing");
    // Running pods should be filtered out
    assert!(
        !result.text.contains("nginx-abc123"),
        "Running pod should be filtered"
    );
    assert!(
        !result.text.contains("redis-def456"),
        "Running pod should be filtered"
    );
}

#[test]
fn kubectl_get_pods_full_keeps_all() {
    let (filter, _dir) = temp_plugin("kubectl-full", KUBECTL_SCRIPT);
    let input = make_input(
        KUBECTL_PODS_OUTPUT,
        "kubectl",
        "get",
        vec!["get", "pods"],
        Level::Full,
        0,
    );
    let result = filter.filter(&input).unwrap();

    // Full level keeps everything
    assert!(
        result.text.contains("nginx-abc123"),
        "all pods should be present at full"
    );
    assert!(
        result.text.contains("CrashLoopBackOff"),
        "all pods should be present at full"
    );
}

#[test]
fn kubectl_get_events_ultra_warnings_only() {
    let (filter, _dir) = temp_plugin("kubectl-events", KUBECTL_SCRIPT);
    let input = make_input(
        KUBECTL_EVENTS_OUTPUT,
        "kubectl",
        "get",
        vec!["get", "events"],
        Level::Ultra,
        0,
    );
    let result = filter.filter(&input).unwrap();

    // Header + Warning events only
    assert!(result.text.contains("LAST SEEN"), "header missing");
    assert!(result.text.contains("BackOff"), "Warning event missing");
    assert!(result.text.contains("Failed"), "Warning event missing");
    // Normal events should be filtered
    assert!(
        !result.text.contains("Scheduled"),
        "Normal event should be filtered"
    );
    assert!(
        !result.text.contains("Pulled"),
        "Normal event should be filtered"
    );
}

#[test]
fn kubectl_get_pods_json_passthrough() {
    let json_output = r#"{"items": [{"metadata": {"name": "nginx"}}]}"#;
    let (filter, _dir) = temp_plugin("kubectl-json", KUBECTL_SCRIPT);
    let input = make_input(
        json_output,
        "kubectl",
        "get",
        vec!["get", "pods", "-o", "json"],
        Level::Ultra,
        0,
    );
    let result = filter.filter(&input).unwrap();

    // -o json should passthrough unmodified
    assert!(
        result.text.contains(r#""items""#),
        "JSON should pass through, got: {}",
        result.text
    );
}

#[test]
fn kubectl_get_pods_yaml_passthrough() {
    let yaml_output = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: nginx\n";
    let (filter, _dir) = temp_plugin("kubectl-yaml", KUBECTL_SCRIPT);
    let input = make_input(
        yaml_output,
        "kubectl",
        "get",
        vec!["get", "pods", "-o", "yaml"],
        Level::Ultra,
        0,
    );
    let result = filter.filter(&input).unwrap();
    assert!(
        result.text.contains("apiVersion"),
        "YAML should pass through, got: {}",
        result.text
    );
}

#[test]
fn kubectl_get_pods_with_namespace_flag() {
    // "get pods -n kube-system" — resource is "pods", not "-n"
    let (filter, _dir) = temp_plugin("kubectl-ns", KUBECTL_SCRIPT);
    let input = make_input(
        KUBECTL_PODS_OUTPUT,
        "kubectl",
        "get",
        vec!["get", "pods", "-n", "kube-system"],
        Level::Ultra,
        0,
    );
    let result = filter.filter(&input).unwrap();

    // Should correctly parse "pods" as resource despite -n flag
    assert!(
        result.text.contains("NAME"),
        "header missing — resource type not parsed correctly"
    );
    assert!(
        result.text.contains("CrashLoopBackOff"),
        "should use pods-specific filter"
    );
    assert!(
        !result.text.contains("nginx-abc123"),
        "Running pods should be filtered in ultra"
    );
}

// ---------------------------------------------------------------------------
// Realistic cargo plugin — verifies bundled plugin works end-to-end
// ---------------------------------------------------------------------------

const CARGO_SCRIPT: &str = r#"#!/bin/sh
RAW=$(cat)
LEVEL="${LOWFAT_LEVEL:-full}"
SUB="${LOWFAT_SUBCOMMAND}"

case "$SUB" in
  build|check)
    if [ "$LEVEL" = "ultra" ]; then
      ISSUES=$(echo "$RAW" | grep -E '^(error|warning)\b' | head -n 15)
      if [ -z "$ISSUES" ]; then
        echo "cargo $SUB: ok"
      else
        echo "$ISSUES"
      fi
    else
      LIMIT=$( [ "$LEVEL" = "lite" ] && echo 60 || echo 30 )
      echo "$RAW" | grep -vE '^\s*(Compiling|Downloading|Checking|Blocking|Updating|Locking) ' | head -n "$LIMIT"
    fi
    ;;
  test)
    if [ "$LEVEL" = "ultra" ]; then
      echo "$RAW" | grep -E '^(test result:|failures:|test .+ FAILED|     Running|FAILED)' | head -n 15
    else
      LIMIT=$( [ "$LEVEL" = "lite" ] && echo 60 || echo 30 )
      echo "$RAW" | grep -vE '^\s*(Compiling|Downloading|Checking|Blocking|Updating|Locking) |\.\.\.+ ok$' | head -n "$LIMIT"
    fi
    ;;
  *)
    echo "$RAW" | head -n 30
    ;;
esac
"#;

const CARGO_BUILD_OUTPUT: &str = "\
   Compiling serde v1.0.200
   Compiling tokio v1.37.0
   Compiling myapp v0.1.0 (/home/user/myapp)
warning: unused variable: `x`
  --> src/main.rs:10:9
   |
10 |     let x = 42;
   |         ^ help: if this is intentional, prefix it with an underscore: `_x`
   |
   = note: `#[warn(unused_variables)]` on by default

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.32s";

const CARGO_TEST_OUTPUT: &str = "\
   Compiling myapp v0.1.0
     Running unittests src/lib.rs (target/debug/deps/myapp-abc123)

running 5 tests
test tests::basic_add ... ok
test tests::basic_sub ... ok
test tests::edge_case ... ok
test tests::overflow_check ... FAILED
test tests::negative_check ... ok

failures:

---- tests::overflow_check stdout ----
thread 'tests::overflow_check' panicked at 'assertion failed'

failures:
    tests::overflow_check

test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out";

#[test]
fn cargo_build_ultra_shows_warnings_only() {
    let (filter, _dir) = temp_plugin("cargo-build", CARGO_SCRIPT);
    let input = make_input(
        CARGO_BUILD_OUTPUT,
        "cargo",
        "build",
        vec!["build"],
        Level::Ultra,
        0,
    );
    let result = filter.filter(&input).unwrap();

    assert!(
        result.text.contains("warning: unused variable"),
        "warning line missing"
    );
    assert!(
        !result.text.contains("Compiling serde"),
        "Compiling noise should be stripped"
    );
    assert!(
        !result.text.contains("Finished"),
        "Finished line should be stripped"
    );
}

#[test]
fn cargo_build_ultra_clean_shows_ok() {
    let clean_output = "\
   Compiling myapp v0.1.0
    Finished `dev` profile in 2.0s";
    let (filter, _dir) = temp_plugin("cargo-clean", CARGO_SCRIPT);
    let input = make_input(
        clean_output,
        "cargo",
        "build",
        vec!["build"],
        Level::Ultra,
        0,
    );
    let result = filter.filter(&input).unwrap();

    assert!(
        result.text.contains("cargo build: ok"),
        "clean build should show ok, got: {}",
        result.text
    );
}

#[test]
fn cargo_build_full_strips_compiling_noise() {
    let (filter, _dir) = temp_plugin("cargo-full", CARGO_SCRIPT);
    let input = make_input(
        CARGO_BUILD_OUTPUT,
        "cargo",
        "build",
        vec!["build"],
        Level::Full,
        0,
    );
    let result = filter.filter(&input).unwrap();

    assert!(
        !result.text.contains("Compiling serde"),
        "Compiling noise should be stripped"
    );
    assert!(
        result.text.contains("warning: unused variable"),
        "warnings should remain"
    );
    assert!(
        result.text.contains("Finished"),
        "Finished line should remain"
    );
}

#[test]
fn cargo_test_ultra_shows_failures_only() {
    let (filter, _dir) = temp_plugin("cargo-test", CARGO_SCRIPT);
    let input = make_input(
        CARGO_TEST_OUTPUT,
        "cargo",
        "test",
        vec!["test"],
        Level::Ultra,
        101,
    );
    let result = filter.filter(&input).unwrap();

    assert!(
        result.text.contains("test result: FAILED"),
        "result summary missing"
    );
    assert!(
        result.text.contains("overflow_check"),
        "failed test name missing"
    );
    // Individual ok tests should not appear
    assert!(
        !result.text.contains("basic_add"),
        "passing tests should be stripped"
    );
}

#[test]
fn cargo_test_full_strips_ok_tests() {
    let (filter, _dir) = temp_plugin("cargo-test-full", CARGO_SCRIPT);
    let input = make_input(
        CARGO_TEST_OUTPUT,
        "cargo",
        "test",
        vec!["test"],
        Level::Full,
        101,
    );
    let result = filter.filter(&input).unwrap();

    // Compiling noise stripped
    assert!(
        !result.text.contains("Compiling myapp"),
        "Compiling should be stripped"
    );
    // Failures and result should remain
    assert!(result.text.contains("FAILED"), "FAILED should remain");
    assert!(
        result.text.contains("test result:"),
        "result summary should remain"
    );
}

// ---------------------------------------------------------------------------
// Pipeline e2e — shell plugin in a full pipeline with builtins
// ---------------------------------------------------------------------------

#[test]
fn pipeline_with_shell_plugin_and_builtins() {
    use lowfat_core::pipeline::Pipeline;
    use lowfat_runner::runner::execute_pipeline;
    use std::collections::HashMap;

    // A simple plugin that uppercases everything
    let upper_script = "#!/bin/sh\ncat | tr '[:lower:]' '[:upper:]'";
    let (filter, _dir) = temp_plugin("upper", upper_script);

    let pipeline = Pipeline::parse("strip-ansi | upper-compact | head");
    let raw = "\x1b[31mhello world\x1b[0m\nsecond line\nthird line";

    let input = make_input(raw, "test", "", vec![], Level::Full, 0);

    let mut plugin_map: HashMap<String, Box<dyn FilterPlugin>> = HashMap::new();
    plugin_map.insert("upper-compact".to_string(), Box::new(filter));

    let result = execute_pipeline(&pipeline, raw, &input, &plugin_map).unwrap();

    // strip-ansi removes ANSI, then upper-compact uppercases
    assert!(
        result.contains("HELLO WORLD"),
        "should be uppercased: {}",
        result
    );
    assert!(!result.contains("\x1b["), "ANSI should be stripped");
}

// ---------------------------------------------------------------------------
// Level propagation — verify all three levels reach the script correctly
// ---------------------------------------------------------------------------

#[test]
fn all_levels_propagated() {
    let script = r#"#!/bin/sh
echo "level=$LOWFAT_LEVEL"
"#;
    let (filter, _dir) = temp_plugin("leveltest", script);

    for (level, expected) in [
        (Level::Lite, "level=lite"),
        (Level::Full, "level=full"),
        (Level::Ultra, "level=ultra"),
    ] {
        let input = make_input("ignored", "test", "", vec![], level, 0);
        let result = filter.filter(&input).unwrap();
        assert!(
            result.text.contains(expected),
            "level {:?}: expected '{}', got '{}'",
            level,
            expected,
            result.text.trim()
        );
    }
}

// ---------------------------------------------------------------------------
// Exit code propagation
// ---------------------------------------------------------------------------

#[test]
fn exit_code_reaches_script() {
    let script = r#"#!/bin/sh
RAW=$(cat)
EXIT="$LOWFAT_EXIT_CODE"
if [ "$EXIT" != "0" ]; then
  echo "ERROR (exit $EXIT):"
  echo "$RAW"
else
  echo "$RAW" | head -n 5
fi
"#;
    let (filter, _dir) = temp_plugin("exitcode", script);

    // Success case
    let input = make_input("all good", "test", "", vec![], Level::Full, 0);
    let result = filter.filter(&input).unwrap();
    assert!(
        !result.text.contains("ERROR"),
        "should not show error on exit 0"
    );

    // Failure case
    let input = make_input("something broke", "test", "", vec![], Level::Full, 1);
    let result = filter.filter(&input).unwrap();
    assert!(
        result.text.contains("ERROR (exit 1)"),
        "should show error on exit 1"
    );
    assert!(
        result.text.contains("something broke"),
        "should preserve raw on error"
    );
}

// ---------------------------------------------------------------------------
// User plugins on large input (issue #9) — loaded the way the runner loads
// them: discovery + HybridRunner, covering both .sh and .lf entry points.
// ---------------------------------------------------------------------------

fn load_user_plugin(name: &str, entry_file: &str, body: &str) -> Box<dyn FilterPlugin> {
    let root = std::env::temp_dir().join(format!("lowfat-user-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join(name).join(format!("{name}-compact"));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = format!(
        "[plugin]\nname = \"{name}-compact\"\nversion = \"0.1.0\"\ncommands = [\"{name}\"]\n"
    );
    std::fs::write(dir.join("lowfat.toml"), manifest).unwrap();
    std::fs::write(dir.join(entry_file), body).unwrap();

    let plugins = lowfat_plugin::discovery::discover_plugins(&root);
    let plugin = plugins
        .iter()
        .find(|p| p.manifest.plugin.name == format!("{name}-compact"))
        .expect("user plugin discovered");
    lowfat_runner::runner::HybridRunner::load(plugin).expect("user plugin loads")
}

fn large_input() -> String {
    "+ a line of output that pads out the stream\n".repeat(50_000) // ~2.2MB
}

#[test]
fn user_sh_plugin_early_exit_on_large_input() {
    let filter = load_user_plugin("bigsh", "filter.sh", "#!/bin/sh\nhead -n 1\n");
    let raw = large_input();
    let result = filter
        .filter(&make_input(&raw, "bigsh", "", vec![], Level::Full, 0))
        .unwrap();
    assert_eq!(result.text.lines().count(), 1);
}

#[test]
fn user_sh_plugin_large_output_no_deadlock() {
    // identity filter: stdout outgrows the pipe buffer while stdin streams
    let filter = load_user_plugin("bigcat", "filter.sh", "#!/bin/sh\ncat\n");
    let raw = large_input();
    let result = filter
        .filter(&make_input(&raw, "bigcat", "", vec![], Level::Full, 0))
        .unwrap();
    assert_eq!(result.text.len(), raw.len());
}

#[test]
fn user_lf_plugin_early_exit_on_large_input() {
    let filter = load_user_plugin("biglf", "filter.lf", "*:\n    shell: head -n 1\n");
    let raw = large_input();
    let result = filter
        .filter(&make_input(&raw, "biglf", "", vec![], Level::Full, 0))
        .unwrap();
    assert_eq!(result.text.lines().count(), 1);
}
