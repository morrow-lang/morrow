/** Bounded SQLite connections with explicit, stale-handle-safe lifetimes. */
#include "fern_runtime.h"
#include <assert.h>
#include <limits.h>
#include <sqlite3.h>
#include <stddef.h>

/* Bound live native connections; closed slots are compacted and reused. */
#define FERN_SQL_HANDLES_MAX 256

typedef struct {
    int64_t handle_id;
    sqlite3* db;
} FernSqlHandle;

typedef struct {
    FernSqlHandle handles[FERN_SQL_HANDLES_MAX];
    size_t handle_len;
    int64_t next_handle_id;
} FernSqlRuntimeState;

/* IDs are never recycled, even when a slot is reused. */
static FernSqlRuntimeState g_sql_runtime = {.next_handle_id = 1};

/**
 * Find an entry among bounded live connections.
 * @param state SQL runtime state.
 * @param handle_id Positive opaque Fern handle ID.
 * @return Matching entry, or NULL for a stale or unknown handle.
 */
static FernSqlHandle* fern_sql_find_handle(FernSqlRuntimeState* state, int64_t handle_id) {
    assert(state != NULL);
    assert(state->handle_len <= FERN_SQL_HANDLES_MAX);
    for (size_t i = 0; i < state->handle_len; i++) {
        if (state->handles[i].handle_id == handle_id) return &state->handles[i];
    }
    return NULL;
}

/**
 * Open SQLite only after verifying handle quota and ID availability.
 * @param path Nonempty database path, or SQLite's :memory: name.
 * @return Ok(handle), Err(IO), or Err(OUT_OF_MEMORY) when the quota is exhausted.
 */
int64_t fern_sql_open(const char* path) {
    FernSqlRuntimeState* state = &g_sql_runtime;
    assert(state->handle_len <= FERN_SQL_HANDLES_MAX);
    assert(state->next_handle_id > 0);
    if (path == NULL || path[0] == '\0') return fern_result_err(FERN_ERR_IO);
    if (state->handle_len == FERN_SQL_HANDLES_MAX || state->next_handle_id == INT64_MAX) {
        return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    }
    sqlite3* db = NULL;
    int rc = sqlite3_open(path, &db);
    if (rc != SQLITE_OK || db == NULL) {
        if (db != NULL) sqlite3_close(db);
        return fern_result_err(FERN_ERR_IO);
    }
    int64_t handle_id = state->next_handle_id++;
    FernSqlHandle* slot = &state->handles[state->handle_len++];
    slot->handle_id = handle_id;
    slot->db = db;
    return fern_result_ok(handle_id);
}

/**
 * Execute complete SQLite statements against a live connection.
 * @param handle Positive Fern handle ID.
 * @param query Nonempty SQL text.
 * @return Ok(rows affected), or Err(IO) for invalid handles or SQLite failures.
 */
int64_t fern_sql_execute(int64_t handle, const char* query) {
    FernSqlRuntimeState* state = &g_sql_runtime;
    assert(state->handle_len <= FERN_SQL_HANDLES_MAX);
    assert(state->next_handle_id > 0);
    if (query == NULL || query[0] == '\0' || handle <= 0) {
        return fern_result_err(FERN_ERR_IO);
    }
    FernSqlHandle* entry = fern_sql_find_handle(state, handle);
    if (entry == NULL) return fern_result_err(FERN_ERR_IO);
    assert(entry->db != NULL);
    char* error = NULL;
    int rc = sqlite3_exec(entry->db, query, NULL, NULL, &error);
    if (error != NULL) sqlite3_free(error);
    if (rc != SQLITE_OK) return fern_result_err(FERN_ERR_IO);
    return fern_result_ok((int64_t)sqlite3_changes(entry->db));
}

/**
 * Close a live connection and release its quota without recycling its ID.
 * @param handle Positive Fern handle ID.
 * @return Ok(0), or Err(IO) for invalid/already-closed handles or SQLite failure.
 * A failed SQLite close retains the connection so the caller can retry.
 */
int64_t fern_sql_close(int64_t handle) {
    FernSqlRuntimeState* state = &g_sql_runtime;
    assert(state->handle_len <= FERN_SQL_HANDLES_MAX);
    assert(state->next_handle_id > 0);
    if (handle <= 0) return fern_result_err(FERN_ERR_IO);
    FernSqlHandle* entry = fern_sql_find_handle(state, handle);
    if (entry == NULL) return fern_result_err(FERN_ERR_IO);
    assert(entry->db != NULL);
    if (sqlite3_close(entry->db) != SQLITE_OK) return fern_result_err(FERN_ERR_IO);
    assert(state->handle_len > 0);
    size_t last = --state->handle_len;
    *entry = state->handles[last];
    state->handles[last] = (FernSqlHandle){0};
    return fern_result_ok(0);
}
