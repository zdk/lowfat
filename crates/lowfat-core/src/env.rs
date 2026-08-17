//! Env allowlist for plugin subprocesses. Secrets (AWS keys, tokens,
//! API keys) must never reach plugin code — start from env_clear() and
//! pass only these through.

use std::collections::HashSet;

const SAFE_ENV_VARS: &[&str] = &[
    "LOWFAT_LEVEL",
    "LOWFAT_COMMAND",
    "LOWFAT_SUBCOMMAND",
    "LOWFAT_EXIT_CODE",
    "PATH",
    "HOME",
    "USER",
    "SHELL",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "TMPDIR",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "DOCKER_HOST",
    "KUBECONFIG",
    "GOPATH",
    "GOROOT",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "NODE_PATH",
    "NPM_CONFIG_PREFIX",
    "VIRTUAL_ENV",
    "PYTHONPATH",
];

pub fn sanitized_env() -> Vec<(String, String)> {
    let safe: HashSet<&str> = SAFE_ENV_VARS.iter().copied().collect();
    std::env::vars()
        .filter(|(k, _)| safe.contains(k.as_str()))
        .collect()
}
