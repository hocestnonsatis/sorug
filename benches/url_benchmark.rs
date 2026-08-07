//! Criterion showdown: `sorug` vs servo/`url` vs `ada-url`.
//!
//! Regression discipline (see CONTRIBUTING.md):
//! - Parse group must keep Fast_Path_ASCII / Complex_Query_Fragment / File_Edge_Case
//!   competitive with ada; report **relative** deltas on the same machine.
//! - Mutation benches are sorug-only trend lines (no peer gate).
//! - Do not merge hot-path parser changes without Criterion evidence; no `unsafe`.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use sorug::Url;

const FAST_PATH_ASCII: &str = "https://example.com/api/v1/users";
const COMPLEX_QUERY_FRAGMENT: &str =
    "https://user:password@api.example.com:8443/v1/search?q=rust+performance&sort=desc#results";
const IDNA_PUNYCODE: &str = "https://türkçe.com/iletisim";
const FILE_EDGE_CASE: &str = "file:///C:/Windows/System32/drivers/etc/hosts";
const JOIN_BASE: &str = "https://example.com/dir/page";
const JOIN_REL: &str = "../other?x=1#f";

fn bench_url_parsers(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_parse");

    let cases = [
        ("Fast_Path_ASCII", FAST_PATH_ASCII),
        ("Complex_Query_Fragment", COMPLEX_QUERY_FRAGMENT),
        ("IDNA_Punycode", IDNA_PUNYCODE),
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

    group.finish();
}

criterion_group!(benches, bench_url_parsers, bench_mutations);
criterion_main!(benches);
