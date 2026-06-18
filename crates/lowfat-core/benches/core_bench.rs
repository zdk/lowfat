use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lowfat_core::level::Level;
use lowfat_core::pipeline::{
    apply_builtin, proc_dedup_blank, proc_strip_ansi, proc_token_budget, proc_truncate,
    ConditionalPipelines, Pipeline,
};
use lowfat_core::tokens::estimate_tokens;

// --- Token estimation ---

fn bench_estimate_tokens(c: &mut Criterion) {
    let short = "hello world";
    let medium = "a]".repeat(500); // 1000 chars
    let large = "x".repeat(10_000);

    let mut group = c.benchmark_group("estimate_tokens");
    group.bench_function("short_11b", |b| {
        b.iter(|| estimate_tokens(black_box(short)))
    });
    group.bench_function("medium_1kb", |b| {
        b.iter(|| estimate_tokens(black_box(&medium)))
    });
    group.bench_function("large_10kb", |b| {
        b.iter(|| estimate_tokens(black_box(&large)))
    });
    group.finish();
}

// --- Built-in processors ---

fn make_ansi_text(lines: usize) -> String {
    (0..lines)
        .map(|i| format!("\x1b[32m+\x1b[0m line {i}: some code here"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn make_plain_text(lines: usize) -> String {
    (0..lines)
        .map(|i| format!("line {i}: some output text here"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn make_blanky_text(lines: usize) -> String {
    // Alternating content and double blanks
    (0..lines)
        .map(|i| {
            if i % 3 == 0 {
                String::new()
            } else {
                format!("content line {i}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn bench_proc_strip_ansi(c: &mut Criterion) {
    let small = make_ansi_text(20);
    let large = make_ansi_text(500);
    let clean = make_plain_text(500); // no ANSI = fast path

    let mut group = c.benchmark_group("proc_strip_ansi");
    group.bench_function("20_lines", |b| {
        b.iter(|| proc_strip_ansi(black_box(&small)))
    });
    group.bench_function("500_lines", |b| {
        b.iter(|| proc_strip_ansi(black_box(&large)))
    });
    group.bench_function("500_lines_clean", |b| {
        b.iter(|| proc_strip_ansi(black_box(&clean)))
    });
    group.finish();
}

fn bench_proc_truncate(c: &mut Criterion) {
    let text = make_plain_text(500);

    let mut group = c.benchmark_group("proc_truncate");
    group.bench_function("within_limit", |b| {
        b.iter(|| proc_truncate(black_box(&text), 1000))
    });
    group.bench_function("over_limit", |b| {
        b.iter(|| proc_truncate(black_box(&text), 40))
    });
    group.finish();
}

fn bench_proc_token_budget(c: &mut Criterion) {
    let text = make_plain_text(500);

    let mut group = c.benchmark_group("proc_token_budget");
    group.bench_function("within_budget", |b| {
        b.iter(|| proc_token_budget(black_box(&text), 100_000))
    });
    group.bench_function("over_budget", |b| {
        b.iter(|| proc_token_budget(black_box(&text), 500))
    });
    group.finish();
}

fn bench_proc_dedup_blank(c: &mut Criterion) {
    let text = make_blanky_text(500);

    c.bench_function("proc_dedup_blank_500_lines", |b| {
        b.iter(|| proc_dedup_blank(black_box(&text)))
    });
}

fn bench_apply_builtin(c: &mut Criterion) {
    let text = make_ansi_text(200);

    let mut group = c.benchmark_group("apply_builtin");
    for name in &[
        "strip-ansi",
        "truncate",
        "head",
        "token-budget",
        "dedup-blank",
        "passthrough",
    ] {
        group.bench_function(*name, |b| {
            b.iter(|| apply_builtin(black_box(name), black_box(&text), Level::Full, None, None))
        });
    }
    // Unknown name returns None immediately
    group.bench_function("unknown", |b| {
        b.iter(|| {
            apply_builtin(
                black_box("git-compact"),
                black_box(&text),
                Level::Full,
                None,
                None,
            )
        })
    });
    group.finish();
}

// --- Pipeline parsing ---

fn bench_pipeline_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline_parse");
    group.bench_function("single", |b| {
        b.iter(|| Pipeline::parse(black_box("git-compact")))
    });
    group.bench_function("three_stages", |b| {
        b.iter(|| Pipeline::parse(black_box("strip-ansi, git-compact, truncate")))
    });
    group.finish();
}

fn bench_conditional_select(c: &mut Criterion) {
    let cp = ConditionalPipelines {
        default: Some(Pipeline::single("git-compact")),
        on_error: Some(Pipeline::parse("strip-ansi | head")),
        on_empty: Some(Pipeline::parse("passthrough")),
        on_large: Some(Pipeline::parse("git-compact | token-budget")),
    };
    let normal_output = "some output";
    let large_output = "x".repeat(5000);

    let mut group = c.benchmark_group("conditional_select");
    group.bench_function("default", |b| {
        b.iter(|| cp.select(black_box(0), black_box(normal_output)))
    });
    group.bench_function("error", |b| {
        b.iter(|| cp.select(black_box(1), black_box(normal_output)))
    });
    group.bench_function("empty", |b| {
        b.iter(|| cp.select(black_box(0), black_box("")))
    });
    group.bench_function("large", |b| {
        b.iter(|| cp.select(black_box(0), black_box(&*large_output)))
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_estimate_tokens,
    bench_proc_strip_ansi,
    bench_proc_truncate,
    bench_proc_token_budget,
    bench_proc_dedup_blank,
    bench_apply_builtin,
    bench_pipeline_parse,
    bench_conditional_select,
);
criterion_main!(benches);
