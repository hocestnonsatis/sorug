# sorug-ffi

C ABI for [sorug](https://github.com/hocestnonsatis/sorug). The main Rust crate keeps
`forbid(unsafe_code)`; this package is the intentional `unsafe` boundary
(`cdylib` + `staticlib`).

## Build

From the repository root (Cargo workspace):

```bash
cargo build -p sorug-ffi --release
```

Artifacts: `target/release/libsorug_ffi.so` (or `.dylib` / `.dll`) and
`libsorug_ffi.a`. Header: [`include/sorug.h`](include/sorug.h).

## Example (C)

```c
#include "sorug.h"
#include <stdio.h>

int main(void) {
    const char *s = "https://example.com/a?q=1#f";
    SorugUrl *url = sorug_parse(s, 27);
    if (!url) return 1;

    const char *href;
    size_t len;
    sorug_href(url, &href, &len);
    fwrite(href, 1, len, stdout);
    putchar('\n');

    sorug_free(url);
    return 0;
}
```

Link with `-lsorug_ffi` (and the Rust sysroot / `libstd` as required for the
`cdylib`).

## Tests

```bash
cargo test -p sorug-ffi
```
