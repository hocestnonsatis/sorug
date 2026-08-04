//! Positive and negative coverage for [`sorug::PathSegmentsMut`].
//!
//! Oracle: servo/`url` 2.5 where the APIs are intended to match. Opaque-path
//! rejection and encoding of `/`, `%`, and special-scheme `\` are locked in.

use sorug::Url;
use url::Url as ServoUrl;

fn servo(input: &str) -> ServoUrl {
    ServoUrl::parse(input).unwrap_or_else(|e| panic!("servo parse {input:?}: {e}"))
}

fn sorug(input: &str) -> Url<'static> {
    Url::parse(input)
        .unwrap_or_else(|e| panic!("sorug parse {input:?}: {e:?}"))
        .into_owned()
}

fn assert_match(label: &str, input: &str, mutate: impl Fn(&mut Url<'_>, &mut ServoUrl)) {
    let mut s = sorug(input);
    let mut v = servo(input);
    mutate(&mut s, &mut v);
    assert_eq!(
        s.as_str(),
        v.as_str(),
        "{label}: input={input:?}\n  sorug={}\n  servo={}",
        s.as_str(),
        v.as_str()
    );
    assert_eq!(s.query(), v.query(), "{label} query");
    assert_eq!(s.fragment(), v.fragment(), "{label} fragment");
    assert_eq!(s.path(), v.path(), "{label} path");
}

// ---------------------------------------------------------------------------
// Negative: cannot-be-a-base
// ---------------------------------------------------------------------------

#[test]
fn negative_opaque_paths_reject_mut() {
    for input in [
        "mailto:me@example.com",
        "data:text/plain,hi",
        "javascript:alert(1)",
        "blob:https://example.com/uuid",
        "about:blank",
    ] {
        let mut url = Url::parse(input).unwrap_or_else(|e| panic!("{input}: {e:?}"));
        assert!(url.cannot_be_a_base(), "{input} should be cannot-be-a-base");
        assert!(
            url.path_segments_mut().is_err(),
            "{input} must reject path_segments_mut"
        );
        assert!(
            url.path_segments().is_none(),
            "{input} must yield no path_segments iterator"
        );
    }
}

#[test]
fn negative_dot_segments_are_ignored_not_traversed() {
    assert_match("ignore dots", "https://example.com/a", |s, v| {
        s.path_segments_mut().unwrap().extend([".", "..", "..."]);
        v.path_segments_mut().unwrap().extend([".", "..", "..."]);
    });
    // `...` is a real segment (not ignored); `.` / `..` alone are skipped.
    let mut url = sorug("https://example.com/a");
    url.path_segments_mut().unwrap().extend([".", "..", "..."]);
    assert_eq!(url.as_str(), "https://example.com/a/...");
}

#[test]
fn negative_root_pop_and_pop_if_empty_are_noops() {
    assert_match("root pop", "https://example.com/", |s, v| {
        s.path_segments_mut().unwrap().pop();
        v.path_segments_mut().unwrap().pop();
    });
    assert_match("root pop_if_empty", "https://example.com/", |s, v| {
        s.path_segments_mut().unwrap().pop_if_empty();
        v.path_segments_mut().unwrap().pop_if_empty();
    });
    assert_match("root clear", "https://example.com/", |s, v| {
        s.path_segments_mut().unwrap().clear();
        v.path_segments_mut().unwrap().clear();
    });
}

#[test]
fn negative_empty_nonspecial_pop_is_noop() {
    assert_match("empty pop", "foo://host", |s, v| {
        s.path_segments_mut().unwrap().pop();
        v.path_segments_mut().unwrap().pop();
    });
    assert_match("empty pop_if_empty", "foo://host", |s, v| {
        s.path_segments_mut().unwrap().pop_if_empty();
        v.path_segments_mut().unwrap().pop_if_empty();
    });
}

// ---------------------------------------------------------------------------
// Positive: rust-url docs + encoding
// ---------------------------------------------------------------------------

#[test]
fn positive_docs_examples() {
    assert_match(
        "docs pop push encode",
        "http://example.net/foo/index.html",
        |s, v| {
            s.path_segments_mut()
                .unwrap()
                .pop()
                .push("img")
                .push("2/100%.png");
            v.path_segments_mut()
                .unwrap()
                .pop()
                .push("img")
                .push("2/100%.png");
        },
    );

    assert_match(
        "docs clear push",
        "https://github.com/servo/rust-url/",
        |s, v| {
            s.path_segments_mut().unwrap().clear().push("logout");
            v.path_segments_mut().unwrap().clear().push("logout");
        },
    );

    assert_match(
        "docs pop_if_empty push",
        "https://github.com/servo/rust-url/",
        |s, v| {
            s.path_segments_mut().unwrap().pop_if_empty().push("pulls");
            v.path_segments_mut().unwrap().pop_if_empty().push("pulls");
        },
    );

    assert_match(
        "docs extend skip dots",
        "https://github.com/servo",
        |s, v| {
            s.path_segments_mut()
                .unwrap()
                .extend(["..", "rust-url", ".", "pulls"]);
            v.path_segments_mut()
                .unwrap()
                .extend(["..", "rust-url", ".", "pulls"]);
        },
    );
}

#[test]
fn positive_trailing_slash_semantics() {
    // Without pop_if_empty, trailing slash yields a double slash before push.
    assert_match("trail push double", "https://example.com/a/b/", |s, v| {
        s.path_segments_mut().unwrap().push("c");
        v.path_segments_mut().unwrap().push("c");
    });
    assert_match(
        "trail pop_if_empty push",
        "https://example.com/a/b/",
        |s, v| {
            s.path_segments_mut().unwrap().pop_if_empty().push("c");
            v.path_segments_mut().unwrap().pop_if_empty().push("c");
        },
    );
    assert_match("trail pop", "https://example.com/a/b/", |s, v| {
        s.path_segments_mut().unwrap().pop();
        v.path_segments_mut().unwrap().pop();
    });
}

#[test]
fn positive_empty_path_nonspecial() {
    assert_match("empty push", "foo://host", |s, v| {
        s.path_segments_mut().unwrap().push("a");
        v.path_segments_mut().unwrap().push("a");
    });
    assert_match("empty clear push", "foo://host", |s, v| {
        s.path_segments_mut().unwrap().clear().push("x");
        v.path_segments_mut().unwrap().clear().push("x");
    });
}

#[test]
fn positive_encoding_slash_percent_backslash() {
    assert_match("encode slash+percent", "https://example.com/", |s, v| {
        s.path_segments_mut().unwrap().push("2/100%.png");
        v.path_segments_mut().unwrap().push("2/100%.png");
    });
    assert_match(
        "encode backslash special",
        "https://example.com/",
        |s, v| {
            s.path_segments_mut().unwrap().push("a\\b");
            v.path_segments_mut().unwrap().push("a\\b");
        },
    );
    // Non-special: `\` stays literal (not in path-segment encode set).
    assert_match("literal backslash nonspecial", "foo://host/", |s, v| {
        s.path_segments_mut().unwrap().push("a\\b");
        v.path_segments_mut().unwrap().push("a\\b");
    });
}

#[test]
fn positive_unicode_and_empty_segment() {
    assert_match("unicode café", "https://example.com/", |s, v| {
        s.path_segments_mut().unwrap().push("café");
        v.path_segments_mut().unwrap().push("café");
    });
    assert_match("push empty segment", "https://example.com/a", |s, v| {
        s.path_segments_mut().unwrap().push("");
        v.path_segments_mut().unwrap().push("");
    });
}

#[test]
fn positive_query_fragment_preserved() {
    assert_match(
        "push keeps q+f",
        "https://example.com/a?q=1#frag",
        |s, v| {
            s.path_segments_mut().unwrap().push("b");
            v.path_segments_mut().unwrap().push("b");
        },
    );
    assert_match(
        "clear keeps query",
        "https://example.com/a/b?q=1",
        |s, v| {
            s.path_segments_mut().unwrap().clear();
            v.path_segments_mut().unwrap().clear();
        },
    );
    assert_match(
        "pop keeps fragment only",
        "https://example.com/a/b#f",
        |s, v| {
            s.path_segments_mut().unwrap().pop();
            v.path_segments_mut().unwrap().pop();
        },
    );
    assert_match(
        "clear then push keeps both",
        "https://example.com/a?x=1#y",
        |s, v| {
            s.path_segments_mut().unwrap().clear().push("z");
            v.path_segments_mut().unwrap().clear().push("z");
        },
    );
}

#[test]
fn positive_file_scheme() {
    assert_match("file pop push", "file:///tmp/x", |s, v| {
        s.path_segments_mut().unwrap().pop().push("y");
        v.path_segments_mut().unwrap().pop().push("y");
    });
    assert_match("file clear push", "file:///tmp/x", |s, v| {
        s.path_segments_mut().unwrap().clear().push("z");
        v.path_segments_mut().unwrap().clear().push("z");
    });
}

#[test]
fn positive_extend_chain_and_joinlike() {
    assert_match("extend multi", "https://example.com/", |s, v| {
        s.path_segments_mut()
            .unwrap()
            .extend(["org", "repo", "issues", "188"]);
        v.path_segments_mut()
            .unwrap()
            .extend(["org", "repo", "issues", "188"]);
    });
    assert_match(
        "joinlike pop extend",
        "https://example.com/dir/page",
        |s, v| {
            s.path_segments_mut().unwrap().pop().extend(["other"]);
            v.path_segments_mut().unwrap().pop().extend(["other"]);
        },
    );
}

#[test]
fn positive_chained_ops_idempotent_drop() {
    // Drop must restore query/fragment exactly once even after many mutations.
    let mut url = sorug("https://example.com/a/b/c?keep=1#hash");
    {
        let mut segs = url.path_segments_mut().unwrap();
        segs.pop().pop().push("x").push("y");
    }
    assert_eq!(url.as_str(), "https://example.com/a/x/y?keep=1#hash");
    assert_eq!(url.query(), Some("keep=1"));
    assert_eq!(url.fragment(), Some("hash"));
    assert_eq!(
        url.path_segments().unwrap().collect::<Vec<_>>(),
        ["a", "x", "y"]
    );
}

#[test]
fn positive_path_segments_iterator_after_mut() {
    let mut url = sorug("https://example.com/");
    url.path_segments_mut().unwrap().extend(["one", "two", ""]);
    assert_eq!(
        url.path_segments().unwrap().collect::<Vec<_>>(),
        ["one", "two", ""]
    );
    assert_eq!(url.path(), "/one/two/");
}

#[test]
fn positive_space_and_question_hash_in_segment() {
    assert_match("space encoded", "https://example.com/", |s, v| {
        s.path_segments_mut().unwrap().push("a b");
        v.path_segments_mut().unwrap().push("a b");
    });
    assert_match("question encoded", "https://example.com/", |s, v| {
        s.path_segments_mut().unwrap().push("a?b");
        v.path_segments_mut().unwrap().push("a?b");
    });
    assert_match("hash encoded", "https://example.com/", |s, v| {
        s.path_segments_mut().unwrap().push("a#b");
        v.path_segments_mut().unwrap().push("a#b");
    });
}

#[test]
fn positive_ws_scheme_special_backslash() {
    assert_match("ws backslash", "ws://example.com/", |s, v| {
        s.path_segments_mut().unwrap().push("a\\b");
        v.path_segments_mut().unwrap().push("a\\b");
    });
}

#[test]
fn negative_mutate_does_not_change_scheme_host_port() {
    let mut url = sorug("https://user:pass@example.com:8443/old?q=1#f");
    url.path_segments_mut().unwrap().clear().push("new");
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.username(), "user");
    assert_eq!(url.password(), "pass");
    assert_eq!(url.host(), Some("example.com"));
    assert_eq!(url.port_u16(), Some(8443));
    assert_eq!(url.query(), Some("q=1"));
    assert_eq!(url.fragment(), Some("f"));
    assert_eq!(url.as_str(), "https://user:pass@example.com:8443/new?q=1#f");
}

#[test]
fn positive_ipv6_host_unaffected() {
    assert_match("ipv6 push", "http://[::1]/foo", |s, v| {
        s.path_segments_mut().unwrap().pop().push("bar");
        v.path_segments_mut().unwrap().pop().push("bar");
    });
    let mut url = sorug("http://[2001:db8::1]:8080/x");
    url.path_segments_mut().unwrap().clear().push("y");
    assert_eq!(url.host(), Some("[2001:db8::1]"));
    assert_eq!(url.port_u16(), Some(8080));
    assert_eq!(url.as_str(), "http://[2001:db8::1]:8080/y");
}

#[test]
fn positive_repeated_mut_sessions() {
    let mut url = sorug("https://example.com/a?q=1#h");
    url.path_segments_mut().unwrap().push("b");
    assert_eq!(url.as_str(), "https://example.com/a/b?q=1#h");
    url.path_segments_mut().unwrap().pop_if_empty().push("c");
    assert_eq!(url.as_str(), "https://example.com/a/b/c?q=1#h");
    url.path_segments_mut().unwrap().clear().extend(["z"]);
    assert_eq!(url.as_str(), "https://example.com/z?q=1#h");
}

#[test]
fn positive_only_dots_extend_leaves_path() {
    assert_match("only dots", "https://example.com/keep", |s, v| {
        s.path_segments_mut().unwrap().extend([".", "..", "."]);
        v.path_segments_mut().unwrap().extend([".", "..", "."]);
    });
}

#[test]
fn negative_blob_opaque_inner_still_rejects() {
    // Outer blob URL is cannot-be-a-base even when the path looks like a URL.
    let mut url = Url::parse("blob:https://example.com/uuid").unwrap();
    assert!(url.cannot_be_a_base());
    assert!(url.path_segments_mut().is_err());
}

#[test]
fn positive_ftp_special_encoding() {
    assert_match("ftp slash percent", "ftp://example.com/dir", |s, v| {
        s.path_segments_mut().unwrap().push("a/b%c");
        v.path_segments_mut().unwrap().push("a/b%c");
    });
}
