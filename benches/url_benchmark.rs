//! Criterion showdown: `sorug` vs servo/`url` vs `ada-url`.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

const FAST_PATH_ASCII: &str = "https://example.com/api/v1/users";
const COMPLEX_QUERY_FRAGMENT: &str =
    "https://user:password@api.example.com:8443/v1/search?q=rust+performance&sort=desc#results";
const IDNA_PUNYCODE: &str = "https://türkçe.com/iletisim";
const FILE_EDGE_CASE: &str = "file:///C:/Windows/System32/drivers/etc/hosts";

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

criterion_group!(benches, bench_url_parsers);
criterion_main!(benches);
