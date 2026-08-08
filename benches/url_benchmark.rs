//! Criterion showdown: `sorug` vs servo/`url` vs `ada-url`.
//!
//! Regression discipline (see CONTRIBUTING.md):
//! - Parse group must keep Fast_Path_ASCII / Complex_Query_Fragment / File_Edge_Case
//!   competitive with ada; report **relative** deltas on the same machine.
//! - Mutation peer group tracks setter/`join` cost vs peers (relative ordering).
//! - Throughput group exercises batch parse (cache-warm).
//! - Do not merge hot-path parser changes without Criterion evidence; no `unsafe`.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use sorug::Url;

const FAST_PATH_ASCII: &str = "https://example.com/api/v1/users";
const COMPLEX_QUERY_FRAGMENT: &str =
    "https://user:password@api.example.com:8443/v1/search?q=rust+performance&sort=desc#results";
const IDNA_PUNYCODE: &str = "https://türkçe.com/iletisim";
/// Long ACE (>128 octets) — regression guard for growable Punycode buffer.
const IDNA_LONG_ACE: &str = "http://x\u{100b}n--LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLwssf/";
const FILE_EDGE_CASE: &str = "file:///C:/Windows/System32/drivers/etc/hosts";
const JOIN_BASE: &str = "https://example.com/dir/page";
const JOIN_REL: &str = "../other?x=1#f";

const THROUGHPUT_BATCH: &[&str] = &[
    FAST_PATH_ASCII,
    COMPLEX_QUERY_FRAGMENT,
    IDNA_PUNYCODE,
    FILE_EDGE_CASE,
    "http://127.0.0.1:8080/health",
    "https://[::1]/x",
    "ws://example.com/socket",
    "mailto:user@example.com",
];

fn bench_url_parsers(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_parse");

    let cases = [
        ("Fast_Path_ASCII", FAST_PATH_ASCII),
        ("Complex_Query_Fragment", COMPLEX_QUERY_FRAGMENT),
        ("IDNA_Punycode", IDNA_PUNYCODE),
        ("IDNA_Long_ACE", IDNA_LONG_ACE),
        ("File_Edge_Case", FILE_EDGE_CASE),
    ];

    for (name, input) in cases {
        group.bench_with_input(BenchmarkId::new("sorug", name), input, |b, input| {
            b.iter(|| {
                let url = sorug::Url::parse(black_box(input)).expect("sorug parse");
                black_box(url)
            });
        });

        group.bench_with_input(BenchmarkId::new("servo_url", name), input, |b, input| {
            b.iter(|| {
                let url = url::Url::parse(black_box(input)).expect("url parse");
                black_box(url)
            });
        });

        group.bench_with_input(BenchmarkId::new("ada_url", name), input, |b, input| {
            b.iter(|| {
                let url = ada_url::Url::parse(black_box(input), None).expect("ada parse");
                black_box(url)
            });
        });
    }

    group.finish();
}

fn bench_parse_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_parse_throughput");
    group.throughput(Throughput::Elements(THROUGHPUT_BATCH.len() as u64));

    group.bench_function("sorug_batch", |b| {
        b.iter(|| {
            let mut n = 0usize;
            for input in THROUGHPUT_BATCH {
                if let Ok(url) = sorug::Url::parse(black_box(*input)) {
                    n = n.wrapping_add(url.href().len());
                }
            }
            black_box(n)
        });
    });

    group.bench_function("servo_url_batch", |b| {
        b.iter(|| {
            let mut n = 0usize;
            for input in THROUGHPUT_BATCH {
                if let Ok(url) = url::Url::parse(black_box(*input)) {
                    n = n.wrapping_add(url.as_str().len());
                }
            }
            black_box(n)
        });
    });

    group.bench_function("ada_url_batch", |b| {
        b.iter(|| {
            let mut n = 0usize;
            for input in THROUGHPUT_BATCH {
                if let Ok(url) = ada_url::Url::parse(black_box(*input), None) {
                    n = n.wrapping_add(url.href().len());
                }
            }
            black_box(n)
        });
    });

    group.finish();
}

fn bench_mutations(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_mutate");

    group.bench_function("sorug_set_pathname", |b| {
        b.iter(|| {
            let mut url = Url::parse(black_box(FAST_PATH_ASCII))
                .expect("parse")
                .into_owned();
            url.set_pathname(black_box("/api/v2/items"));
            black_box(url.href().len())
        });
    });

    group.bench_function("sorug_set_search", |b| {
        b.iter(|| {
            let mut url = Url::parse(black_box(FAST_PATH_ASCII))
                .expect("parse")
                .into_owned();
            url.set_search(black_box("?q=bench&lang=tr"));
            black_box(url.href().len())
        });
    });

    group.bench_function("sorug_join", |b| {
        let base = Url::parse(JOIN_BASE).expect("base").into_owned();
        b.iter(|| {
            let joined = base.join(black_box(JOIN_REL)).expect("join");
            black_box(joined.href().len())
        });
    });

    group.bench_function("sorug_search_params_mutate", |b| {
        b.iter(|| {
            let mut url = Url::parse(black_box("https://example.com/?a=1&b=2"))
                .expect("parse")
                .into_owned();
            {
                let mut q = url.query_pairs_mut();
                q.append("c", "3").set("a", "9");
            }
            black_box(url.href().len())
        });
    });

    group.bench_function("sorug_href_roundtrip", |b| {
        let url = Url::parse(COMPLEX_QUERY_FRAGMENT).expect("parse");
        let href = url.href().to_owned();
        b.iter(|| {
            let again = Url::parse(black_box(href.as_str())).expect("reparse");
            black_box(again.href().len())
        });
    });

    group.bench_function("sorug_set_pathname_idempotent", |b| {
        let mut url = Url::parse(FAST_PATH_ASCII).expect("parse").into_owned();
        // Warm owned buffer once; measure no-op setter path.
        url.set_pathname("/api/v1/users");
        b.iter(|| {
            url.set_pathname(black_box("/api/v1/users"));
            black_box(url.href().len())
        });
    });

    group.bench_function("sorug_set_scheme_idempotent", |b| {
        let mut url = Url::parse(FAST_PATH_ASCII).expect("parse").into_owned();
        b.iter(|| {
            let _ = url.set_scheme(black_box("https"));
            black_box(url.href().len())
        });
    });

    group.bench_function("sorug_set_hostname_idempotent", |b| {
        let mut url = Url::parse(FAST_PATH_ASCII).expect("parse").into_owned();
        b.iter(|| {
            let _ = url.set_hostname(black_box("example.com"));
            black_box(url.href().len())
        });
    });

    group.bench_function("sorug_path_segments_push", |b| {
        b.iter(|| {
            let mut url = Url::parse(black_box("https://example.com/a"))
                .expect("parse")
                .into_owned();
            url.path_segments_mut()
                .expect("can be base")
                .push(black_box("b"))
                .push(black_box("c"));
            black_box(url.href().len())
        });
    });

    group.finish();
}

fn bench_mutate_peers(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_mutate_peers");

    group.bench_function("sorug_set_pathname", |b| {
        b.iter(|| {
            let mut url = sorug::Url::parse(black_box(FAST_PATH_ASCII))
                .expect("parse")
                .into_owned();
            url.set_pathname(black_box("/api/v2/items"));
            black_box(url.href().len())
        });
    });

    group.bench_function("servo_set_path", |b| {
        b.iter(|| {
            let mut url = url::Url::parse(black_box(FAST_PATH_ASCII)).expect("parse");
            url.set_path(black_box("/api/v2/items"));
            black_box(url.as_str().len())
        });
    });

    group.bench_function("ada_set_pathname", |b| {
        b.iter(|| {
            let mut url = ada_url::Url::parse(black_box(FAST_PATH_ASCII), None).expect("parse");
            let _ = url.set_pathname(black_box(Some("/api/v2/items")));
            black_box(url.href().len())
        });
    });

    group.bench_function("sorug_join", |b| {
        let base = sorug::Url::parse(JOIN_BASE).expect("base").into_owned();
        b.iter(|| {
            let joined = base.join(black_box(JOIN_REL)).expect("join");
            black_box(joined.href().len())
        });
    });

    group.bench_function("servo_join", |b| {
        let base = url::Url::parse(JOIN_BASE).expect("base");
        b.iter(|| {
            let joined = base.join(black_box(JOIN_REL)).expect("join");
            black_box(joined.as_str().len())
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_url_parsers,
    bench_parse_throughput,
    bench_mutations,
    bench_mutate_peers
);
criterion_main!(benches);
