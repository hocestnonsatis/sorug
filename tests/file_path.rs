//! `from_file_path` / `to_file_path` / `from_directory_path` parity tests.

#![cfg(all(unix, feature = "std"))]

use std::path::{Path, PathBuf};

use sorug::Url;

#[test]
fn from_file_path_absolute() {
    let url = Url::from_file_path("/tmp/foo.txt").unwrap();
    assert_eq!(url.as_str(), "file:///tmp/foo.txt");
    assert_eq!(url.scheme(), "file");
}

#[test]
fn from_file_path_rejects_relative() {
    assert!(Url::from_file_path("../foo.txt").is_err());
    assert!(Url::from_file_path("https://google.com/").is_err());
}

#[test]
fn from_directory_path_trailing_slash() {
    let url = Url::from_directory_path("/var/www").unwrap();
    assert!(url.as_str().ends_with('/'));
    let joined = url.join("index.html").unwrap();
    assert_eq!(joined.as_str(), "file:///var/www/index.html");
}

#[test]
fn to_file_path_roundtrip() {
    let url = Url::from_file_path("/etc/passwd").unwrap();
    let path = url.to_file_path().unwrap();
    assert_eq!(path, PathBuf::from("/etc/passwd"));
}

#[test]
fn to_file_path_localhost_host_ok() {
    let url = Url::parse("file://localhost/tmp/x").unwrap();
    assert_eq!(url.to_file_path().unwrap(), Path::new("/tmp/x"));
}

#[test]
fn to_file_path_rejects_non_local_host() {
    let url = Url::parse("file://example.com/tmp/x").unwrap();
    assert!(url.to_file_path().is_err());
}

#[test]
fn space_in_path_percent_encoded() {
    let url = Url::from_file_path("/tmp/foo bar").unwrap();
    assert!(url.as_str().contains("foo%20bar") || url.as_str().contains("foo bar"));
    let back = url.to_file_path().unwrap();
    assert_eq!(back, PathBuf::from("/tmp/foo bar"));
}
