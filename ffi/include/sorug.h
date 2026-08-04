/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef SORUG_H
#define SORUG_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct SorugUrl SorugUrl;

/* Parse UTF-8 `input[0..len)`. Returns NULL on failure. Free with sorug_free. */
SorugUrl *sorug_parse(const char *input, size_t len);

/* Parse against optional `base` (may be NULL). */
SorugUrl *sorug_parse_with_base(const char *input, size_t len, const SorugUrl *base);

void sorug_free(SorugUrl *url);

/*
 * String getters write a pointer into the handle's serialization (not NUL-terminated)
 * and a byte length. out_ptr / out_len may be NULL.
 * Return 0 on success, -1 if url is NULL.
 */
int sorug_href(const SorugUrl *url, const char **out_ptr, size_t *out_len);
int sorug_scheme(const SorugUrl *url, const char **out_ptr, size_t *out_len);
int sorug_username(const SorugUrl *url, const char **out_ptr, size_t *out_len);
int sorug_password(const SorugUrl *url, const char **out_ptr, size_t *out_len);
int sorug_pathname(const SorugUrl *url, const char **out_ptr, size_t *out_len);
int sorug_search(const SorugUrl *url, const char **out_ptr, size_t *out_len);
int sorug_hash(const SorugUrl *url, const char **out_ptr, size_t *out_len);

/* Optional components: 1 present, 0 absent, -1 if url is NULL. */
int sorug_host(const SorugUrl *url, const char **out_ptr, size_t *out_len);
int sorug_query(const SorugUrl *url, const char **out_ptr, size_t *out_len);
int sorug_fragment(const SorugUrl *url, const char **out_ptr, size_t *out_len);

/* 1 if port present (written to *out_port), 0 absent, -1 if url is NULL. */
int sorug_port(const SorugUrl *url, uint16_t *out_port);

/* 1 if cannot-be-a-base, 0 otherwise, -1 if url is NULL. */
int sorug_cannot_be_a_base(const SorugUrl *url);

#ifdef __cplusplus
}
#endif

#endif /* SORUG_H */
