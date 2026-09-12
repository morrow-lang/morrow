/** SQL handle lifecycle and live-resource quota oracles. */
#include "fern_runtime.h"
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>

/** Fail visibly in debug and release. @param ok Condition. @param line Location. */
static void require_at(bool ok, int line) {
    if (!ok) { fprintf(stderr, "SQL failure:%d\n", line); exit(90); }
}
#define REQUIRE(x) require_at((x), __LINE__)

/** Verify the tag and entire payload. @param result Result. @param ok Tag. @param value Payload. */
static void scalar(int64_t result, bool ok, int64_t value) {
    REQUIRE(fern_result_is_ok(result) == ok);
    REQUIRE(fern_result_unwrap(result) == value);
}

/** Open a private database. @param path Database path. @return Live handle. */
static int64_t opened(const char* path) {
    int64_t result = fern_sql_open(path);
    REQUIRE(fern_result_is_ok(result));
    int64_t handle = fern_result_unwrap(result);
    REQUIRE(handle > 0);
    return handle;
}

/** Check invalid IDs and independence of stale and live handles. */
static void lifecycle(void) {
    int64_t invalid[] = {0, -1, INT64_MIN, INT64_MAX, INT64_C(4294967297)};
    for (size_t i = 0; i < sizeof(invalid) / sizeof(invalid[0]); i++) {
        scalar(fern_sql_close(invalid[i]), false, FERN_ERR_IO);
        scalar(fern_sql_execute(invalid[i], "select 1"), false, FERN_ERR_IO);
    }
    int64_t first = opened(":memory:");
    int64_t second = opened(":memory:");
    scalar(fern_sql_execute(first, "create table data (n integer)"), true, 0);
    scalar(fern_sql_close(first), true, 0);
    scalar(fern_sql_close(first), false, FERN_ERR_IO);
    scalar(fern_sql_execute(first, "select 1"), false, FERN_ERR_IO);
    scalar(fern_sql_execute(second, "create table data (n integer)"), true, 0);
    int64_t third = opened(":memory:");
    REQUIRE(third != first && third != second);
    scalar(fern_sql_close(first), false, FERN_ERR_IO);
    scalar(fern_sql_execute(third, "create table data (n integer)"), true, 0);
    scalar(fern_sql_close(second), true, 0);
    scalar(fern_sql_close(third), true, 0);
    for (int i = 0; i < 768; i++) scalar(fern_sql_close(opened(":memory:")), true, 0);
}

/** Check exact live-cap boundary and quota release before reopening. @param path Uncreated path. */
static void capacity(const char* path) {
    int64_t handles[256];
    for (size_t i = 0; i < 256; i++) handles[i] = opened(":memory:");
    scalar(fern_sql_open(path), false, FERN_ERR_OUT_OF_MEMORY);
    REQUIRE(access(path, F_OK) != 0);
    scalar(fern_sql_close(handles[127]), true, 0);
    int64_t replacement = opened(":memory:");
    REQUIRE(replacement > handles[255]);
    scalar(fern_sql_close(handles[127]), false, FERN_ERR_IO);
    scalar(fern_sql_open(":memory:"), false, FERN_ERR_OUT_OF_MEMORY);
    for (size_t i = 0; i < 256; i++) {
        if (i != 127) scalar(fern_sql_close(handles[i]), true, 0);
    }
    scalar(fern_sql_close(replacement), true, 0);
    scalar(fern_sql_close(opened(":memory:")), true, 0);
}

/** Closing rolls back unfinished transactions and releases SQLite locks. @param path Private db. */
static void transaction(const char* path) {
    int64_t db = opened(path);
    scalar(fern_sql_execute(db, "create table data (n integer unique)"), true, 0);
    scalar(fern_sql_execute(db, "begin; insert into data values (1)"), true, 1);
    scalar(fern_sql_close(db), true, 0);
    db = opened(path);
    scalar(fern_sql_execute(db, "insert into data values (1)"), true, 1);
    scalar(fern_sql_execute(db, "insert into data values (1)"), false, FERN_ERR_IO);
    scalar(fern_sql_close(db), true, 0);
}

/** Run one isolated scenario through the real runtime entry. @return Status. */
int fern_main(void) {
    REQUIRE(fern_args_count() == 3);
    const char* mode = fern_arg(1);
    const char* path = fern_arg(2);
    if (strcmp(mode, "lifecycle") == 0) lifecycle();
    else if (strcmp(mode, "capacity") == 0) capacity(path);
    else if (strcmp(mode, "transaction") == 0) transaction(path);
    else return 2;
    printf("ok:%s\n", mode);
    return 0;
}
