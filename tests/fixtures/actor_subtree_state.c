#define _POSIX_C_SOURCE 200809L
#define _DEFAULT_SOURCE
#define _BSD_SOURCE
/* Exercise actual actor state and deterministic notification allocation failure. */
#include "../../runtime/fern_gc.h"
#include <stdbool.h>
static bool fail_next;
static int fail_copy_after;
static void* allocate_once(size_t bytes) {
    if (fail_next) { fail_next = false; return NULL; }
    return GC_MALLOC(bytes);
}
static char* copy_once(const char* text) {
    if (fail_copy_after > 0 && --fail_copy_after == 0) return NULL;
    return fern_gc_strdup(text);
}
#undef FERN_STRDUP
#define FERN_STRDUP(text) copy_once(text)
#undef FERN_ALLOC
#define FERN_ALLOC(bytes) allocate_once(bytes)
/* glibc normalizes feature macros after headers; the included implementation
 * supplies its original definitions again. */
#undef _POSIX_C_SOURCE
#undef _DEFAULT_SOURCE
#include "../../runtime/fern_runtime.c"
#define REQUIRE(c) do { if (!(c)) { fprintf(stderr, "%s:%d: %s\n", __func__, __LINE__, #c); return 1; } } while (0)

static int signal_is(int64_t observer, const char* kind, int64_t pid, const char* reason) {
    int64_t received = fern_actor_receive(observer);
    REQUIRE(fern_result_is_ok(received));
    char expected[128];
    snprintf(expected, sizeof(expected), "%s(%lld,%s)", kind, (long long)pid, reason);
    REQUIRE(strcmp((char*)(intptr_t)fern_result_unwrap(received), expected) == 0);
    return 0;
}
static int dead(int64_t pid) {
    REQUIRE(fern_actor_mailbox_len(pid) == -1);
    REQUIRE(!fern_result_is_ok(fern_actor_send(pid, "stale")));
    REQUIRE(!fern_result_is_ok(fern_actor_receive(pid)));
    REQUIRE(fern_actor_set_current(pid) == -1);
    return 0;
}
static int tree(const char* reason, int inject) {
    int64_t observer = fern_actor_spawn("observer");
    REQUIRE(fern_actor_set_current(observer) == 0);
    int64_t root = fern_actor_spawn_link("root");
    int64_t sibling = fern_actor_spawn("sibling"), leaf = fern_actor_spawn("leaf");
    int64_t middle = fern_actor_spawn("middle"), other = fern_actor_spawn("other");
    int64_t ids[] = {root, middle, leaf, sibling};
    REQUIRE(fern_result_is_ok(fern_actor_supervise(root, middle, 10, 60)));
    REQUIRE(fern_result_is_ok(fern_actor_supervise(middle, leaf, 10, 60)));
    REQUIRE(fern_result_is_ok(fern_actor_supervise(root, sibling, 10, 60)));
    for (size_t i = 0; i < 4; i++) {
        REQUIRE(fern_result_is_ok(fern_actor_monitor(observer, ids[i])));
        REQUIRE(fern_result_is_ok(fern_actor_send(ids[i], "pending")));
    }
    REQUIRE(fern_actor_set_current(leaf) == 0);
    fail_next = inject == 1;
    fail_copy_after = inject == 2 ? 2 : 0;
    int64_t status = fern_actor_exit(root, reason);
    REQUIRE(fern_result_is_ok(status) == !inject);
    REQUIRE(fern_actor_self() == 0);
    for (size_t i = 0; i < 4; i++) REQUIRE(dead(ids[i]) == 0);
    REQUIRE(fern_actor_mailbox_len(other) == 0);
    if (!inject) {
        REQUIRE(signal_is(observer, "Exit", root, reason) == 0);
        for (size_t i = 0; i < 4; i++)
            REQUIRE(signal_is(observer, "DOWN", ids[i], i == 0 ? reason : "shutdown") == 0);
    } else {
        REQUIRE(fern_result_unwrap(status) == FERN_ERR_OUT_OF_MEMORY);
        if (inject == 2) REQUIRE(signal_is(observer, "Exit", root, reason) == 0);
    }
    REQUIRE(fern_actor_mailbox_len(observer) == 0);
    REQUIRE(fern_actor_scheduler_next() == 0);
    REQUIRE(!fern_result_is_ok(fern_actor_restart(leaf)));
    REQUIRE(!fern_result_is_ok(fern_actor_restart(middle)));
    int64_t restarted = fern_actor_restart(root);
    REQUIRE(fern_result_is_ok(restarted));
    REQUIRE(!fern_result_is_ok(fern_actor_restart(root)));
    REQUIRE(!fern_result_is_ok(fern_actor_restart(middle)));
    return 0;
}
static int strategy_tree(void) {
    int64_t root = fern_actor_spawn("root"), a = fern_actor_spawn("a");
    int64_t b = fern_actor_spawn("b"), leaf = fern_actor_spawn("leaf");
    REQUIRE(fern_result_is_ok(fern_actor_supervise_one_for_all(root, a, 10, 60)));
    REQUIRE(fern_result_is_ok(fern_actor_supervise_one_for_all(root, b, 10, 60)));
    REQUIRE(fern_result_is_ok(fern_actor_supervise(b, leaf, 10, 60)));
    REQUIRE(fern_result_is_ok(fern_actor_send(leaf, "pending")));
    REQUIRE(fern_actor_set_current(leaf) == 0);
    REQUIRE(fern_result_is_ok(fern_actor_exit(a, "failure")));
    REQUIRE(dead(leaf) == 0);
    REQUIRE(fern_actor_self() == 0);
    REQUIRE(fern_actor_mailbox_len(root) == 4);
    return 0;
}
static int deep_tree(void) {
    int64_t root = fern_actor_spawn("root"), last = root;
    for (int i = 0; i < 2048; i++) {
        int64_t next = fern_actor_spawn("child");
        REQUIRE(fern_result_is_ok(fern_actor_supervise(last, next, 10, 60)));
        last = next;
    }
    REQUIRE(fern_actor_set_current(last) == 0);
    REQUIRE(fern_result_is_ok(fern_actor_send(last, "pending")));
    REQUIRE(fern_result_is_ok(fern_actor_exit(root, "shutdown")));
    REQUIRE(dead(last) == 0);
    REQUIRE(fern_actor_self() == 0);
    REQUIRE(fern_actor_scheduler_next() == 0);
    return 0;
}
static int preterminated(void) {
    int64_t observer = fern_actor_spawn("observer"), root = fern_actor_spawn("root");
    int64_t stopped = fern_actor_spawn("stopped"), child = fern_actor_spawn("child");
    int64_t branch = fern_actor_spawn("branch"), leaf = fern_actor_spawn("leaf");
    int64_t ids[] = {root, stopped, child, branch, leaf};
    REQUIRE(fern_result_is_ok(fern_actor_supervise(root, branch, 10, 60)));
    REQUIRE(fern_result_is_ok(fern_actor_supervise(branch, leaf, 10, 60)));
    REQUIRE(fern_result_is_ok(fern_actor_supervise(root, stopped, 10, 60)));
    REQUIRE(fern_result_is_ok(fern_actor_supervise(stopped, child, 10, 60)));
    for (size_t i = 0; i < 5; i++) REQUIRE(fern_result_is_ok(fern_actor_monitor(observer, ids[i])));
    REQUIRE(fern_result_is_ok(fern_actor_exit(stopped, "normal")));
    REQUIRE(signal_is(observer, "DOWN", stopped, "normal") == 0);
    REQUIRE(signal_is(observer, "DOWN", child, "shutdown") == 0);
    REQUIRE(fern_actor_set_current(observer) == 0);
    REQUIRE(fern_result_is_ok(fern_actor_exit(root, "shutdown")));
    REQUIRE(fern_actor_self() == observer);
    REQUIRE(signal_is(observer, "DOWN", root, "shutdown") == 0);
    REQUIRE(signal_is(observer, "DOWN", branch, "shutdown") == 0);
    REQUIRE(signal_is(observer, "DOWN", leaf, "shutdown") == 0);
    REQUIRE(fern_actor_mailbox_len(observer) == 0);
    REQUIRE(fern_actor_scheduler_next() == 0);
    for (size_t i = 0; i < 5; i++) REQUIRE(dead(ids[i]) == 0);
    return 0;
}
static int restart_atomic(void) {
    int64_t observer = fern_actor_spawn("observer"), worker = fern_actor_spawn("worker");
    REQUIRE(fern_result_is_ok(fern_actor_monitor(observer, worker)));
    REQUIRE(fern_result_is_ok(fern_actor_exit(worker, "normal")));
    int64_t length = g_actor_runtime.actor_len, next = g_actor_runtime.next_actor_id;
    fail_next = true;
    int64_t status = fern_actor_restart(worker);
    REQUIRE(!fern_result_is_ok(status));
    REQUIRE(fern_result_unwrap(status) == FERN_ERR_OUT_OF_MEMORY);
    REQUIRE(g_actor_runtime.actor_len == length);
    REQUIRE(g_actor_runtime.next_actor_id == next);
    REQUIRE(fern_actor_lookup(&g_actor_runtime, worker)->replacement_id == 0);
    REQUIRE(fern_result_is_ok(fern_actor_restart(worker)));
    return 0;
}
static int spawn_atomic(void) {
    int64_t worker = fern_actor_spawn("worker");
    int64_t length = g_actor_runtime.actor_len, next = g_actor_runtime.next_actor_id;
    fail_copy_after = 1;
    REQUIRE(fern_actor_spawn("failure") == 0);
    REQUIRE(g_actor_runtime.actor_len == length);
    REQUIRE(g_actor_runtime.next_actor_id == next);
    REQUIRE(fern_actor_mailbox_len(worker) == 0);
    REQUIRE(fern_actor_spawn("success") == next);
    return 0;
}
int fern_main(void) {
    const char* mode = getenv("FERN_SUBTREE_MODE");
    if (!mode) return 2;
    if (strcmp(mode, "fault") == 0) return tree("failure", 1);
    if (strcmp(mode, "send-fault") == 0) return tree("failure", 2);
    if (strcmp(mode, "preterminated") == 0) return preterminated();
    if (strcmp(mode, "restart-atomic") == 0) return restart_atomic();
    if (strcmp(mode, "spawn-atomic") == 0) return spawn_atomic();
    if (strcmp(mode, "strategy") == 0) return strategy_tree();
    if (strcmp(mode, "deep") == 0) return deep_tree();
    return tree(mode, false);
}
