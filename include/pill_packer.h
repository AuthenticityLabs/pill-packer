#ifndef PILL_PACKER_H
#define PILL_PACKER_H
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct Context pp_context;
pp_context *pp_new(void);
void pp_free(pp_context *ctx);
int64_t pp_define(pp_context *ctx, const uint8_t *bytes, size_t length, uint8_t format);
int64_t pp_scan(pp_context *ctx, const uint8_t *bytes, size_t length, size_t maximum);
const uint8_t *pp_document(const pp_context *ctx, size_t *length);
int64_t pp_json(pp_context *ctx, const uint8_t *bytes, size_t length, uint8_t mode);
const uint8_t *pp_output(const pp_context *ctx, size_t *length);
const uint8_t *pp_error(const pp_context *ctx, size_t *length);
#ifdef __cplusplus
}
#endif
#endif
