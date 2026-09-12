/** Verify capture positions remain stable when optional groups do not participate. */
#include "fern_runtime.h"
#include <assert.h>
#include <stdio.h>

/** Print each capture, including absent-group sentinels, for an external oracle. */
int fern_main(void) {
    assert(fern_args_count() == 3);
    assert(fern_arg(1) != NULL);
    FernRegexCaptures* result = fern_regex_captures(fern_arg(1), fern_arg(2));
    assert(result != NULL);
    printf("%lld\n", (long long)result->count);
    for (int64_t i = 0; i < result->count; i++) {
        FernRegexMatch capture = result->captures[i];
        assert(capture.matched != NULL);
        printf("%lld:%lld:%s\n", (long long)capture.start,
            (long long)capture.end, capture.matched);
    }
    return 0;
}
