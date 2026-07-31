//! Ensures `tests/urltestdata.json` exists so the WPT harness is reproducible.
//!
//! If the fixture is missing (fresh clone without the checked-in file), this
//! downloads it from the web-platform-tests repository via `curl`.
//!
//! Manual refresh:
//! ```bash
//! curl -fsSL -o tests/urltestdata.json \
//!   https://raw.githubusercontent.com/web-platform-tests/wpt/master/url/resources/urltestdata.json
//! ```

use std::path::Path;
use std::process::Command;

const WPT_URL: &str =
    "https://raw.githubusercontent.com/web-platform-tests/wpt/master/url/resources/urltestdata.json";
const WPT_PATH: &str = "tests/urltestdata.json";

fn main() {
    println!("cargo:rerun-if-changed={WPT_PATH}");

    let path = Path::new(WPT_PATH);
    if path.is_file() {
        return;
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| {
            panic!("failed to create {}: {e}", parent.display());
        });
    }

    eprintln!("cargo:warning=downloading WPT urltestdata.json → {WPT_PATH}");

    let status = Command::new("curl")
        .args(["-fsSL", "-o", WPT_PATH, WPT_URL])
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "failed to run curl ({e}). Install curl or place {WPT_PATH} manually from:\n  {WPT_URL}"
            );
        });

    assert!(
        status.success(),
        "curl failed downloading urltestdata.json (status {status}). Source:\n  {WPT_URL}"
    );
    assert!(
        path.is_file(),
        "download reported success but {WPT_PATH} is missing"
    );
}
