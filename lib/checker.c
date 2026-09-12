/* Fern Type Checker Implementation */

#include "checker.h"
#include <assert.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/**
 * Error message storage node.
 */
typedef struct ErrorNode {
    String* message;
    struct ErrorNode* next;
} ErrorNode;

/**
 * The type checker context.
 */
struct Checker {
    Arena* arena;
    TypeEnv* env;
    ErrorNode* errors;
    ErrorNode* errors_tail;
    Type* current_return_type;
    SourceLoc current_expr_loc;
    bool require_executable;
};

/* ========== Forward Declarations ========== */

static bool bind_pattern(Checker* checker, Pattern* pattern, Type* type);
static bool unify(Type* a, Type* b);
static Type* substitute(Arena* arena, Type* type);
static Type* check_pipe_expr(Checker* checker, BinaryExpr* expr);
static Type* check_lambda_against(Checker* checker, LambdaExpr* lambda, Type* expected_fn_type);
static bool is_catch_all_pattern(Pattern* pattern);
static bool check_match_exhaustiveness(Checker* checker, MatchExpr* expr, Type* scrutinee_type);
static Type* require_result_propagation_context(Checker* checker, SourceLoc loc, Type* propagated_err, const char* op_name);
static Type* check_record_update_expr(Checker* checker, RecordUpdateExpr* expr, SourceLoc loc);
static Type* check_spawn_expr(Checker* checker, SpawnExpr* expr, SourceLoc loc);
static Type* check_spawn_link_call_expr(Checker* checker, CallExpr* expr, SourceLoc loc);
static Type* check_send_expr(Checker* checker, SendExpr* expr, SourceLoc loc);
static Type* check_receive_expr(Checker* checker, ReceiveExpr* expr, SourceLoc loc);

/**
 * Map from old var id to new type variable for instantiation.
 */
typedef struct VarMapping {
    int old_id;
    Type* new_var;
    struct VarMapping* next;
} VarMapping;

static Type* instantiate_type(Arena* arena, Type* type, VarMapping** map);

/* ========== Helper Functions ========== */

/**
 * Add an error message to the checker's error list.
 * @param checker The type checker context.
 * @param fmt The format string.
 * @param ... The format arguments.
 */
static void add_error(Checker* checker, const char* fmt, ...) {
    assert(checker != NULL);
    assert(fmt != NULL);
    va_list args;
    va_start(args, fmt);
    
    /* Format the error message */
    char buf[1024];
    vsnprintf(buf, sizeof(buf), fmt, args);
    va_end(args);
    
    ErrorNode* node = arena_alloc(checker->arena, sizeof(ErrorNode));
    node->message = string_new(checker->arena, buf);
    node->next = NULL;
    
    if (checker->errors_tail) {
        checker->errors_tail->next = node;
        checker->errors_tail = node;
    } else {
        checker->errors = node;
        checker->errors_tail = node;
    }
}

/**
 * Add a located error message to the checker's error list.
 * @param checker The type checker context.
 * @param loc Source location for this error.
 * @param fmt The format string.
 * @param ... The format arguments.
 */
static void add_error_at(Checker* checker, SourceLoc loc, const char* fmt, ...) {
    assert(checker != NULL);
    assert(fmt != NULL);
    va_list args;
    va_start(args, fmt);

    char msg_buf[1024];
    vsnprintf(msg_buf, sizeof(msg_buf), fmt, args);
    va_end(args);

    char full_buf[1280];
    if (loc.filename && loc.line > 0) {
        snprintf(full_buf, sizeof(full_buf), "%s:%zu:%zu: %s",
            string_cstr(loc.filename), loc.line, loc.column, msg_buf);
    } else if (loc.line > 0) {
        snprintf(full_buf, sizeof(full_buf), "%zu:%zu: %s",
            loc.line, loc.column, msg_buf);
    } else {
        snprintf(full_buf, sizeof(full_buf), "%s", msg_buf);
    }

    add_error(checker, "%s", full_buf);
}

/**
 * Create an error type and add error message.
 * @param checker The type checker context.
 * @param fmt The format string.
 * @param ... The format arguments.
 * @return The error type.
 */
static Type* error_type(Checker* checker, const char* fmt, ...) {
    assert(checker != NULL);
    assert(fmt != NULL);
    va_list args;
    va_start(args, fmt);
    
    char msg_buf[1024];
    vsnprintf(msg_buf, sizeof(msg_buf), fmt, args);
    va_end(args);

    SourceLoc loc = checker->current_expr_loc;
    char full_buf[1280];
    if (loc.filename && loc.line > 0) {
        snprintf(full_buf, sizeof(full_buf), "%s:%zu:%zu: %s",
            string_cstr(loc.filename), loc.line, loc.column, msg_buf);
    } else if (loc.line > 0) {
        snprintf(full_buf, sizeof(full_buf), "%zu:%zu: %s",
            loc.line, loc.column, msg_buf);
    } else {
        snprintf(full_buf, sizeof(full_buf), "%s", msg_buf);
    }

    add_error(checker, "%s", full_buf);
    return type_error(checker->arena, string_new(checker->arena, full_buf));
}

/**
 * Create an error type with source location.
 * @param checker The type checker context.
 * @param loc The source location.
 * @param fmt The format string.
 * @param ... The format arguments.
 * @return The error type.
 */
static Type* error_type_at(Checker* checker, SourceLoc loc, const char* fmt, ...) {
    assert(checker != NULL);
    assert(fmt != NULL);
    va_list args;
    va_start(args, fmt);
    
    char msg_buf[1024];
    vsnprintf(msg_buf, sizeof(msg_buf), fmt, args);
    va_end(args);
    
    /* Format with location: "file:line:col: error message" */
    char full_buf[1280];
    if (loc.filename && loc.line > 0) {
        snprintf(full_buf, sizeof(full_buf), "%s:%zu:%zu: %s",
            string_cstr(loc.filename), loc.line, loc.column, msg_buf);
    } else if (loc.line > 0) {
        snprintf(full_buf, sizeof(full_buf), "%zu:%zu: %s",
            loc.line, loc.column, msg_buf);
    } else {
        snprintf(full_buf, sizeof(full_buf), "%s", msg_buf);
    }
    
    add_error(checker, "%s", full_buf);
    return type_error(checker->arena, string_new(checker->arena, full_buf));
}

/* ========== Built-in Module System ========== */

/**
 * Try to build a module path from a dot expression.
 * For example: Tui.Panel -> "Tui.Panel", String -> "String"
 * Returns NULL if the expression is not a valid module path.
 * @param arena Arena for string allocation.
 * @param expr The expression to check.
 * @return Module path string or NULL.
 */
static String* try_build_module_path(Arena* arena, Expr* expr) {
    assert(arena != NULL);
    assert(expr != NULL);
    
    if (expr->type == EXPR_IDENT) {
        /* Simple identifier: "String", "Tui", etc. */
        return expr->data.ident.name;
    }
    
    if (expr->type == EXPR_DOT) {
        /* Nested dot: try to build path from left side, then append right */
        String* left_path = try_build_module_path(arena, expr->data.dot.object);
        if (left_path == NULL) return NULL;
        
        /* Build "Left.Right" */
        const char* left_str = string_cstr(left_path);
        const char* right_str = string_cstr(expr->data.dot.field);
        size_t left_len = strlen(left_str);
        size_t right_len = strlen(right_str);
        
        char* buf = arena_alloc(arena, left_len + 1 + right_len + 1);
        memcpy(buf, left_str, left_len);
        buf[left_len] = '.';
        memcpy(buf + left_len + 1, right_str, right_len);
        buf[left_len + 1 + right_len] = '\0';
        
        return string_new(arena, buf);
    }
    
    return NULL;
}

/**
 * Canonicalize built-in module names, including compatibility aliases.
 * @param name Module name from source.
 * @return Canonical module name or NULL if not built-in.
 */
static const char* canonical_builtin_module_name(const char* name) {
    assert(name != NULL);
    assert(name[0] != '\0');
    if (strcmp(name, "String") == 0 || strcmp(name, "List") == 0 ||
        strcmp(name, "System") == 0 || strcmp(name, "Regex") == 0 ||
        strcmp(name, "Result") == 0 || strcmp(name, "Option") == 0 ||
        strcmp(name, "Tui.Term") == 0 || strcmp(name, "Tui.Panel") == 0 ||
        strcmp(name, "Tui.Table") == 0 || strcmp(name, "Tui.Style") == 0 ||
        strcmp(name, "Tui.Status") == 0 || strcmp(name, "Tui.Live") == 0 ||
        strcmp(name, "Tui.Progress") == 0 || strcmp(name, "Tui.Spinner") == 0 ||
        strcmp(name, "Tui.Prompt") == 0 || strcmp(name, "Tui.Tree") == 0 ||
        strcmp(name, "Tui.Log") == 0) {
        return name;
    }
    if (strcmp(name, "File") == 0 || strcmp(name, "fs") == 0) return "File";
    if (strcmp(name, "json") == 0 || strcmp(name, "Json") == 0) return "json";
    if (strcmp(name, "http") == 0 || strcmp(name, "Http") == 0) return "http";
    if (strcmp(name, "sql") == 0 || strcmp(name, "Sql") == 0) return "sql";
    if (strcmp(name, "actors") == 0 || strcmp(name, "Actors") == 0) return "actors";
    return NULL;
}

/**
 * Check if a name is a built-in module.
 * Supports nested modules like "Tui.Panel".
 * @param name The name to check.
 * @return True if the name is a built-in module.
 */
static bool is_builtin_module(const char* name) {
    assert(name != NULL);
    assert(name[0] != '\0');  /* Name must be non-empty */
    assert(strlen(name) < 256);
    return canonical_builtin_module_name(name) != NULL;
}

/**
 * Look up a function type from a built-in module.
 * @param checker The type checker context.
 * @param module The module name (e.g., "String").
 * @param func The function name (e.g., "len").
 * @return The function type, or NULL if not found.
 */
static Type* lookup_module_function(Checker* checker, const char* module, const char* func) {
    // FERN_STYLE: allow(function-length) module function lookup handles all built-in modules
    assert(checker != NULL);
    assert(module != NULL);
    assert(func != NULL);
    module = canonical_builtin_module_name(module);
    if (module == NULL) return NULL;
    Arena* arena = checker->arena;
    TypeVec* params;
    Type* var;
    TypeVec* type_args;
    Type* list_type;
    TypeVec* result_args;
    Type* result_type;

    /* C packed Options support the workflow's byte/index Int payloads. */
    if (strcmp(module, "Option") == 0 && strcmp(func, "unwrap_or") == 0) {
        params = TypeVec_new(arena);
        TypeVec_push(arena, params, type_option(arena, type_int(arena)));
        TypeVec_push(arena, params, type_int(arena));
        return type_fn(arena, params, type_int(arena));
    }

    /* ===== String module ===== */
    if (strcmp(module, "String") == 0) {
        /* String.len(String) -> Int */
        if (strcmp(func, "len") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_int(arena));
        }
        /* String.concat(String, String) -> String */
        if (strcmp(func, "concat") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* String.eq(String, String) -> Bool */
        if (strcmp(func, "eq") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_bool(arena));
        }
        /* String.starts_with(String, String) -> Bool */
        if (strcmp(func, "starts_with") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_bool(arena));
        }
        /* String.ends_with(String, String) -> Bool */
        if (strcmp(func, "ends_with") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_bool(arena));
        }
        /* String.contains(String, String) -> Bool */
        if (strcmp(func, "contains") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_bool(arena));
        }
        /* String.slice(String, Int, Int) -> String */
        if (strcmp(func, "slice") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* String.trim(String) -> String */
        if (strcmp(func, "trim") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* String.trim_start(String) -> String */
        if (strcmp(func, "trim_start") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* String.trim_end(String) -> String */
        if (strcmp(func, "trim_end") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* String.to_upper(String) -> String */
        if (strcmp(func, "to_upper") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* String.to_lower(String) -> String */
        if (strcmp(func, "to_lower") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* String.replace(String, String, String) -> String */
        if (strcmp(func, "replace") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* String.repeat(String, Int) -> String */
        if (strcmp(func, "repeat") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* String.is_empty(String) -> Bool */
        if (strcmp(func, "is_empty") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_bool(arena));
        }
        /* String.is_decimal(String) -> Bool */
        if (strcmp(func, "is_decimal") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_bool(arena));
        }
        /* String.split(String, String) -> List(String) */
        if (strcmp(func, "split") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_list(arena, type_string(arena)));
        }
        /* String.lines(String) -> List(String) */
        if (strcmp(func, "lines") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_list(arena, type_string(arena)));
        }
        /* String.index_of(String, String) -> Option(Int) */
        if (strcmp(func, "index_of") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            TypeVec* option_args = TypeVec_new(arena);
            TypeVec_push(arena, option_args, type_int(arena));
            return type_fn(arena, params, type_con(arena, string_new(arena, "Option"), option_args));
        }
        /* String.char_at(String, Int) -> Option(Int) */
        if (strcmp(func, "char_at") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec* option_args = TypeVec_new(arena);
            TypeVec_push(arena, option_args, type_int(arena));
            return type_fn(arena, params, type_con(arena, string_new(arena, "Option"), option_args));
        }
        /* String.join(List(String), String) -> String */
        if (strcmp(func, "join") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_list(arena, type_string(arena)));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
    }

    /* ===== List module ===== */
    if (strcmp(module, "List") == 0) {
        /* List.len(List(a)) -> Int */
        if (strcmp(func, "len") == 0) {
            params = TypeVec_new(arena);
            var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
            type_args = TypeVec_new(arena);
            TypeVec_push(arena, type_args, var);
            list_type = type_con(arena, string_new(arena, "List"), type_args);
            TypeVec_push(arena, params, list_type);
            return type_fn(arena, params, type_int(arena));
        }
        /* List.get(List(a), Int) -> a */
        if (strcmp(func, "get") == 0) {
            params = TypeVec_new(arena);
            var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
            type_args = TypeVec_new(arena);
            TypeVec_push(arena, type_args, var);
            list_type = type_con(arena, string_new(arena, "List"), type_args);
            TypeVec_push(arena, params, list_type);
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, var);
        }
        /* List.push(List(a), a) -> List(a) */
        if (strcmp(func, "push") == 0) {
            params = TypeVec_new(arena);
            var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
            type_args = TypeVec_new(arena);
            TypeVec_push(arena, type_args, var);
            list_type = type_con(arena, string_new(arena, "List"), type_args);
            TypeVec_push(arena, params, list_type);
            TypeVec_push(arena, params, var);
            return type_fn(arena, params, list_type);
        }
        /* List.reverse(List(a)) -> List(a) */
        if (strcmp(func, "reverse") == 0) {
            params = TypeVec_new(arena);
            var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
            type_args = TypeVec_new(arena);
            TypeVec_push(arena, type_args, var);
            list_type = type_con(arena, string_new(arena, "List"), type_args);
            TypeVec_push(arena, params, list_type);
            return type_fn(arena, params, list_type);
        }
        /* List.concat(List(a), List(a)) -> List(a) */
        if (strcmp(func, "concat") == 0) {
            params = TypeVec_new(arena);
            var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
            type_args = TypeVec_new(arena);
            TypeVec_push(arena, type_args, var);
            list_type = type_con(arena, string_new(arena, "List"), type_args);
            TypeVec_push(arena, params, list_type);
            TypeVec_push(arena, params, list_type);
            return type_fn(arena, params, list_type);
        }
        /* List.head(List(a)) -> a */
        if (strcmp(func, "head") == 0) {
            params = TypeVec_new(arena);
            var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
            type_args = TypeVec_new(arena);
            TypeVec_push(arena, type_args, var);
            list_type = type_con(arena, string_new(arena, "List"), type_args);
            TypeVec_push(arena, params, list_type);
            return type_fn(arena, params, var);
        }
        /* List.tail(List(a)) -> List(a) */
        if (strcmp(func, "tail") == 0) {
            params = TypeVec_new(arena);
            var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
            type_args = TypeVec_new(arena);
            TypeVec_push(arena, type_args, var);
            list_type = type_con(arena, string_new(arena, "List"), type_args);
            TypeVec_push(arena, params, list_type);
            return type_fn(arena, params, list_type);
        }
        /* List.is_empty(List(a)) -> Bool */
        if (strcmp(func, "is_empty") == 0) {
            params = TypeVec_new(arena);
            var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
            type_args = TypeVec_new(arena);
            TypeVec_push(arena, type_args, var);
            list_type = type_con(arena, string_new(arena, "List"), type_args);
            TypeVec_push(arena, params, list_type);
            return type_fn(arena, params, type_bool(arena));
        }
        /* List.contains(List(a), a) -> Bool */
        if (strcmp(func, "contains") == 0) {
            params = TypeVec_new(arena);
            var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
            type_args = TypeVec_new(arena);
            TypeVec_push(arena, type_args, var);
            list_type = type_con(arena, string_new(arena, "List"), type_args);
            TypeVec_push(arena, params, list_type);
            TypeVec_push(arena, params, var);
            return type_fn(arena, params, type_bool(arena));
        }
        /* List.any(List(a), (a) -> Bool) -> Bool */
        if (strcmp(func, "any") == 0) {
            params = TypeVec_new(arena);
            var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
            type_args = TypeVec_new(arena);
            TypeVec_push(arena, type_args, var);
            list_type = type_con(arena, string_new(arena, "List"), type_args);
            TypeVec_push(arena, params, list_type);
            /* The predicate function: (a) -> Bool */
            TypeVec* pred_params = TypeVec_new(arena);
            TypeVec_push(arena, pred_params, var);
            Type* pred_type = type_fn(arena, pred_params, type_bool(arena));
            TypeVec_push(arena, params, pred_type);
            return type_fn(arena, params, type_bool(arena));
        }
        /* List.all(List(a), (a) -> Bool) -> Bool */
        if (strcmp(func, "all") == 0) {
            params = TypeVec_new(arena);
            var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
            type_args = TypeVec_new(arena);
            TypeVec_push(arena, type_args, var);
            list_type = type_con(arena, string_new(arena, "List"), type_args);
            TypeVec_push(arena, params, list_type);
            /* The predicate function: (a) -> Bool */
            TypeVec* pred_params = TypeVec_new(arena);
            TypeVec_push(arena, pred_params, var);
            Type* pred_type = type_fn(arena, pred_params, type_bool(arena));
            TypeVec_push(arena, params, pred_type);
            return type_fn(arena, params, type_bool(arena));
        }
    }

    /* ===== File module ===== */
    if (strcmp(module, "File") == 0) {
        /* File.read(String) -> Result(String, Int) */
        if (strcmp(func, "read") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_string(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* File.write(String, String) -> Result(Int, Int) */
        if (strcmp(func, "write") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* File.append(String, String) -> Result(Int, Int) */
        if (strcmp(func, "append") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* File.exists(String) -> Bool */
        if (strcmp(func, "exists") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_bool(arena));
        }
        /* File.delete(String) -> Result(Int, Int) */
        if (strcmp(func, "delete") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* File.size(String) -> Result(Int, Int) */
        if (strcmp(func, "size") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* File.is_dir(String) -> Bool */
        if (strcmp(func, "is_dir") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_bool(arena));
        }
        /* File.list_dir(String) -> Result(List(String), Int) */
        if (strcmp(func, "list_dir") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_list(arena, type_string(arena)));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
    }

    /* ===== System module ===== */
    if (strcmp(module, "System") == 0) {
        /* System.args() -> List(String) */
        if (strcmp(func, "args") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, type_list(arena, type_string(arena)));
        }
        /* System.args_count() -> Int */
        if (strcmp(func, "args_count") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, type_int(arena));
        }
        /* System.arg(Int) -> String */
        if (strcmp(func, "arg") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* System.exit(Int) -> Unit */
        if (strcmp(func, "exit") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, type_unit(arena));
        }
        /* System.write_stderr(String) -> Result(Unit, Int), without an added newline. */
        if (strcmp(func, "write_stderr") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_result(arena, type_unit(arena), type_int(arena)));
        }
        /* System.exec(String) -> (Int, String, String) - exit code, stdout, stderr */
        if (strcmp(func, "exec") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec* tuple_elems = TypeVec_new(arena);
            TypeVec_push(arena, tuple_elems, type_int(arena));
            TypeVec_push(arena, tuple_elems, type_string(arena));
            TypeVec_push(arena, tuple_elems, type_string(arena));
            return type_fn(arena, params, type_tuple(arena, tuple_elems));
        }
        /* Bounded capture reports policy failures separately from normal child statuses. */
        if (strcmp(func, "exec_args_bounded") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_list(arena, type_string(arena)));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec* fields = TypeVec_new(arena);
            TypeVec_push(arena, fields, type_int(arena));
            TypeVec_push(arena, fields, type_string(arena));
            TypeVec_push(arena, fields, type_string(arena));
            Type* result = type_result(arena, type_tuple(arena, fields), type_int(arena));
            return type_fn(arena, params, result);
        }
        /* System.exec_args(List(String)) -> (Int, String, String) */
        if (strcmp(func, "exec_args") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_list(arena, type_string(arena)));
            TypeVec* tuple_elems = TypeVec_new(arena);
            TypeVec_push(arena, tuple_elems, type_int(arena));
            TypeVec_push(arena, tuple_elems, type_string(arena));
            TypeVec_push(arena, tuple_elems, type_string(arena));
            return type_fn(arena, params, type_tuple(arena, tuple_elems));
        }
        /* System.getenv(String) -> String */
        if (strcmp(func, "getenv") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* System.setenv(String, String) -> Int */
        if (strcmp(func, "setenv") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_int(arena));
        }
        /* System.cwd() -> String */
        if (strcmp(func, "cwd") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, type_string(arena));
        }
        /* System.chdir(String) -> Int */
        if (strcmp(func, "chdir") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_int(arena));
        }
        /* System.hostname() -> String */
        if (strcmp(func, "hostname") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, type_string(arena));
        }
        /* System.user() -> String */
        if (strcmp(func, "user") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, type_string(arena));
        }
        /* System.home() -> String */
        if (strcmp(func, "home") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, type_string(arena));
        }
    }

    /* ===== Regex module ===== */
    if (strcmp(module, "Regex") == 0) {
        /* Regex.is_match(String, String) -> Bool */
        if (strcmp(func, "is_match") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_bool(arena));
        }
        /* Regex.find(String, String) -> Option((Int, Int, String)) */
        if (strcmp(func, "find") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            /* Return match as (start, end, matched_text) tuple wrapped in Option */
            TypeVec* match_elems = TypeVec_new(arena);
            TypeVec_push(arena, match_elems, type_int(arena));
            TypeVec_push(arena, match_elems, type_int(arena));
            TypeVec_push(arena, match_elems, type_string(arena));
            TypeVec* option_args = TypeVec_new(arena);
            TypeVec_push(arena, option_args, type_tuple(arena, match_elems));
            return type_fn(arena, params, type_con(arena, string_new(arena, "Option"), option_args));
        }
        /* Regex.find_all(String, String) -> List(String) */
        if (strcmp(func, "find_all") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_list(arena, type_string(arena)));
        }
        /* Regex.replace(String, String, String) -> String */
        if (strcmp(func, "replace") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* Regex.replace_all(String, String, String) -> String */
        if (strcmp(func, "replace_all") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* Regex.split(String, String) -> List(String) */
        if (strcmp(func, "split") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_list(arena, type_string(arena)));
        }
        /* Regex.captures(String, String) -> List((Int, Int, String)) */
        if (strcmp(func, "captures") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            TypeVec* match_elems = TypeVec_new(arena);
            TypeVec_push(arena, match_elems, type_int(arena));
            TypeVec_push(arena, match_elems, type_int(arena));
            TypeVec_push(arena, match_elems, type_string(arena));
            return type_fn(arena, params, type_list(arena, type_tuple(arena, match_elems)));
        }
    }

    /* Immutable tree construction and plain structured log records. */
    if (strcmp(module, "Tui.Tree") == 0) {
        Type* tree_type = type_con(arena, string_new(arena, "Tree"), NULL);
        if (strcmp(func, "new") == 0 || strcmp(func, "render") == 0 ||
            strcmp(func, "add") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, strcmp(func, "new") == 0 ?
                type_string(arena) : tree_type);
            if (strcmp(func, "add") == 0) TypeVec_push(arena, params, tree_type);
            return type_fn(arena, params, strcmp(func, "render") == 0 ?
                type_string(arena) : tree_type);
        }
    }
    if (strcmp(module, "Tui.Log") == 0 &&
        (strcmp(func, "debug") == 0 || strcmp(func, "info") == 0 ||
         strcmp(func, "warn") == 0 || strcmp(func, "error") == 0)) {
        params = TypeVec_new(arena);
        TypeVec_push(arena, params, type_string(arena));
        return type_fn(arena, params, type_string(arena));
    }

    /* ===== Tui.Term module ===== */
    if (strcmp(module, "Tui.Term") == 0) {
        if (strcmp(func, "move_to") == 0 || strcmp(func, "up") == 0 ||
            strcmp(func, "down") == 0 || strcmp(func, "left") == 0 ||
            strcmp(func, "right") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            if (strcmp(func, "move_to") == 0) TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, type_unit(arena));
        }
        if (strcmp(func, "clear") == 0 || strcmp(func, "hide_cursor") == 0 ||
            strcmp(func, "show_cursor") == 0 || strcmp(func, "save_cursor") == 0 ||
            strcmp(func, "restore_cursor") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, type_unit(arena));
        }
        /* Tui.Term.size() -> (Int, Int) */
        if (strcmp(func, "size") == 0) {
            params = TypeVec_new(arena);
            TypeVec* tuple_elems = TypeVec_new(arena);
            TypeVec_push(arena, tuple_elems, type_int(arena));
            TypeVec_push(arena, tuple_elems, type_int(arena));
            return type_fn(arena, params, type_tuple(arena, tuple_elems));
        }
        /* Tui.Term.is_tty() -> Bool */
        if (strcmp(func, "is_tty") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, type_bool(arena));
        }
        /* Tui.Term.color_support() -> Int */
        if (strcmp(func, "color_support") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, type_int(arena));
        }
    }

    /* ===== json module ===== */
    if (strcmp(module, "json") == 0) {
        /* json.parse(String) -> Result(String, Int) */
        if (strcmp(func, "parse") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_string(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* json.stringify(String) -> Result(String, Int) */
        if (strcmp(func, "stringify") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_string(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
    }

    /* ===== http module ===== */
    if (strcmp(module, "http") == 0) {
        /* http.get(String) -> Result(String, Int) */
        if (strcmp(func, "get") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_string(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* http.post(String, String) -> Result(String, Int) */
        if (strcmp(func, "post") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_string(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
    }

    /* ===== sql module ===== */
    if (strcmp(module, "sql") == 0) {
        /* sql.close(Int) -> Result(Int, Int), returning Ok(0) on completion. */
        if (strcmp(func, "close") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }

        /* sql.open(String) -> Result(Int, Int) */
        if (strcmp(func, "open") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* sql.execute(Int, String) -> Result(Int, Int) */
        if (strcmp(func, "execute") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
    }

    /* ===== actors module ===== */
    if (strcmp(module, "actors") == 0) {
        /* actors.start(String) -> Int */
        if (strcmp(func, "start") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_int(arena));
        }
        /* actors.post(Int, String) -> Result(Int, Int) */
        if (strcmp(func, "post") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_string(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* actors.next(Int) -> Result(String, Int) */
        if (strcmp(func, "next") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_string(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* actors.monitor(Int, Int) -> Result(Int, Int) */
        if (strcmp(func, "monitor") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* actors.demonitor(Int, Int) -> Result(Int, Int) */
        if (strcmp(func, "demonitor") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* actors.restart(Int) -> Result(Int, Int) */
        if (strcmp(func, "restart") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* actors.supervise(Int, Int, Int, Int) -> Result(Int, Int) */
        if (strcmp(func, "supervise") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* actors.supervise_one_for_all(Int, Int, Int, Int) -> Result(Int, Int) */
        if (strcmp(func, "supervise_one_for_all") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
        /* actors.supervise_rest_for_one(Int, Int, Int, Int) -> Result(Int, Int) */
        if (strcmp(func, "supervise_rest_for_one") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            result_args = TypeVec_new(arena);
            TypeVec_push(arena, result_args, type_int(arena));
            TypeVec_push(arena, result_args, type_int(arena));
            result_type = type_con(arena, string_new(arena, "Result"), result_args);
            return type_fn(arena, params, result_type);
        }
    }

    /* ===== Tui.Style module ===== */
    if (strcmp(module, "Tui.Style") == 0) {
        /* All basic color functions: Style.red(String) -> String, etc. */
        if (strcmp(func, "black") == 0 || strcmp(func, "red") == 0 ||
            strcmp(func, "green") == 0 || strcmp(func, "yellow") == 0 ||
            strcmp(func, "blue") == 0 || strcmp(func, "magenta") == 0 ||
            strcmp(func, "cyan") == 0 || strcmp(func, "white") == 0 ||
            strcmp(func, "bright_black") == 0 || strcmp(func, "bright_red") == 0 ||
            strcmp(func, "bright_green") == 0 || strcmp(func, "bright_yellow") == 0 ||
            strcmp(func, "bright_blue") == 0 || strcmp(func, "bright_magenta") == 0 ||
            strcmp(func, "bright_cyan") == 0 || strcmp(func, "bright_white") == 0 ||
            strcmp(func, "on_black") == 0 || strcmp(func, "on_red") == 0 ||
            strcmp(func, "on_green") == 0 || strcmp(func, "on_yellow") == 0 ||
            strcmp(func, "on_blue") == 0 || strcmp(func, "on_magenta") == 0 ||
            strcmp(func, "on_cyan") == 0 || strcmp(func, "on_white") == 0 ||
            strcmp(func, "bold") == 0 || strcmp(func, "dim") == 0 ||
            strcmp(func, "italic") == 0 || strcmp(func, "underline") == 0 ||
            strcmp(func, "blink") == 0 || strcmp(func, "reverse") == 0 ||
            strcmp(func, "strikethrough") == 0 || strcmp(func, "reset") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* Style.color(String, Int) -> String */
        if (strcmp(func, "color") == 0 || strcmp(func, "on_color") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* Style.rgb(String, Int, Int, Int) -> String */
        if (strcmp(func, "rgb") == 0 || strcmp(func, "on_rgb") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* Style.hex(String, String) -> String */
        if (strcmp(func, "hex") == 0 || strcmp(func, "on_hex") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
    }

    /* ===== Tui.Status module (badges) ===== */
    if (strcmp(module, "Tui.Status") == 0) {
        /* Status.warn(String) -> String */
        /* Status.ok(String) -> String */
        /* Status.info(String) -> String */
        /* Status.error(String) -> String */
        /* Status.debug(String) -> String */
        if (strcmp(func, "warn") == 0 || strcmp(func, "ok") == 0 ||
            strcmp(func, "info") == 0 || strcmp(func, "error") == 0 ||
            strcmp(func, "debug") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
    }

    /* ===== Tui.Live module (same-line updates) ===== */
    if (strcmp(module, "Tui.Live") == 0) {
        /* Live.print(String) -> Unit - print without newline */
        if (strcmp(func, "print") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_unit(arena));
        }
        /* Live.clear_line() -> Unit - clear current line */
        if (strcmp(func, "clear_line") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, type_unit(arena));
        }
        /* Live.update(String) -> Unit - clear line and print */
        if (strcmp(func, "update") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_unit(arena));
        }
        /* Live.done() -> Unit - finish with newline */
        if (strcmp(func, "done") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, type_unit(arena));
        }
        /* Live.sleep(Int) -> Unit - sleep for milliseconds */
        if (strcmp(func, "sleep") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, type_unit(arena));
        }
    }

    /* ===== Tui.Panel module ===== */
    if (strcmp(module, "Tui.Panel") == 0) {
        Type* panel_type = type_con(arena, string_new(arena, "Panel"), NULL);
        /* Panel.new(String) -> Panel - create panel with content */
        if (strcmp(func, "new") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, panel_type);
        }
        /* Panel.title(Panel, String) -> Panel - set title */
        if (strcmp(func, "title") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, panel_type);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, panel_type);
        }
        /* Panel.subtitle(Panel, String) -> Panel - set subtitle */
        if (strcmp(func, "subtitle") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, panel_type);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, panel_type);
        }
        /* Panel.border(Panel, String) -> Panel - set border style */
        if (strcmp(func, "border") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, panel_type);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, panel_type);
        }
        /* Panel.width(Panel, Int) -> Panel - set width */
        if (strcmp(func, "width") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, panel_type);
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, panel_type);
        }
        /* Panel.padding(Panel, Int) -> Panel - set padding */
        if (strcmp(func, "padding") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, panel_type);
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, panel_type);
        }
        /* Panel.border_color(Panel, String) -> Panel - set border color */
        if (strcmp(func, "border_color") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, panel_type);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, panel_type);
        }
        /* Panel.render(Panel) -> String - render to string */
        if (strcmp(func, "render") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, panel_type);
            return type_fn(arena, params, type_string(arena));
        }
    }

    /* ===== Tui.Table module ===== */
    if (strcmp(module, "Tui.Table") == 0) {
        Type* table_type = type_con(arena, string_new(arena, "Table"), NULL);
        /* Table.new() -> Table - create empty table */
        if (strcmp(func, "new") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, table_type);
        }
        /* Table.add_column(Table, String) -> Table - add column header */
        if (strcmp(func, "add_column") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, table_type);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, table_type);
        }
        /* Table.add_row(Table, List(String)) -> Table - add row */
        if (strcmp(func, "add_row") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, table_type);
            TypeVec_push(arena, params, type_list(arena, type_string(arena)));
            return type_fn(arena, params, table_type);
        }
        /* Table.title(Table, String) -> Table - set title */
        if (strcmp(func, "title") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, table_type);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, table_type);
        }
        /* Table.border(Table, String) -> Table - set border style */
        if (strcmp(func, "border") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, table_type);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, table_type);
        }
        /* Table.show_header(Table, Int) -> Table - show/hide header row */
        if (strcmp(func, "show_header") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, table_type);
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, table_type);
        }
        /* Table.render(Table) -> String - render to string */
        if (strcmp(func, "render") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, table_type);
            return type_fn(arena, params, type_string(arena));
        }
    }

    /* ===== Tui.Progress module ===== */
    if (strcmp(module, "Tui.Progress") == 0) {
        Type* progress_type = type_con(arena, string_new(arena, "Progress"), NULL);
        /* Progress.new(Int) -> Progress - create progress bar with total */
        if (strcmp(func, "new") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, progress_type);
        }
        /* Progress.description(Progress, String) -> Progress */
        if (strcmp(func, "description") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, progress_type);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, progress_type);
        }
        /* Progress.width(Progress, Int) -> Progress */
        if (strcmp(func, "width") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, progress_type);
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, progress_type);
        }
        /* Progress.advance(Progress) -> Progress */
        if (strcmp(func, "advance") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, progress_type);
            return type_fn(arena, params, progress_type);
        }
        /* Progress.set(Progress, Int) -> Progress */
        if (strcmp(func, "set") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, progress_type);
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, progress_type);
        }
        /* Progress.render(Progress) -> String */
        if (strcmp(func, "render") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, progress_type);
            return type_fn(arena, params, type_string(arena));
        }
    }

    /* ===== Tui.Spinner module ===== */
    if (strcmp(module, "Tui.Spinner") == 0) {
        Type* spinner_type = type_con(arena, string_new(arena, "Spinner"), NULL);
        /* Spinner.new() -> Spinner */
        if (strcmp(func, "new") == 0) {
            params = TypeVec_new(arena);
            return type_fn(arena, params, spinner_type);
        }
        /* Spinner.message(Spinner, String) -> Spinner */
        if (strcmp(func, "message") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, spinner_type);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, spinner_type);
        }
        /* Spinner.style(Spinner, String) -> Spinner */
        if (strcmp(func, "style") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, spinner_type);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, spinner_type);
        }
        /* Spinner.tick(Spinner) -> Spinner */
        if (strcmp(func, "tick") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, spinner_type);
            return type_fn(arena, params, spinner_type);
        }
        /* Spinner.render(Spinner) -> String */
        if (strcmp(func, "render") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, spinner_type);
            return type_fn(arena, params, type_string(arena));
        }
    }

    /* ===== Tui.Prompt module ===== */
    if (strcmp(module, "Tui.Prompt") == 0) {
        /* Tui.Prompt.input(String) -> String - read line from user */
        if (strcmp(func, "input") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* Tui.Prompt.confirm(String) -> Bool - yes/no question */
        if (strcmp(func, "confirm") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_bool(arena));
        }
        /* Tui.Prompt.select(String, List(String)) -> Int - select from choices */
        if (strcmp(func, "select") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_list(arena, type_string(arena)));
            return type_fn(arena, params, type_int(arena));
        }
        /* Tui.Prompt.password(String) -> String - hidden input */
        if (strcmp(func, "password") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            return type_fn(arena, params, type_string(arena));
        }
        /* Tui.Prompt.int(String, Int, Int) -> Int - validated int input */
        if (strcmp(func, "int") == 0) {
            params = TypeVec_new(arena);
            TypeVec_push(arena, params, type_string(arena));
            TypeVec_push(arena, params, type_int(arena));
            TypeVec_push(arena, params, type_int(arena));
            return type_fn(arena, params, type_int(arena));
        }
    }

    /* Not found */
    return NULL;
}

/* ========== Built-in Function Registration ========== */

/**
 * Helper to register a built-in function with the given signature.
 * @param checker The type checker context.
 * @param name The function name.
 * @param fn_type The function type.
 */
static void register_builtin(Checker* checker, const char* name, Type* fn_type) {
    assert(checker != NULL);
    assert(name != NULL);
    assert(fn_type != NULL);
    type_env_define(checker->env, string_new(checker->arena, name), fn_type);
}

/**
 * Register built-in I/O functions.
 * @param checker The type checker context.
 */
static void register_io_builtins(Checker* checker) {
    assert(checker != NULL);
    assert(checker->arena != NULL);
    Arena* arena = checker->arena;

    /* print(a) -> () - polymorphic, accepts Int, String, Bool */
    TypeVec* print_params = TypeVec_new(arena);
    Type* print_var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
    TypeVec_push(arena, print_params, print_var);
    register_builtin(checker, "print", type_fn(arena, print_params, type_unit(arena)));

    /* println(a) -> () - polymorphic, accepts Int, String, Bool */
    TypeVec* println_params = TypeVec_new(arena);
    Type* println_var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
    TypeVec_push(arena, println_params, println_var);
    register_builtin(checker, "println", type_fn(arena, println_params, type_unit(arena)));
}

/**
 * Register built-in string functions.
 * @param checker The type checker context.
 */
static void register_string_builtins(Checker* checker) {
    // FERN_STYLE: allow(function-length) registering all string builtins in one place
    assert(checker != NULL);
    assert(checker->arena != NULL);
    Arena* arena = checker->arena;
    TypeVec* params;

    /* str_len(String) -> Int */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_len", type_fn(arena, params, type_int(arena)));

    /* str_concat(String, String) -> String */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_concat", type_fn(arena, params, type_string(arena)));

    /* str_eq(String, String) -> Bool */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_eq", type_fn(arena, params, type_bool(arena)));

    /* str_starts_with(String, String) -> Bool */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_starts_with", type_fn(arena, params, type_bool(arena)));

    /* str_ends_with(String, String) -> Bool */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_ends_with", type_fn(arena, params, type_bool(arena)));

    /* str_contains(String, String) -> Bool */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_contains", type_fn(arena, params, type_bool(arena)));

    /* str_slice(String, Int, Int) -> String */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    TypeVec_push(arena, params, type_int(arena));
    TypeVec_push(arena, params, type_int(arena));
    register_builtin(checker, "str_slice", type_fn(arena, params, type_string(arena)));

    /* str_trim(String) -> String */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_trim", type_fn(arena, params, type_string(arena)));

    /* str_trim_start(String) -> String */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_trim_start", type_fn(arena, params, type_string(arena)));

    /* str_trim_end(String) -> String */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_trim_end", type_fn(arena, params, type_string(arena)));

    /* str_to_upper(String) -> String */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_to_upper", type_fn(arena, params, type_string(arena)));

    /* str_to_lower(String) -> String */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_to_lower", type_fn(arena, params, type_string(arena)));

    /* str_replace(String, String, String) -> String */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    TypeVec_push(arena, params, type_string(arena));
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_replace", type_fn(arena, params, type_string(arena)));

    /* str_repeat(String, Int) -> String */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    TypeVec_push(arena, params, type_int(arena));
    register_builtin(checker, "str_repeat", type_fn(arena, params, type_string(arena)));

    /* str_is_empty(String) -> Bool */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_is_empty", type_fn(arena, params, type_bool(arena)));
    /* str_is_decimal(String) -> Bool */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "str_is_decimal", type_fn(arena, params, type_bool(arena)));
}

/**
 * Register built-in list functions.
 * @param checker The type checker context.
 */
static void register_list_builtins(Checker* checker) {
    // FERN_STYLE: allow(function-length) registering all list builtins in one place
    assert(checker != NULL);
    assert(checker->arena != NULL);
    Arena* arena = checker->arena;
    TypeVec* params;
    Type* var;
    TypeVec* type_args;
    Type* list_type;
    int var_id = 1;

    /* list_len(List(a)) -> Int */
    params = TypeVec_new(arena);
    var = type_var(arena, string_new(arena, "a"), var_id++);
    type_args = TypeVec_new(arena);
    TypeVec_push(arena, type_args, var);
    list_type = type_con(arena, string_new(arena, "List"), type_args);
    TypeVec_push(arena, params, list_type);
    register_builtin(checker, "list_len", type_fn(arena, params, type_int(arena)));

    /* list_get(List(a), Int) -> a */
    params = TypeVec_new(arena);
    var = type_var(arena, string_new(arena, "b"), var_id++);
    type_args = TypeVec_new(arena);
    TypeVec_push(arena, type_args, var);
    list_type = type_con(arena, string_new(arena, "List"), type_args);
    TypeVec_push(arena, params, list_type);
    TypeVec_push(arena, params, type_int(arena));
    register_builtin(checker, "list_get", type_fn(arena, params, var));

    /* list_push(List(a), a) -> List(a) */
    params = TypeVec_new(arena);
    var = type_var(arena, string_new(arena, "c"), var_id++);
    type_args = TypeVec_new(arena);
    TypeVec_push(arena, type_args, var);
    list_type = type_con(arena, string_new(arena, "List"), type_args);
    TypeVec_push(arena, params, list_type);
    TypeVec_push(arena, params, var);
    register_builtin(checker, "list_push", type_fn(arena, params, list_type));

    /* list_reverse(List(a)) -> List(a) */
    params = TypeVec_new(arena);
    var = type_var(arena, string_new(arena, "d"), var_id++);
    type_args = TypeVec_new(arena);
    TypeVec_push(arena, type_args, var);
    list_type = type_con(arena, string_new(arena, "List"), type_args);
    TypeVec_push(arena, params, list_type);
    register_builtin(checker, "list_reverse", type_fn(arena, params, list_type));

    /* list_concat(List(a), List(a)) -> List(a) */
    params = TypeVec_new(arena);
    var = type_var(arena, string_new(arena, "e"), var_id++);
    type_args = TypeVec_new(arena);
    TypeVec_push(arena, type_args, var);
    list_type = type_con(arena, string_new(arena, "List"), type_args);
    TypeVec_push(arena, params, list_type);
    TypeVec_push(arena, params, list_type);
    register_builtin(checker, "list_concat", type_fn(arena, params, list_type));

    /* list_head(List(a)) -> a */
    params = TypeVec_new(arena);
    var = type_var(arena, string_new(arena, "f"), var_id++);
    type_args = TypeVec_new(arena);
    TypeVec_push(arena, type_args, var);
    list_type = type_con(arena, string_new(arena, "List"), type_args);
    TypeVec_push(arena, params, list_type);
    register_builtin(checker, "list_head", type_fn(arena, params, var));

    /* list_tail(List(a)) -> List(a) */
    params = TypeVec_new(arena);
    var = type_var(arena, string_new(arena, "g"), var_id++);
    type_args = TypeVec_new(arena);
    TypeVec_push(arena, type_args, var);
    list_type = type_con(arena, string_new(arena, "List"), type_args);
    TypeVec_push(arena, params, list_type);
    register_builtin(checker, "list_tail", type_fn(arena, params, list_type));

    /* list_is_empty(List(a)) -> Bool */
    params = TypeVec_new(arena);
    var = type_var(arena, string_new(arena, "h"), var_id++);
    type_args = TypeVec_new(arena);
    TypeVec_push(arena, type_args, var);
    list_type = type_con(arena, string_new(arena, "List"), type_args);
    TypeVec_push(arena, params, list_type);
    register_builtin(checker, "list_is_empty", type_fn(arena, params, type_bool(arena)));
}

/**
 * Register built-in file I/O functions.
 * @param checker The type checker context.
 */
static void register_file_builtins(Checker* checker) {
    assert(checker != NULL);
    assert(checker->arena != NULL);
    Arena* arena = checker->arena;
    TypeVec* params;
    TypeVec* result_args;
    Type* result_type;

    /* read_file(String) -> Result(String, Int) */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    result_args = TypeVec_new(arena);
    TypeVec_push(arena, result_args, type_string(arena));
    TypeVec_push(arena, result_args, type_int(arena));
    result_type = type_con(arena, string_new(arena, "Result"), result_args);
    register_builtin(checker, "read_file", type_fn(arena, params, result_type));

    /* write_file(String, String) -> Result(Int, Int) */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    TypeVec_push(arena, params, type_string(arena));
    result_args = TypeVec_new(arena);
    TypeVec_push(arena, result_args, type_int(arena));
    TypeVec_push(arena, result_args, type_int(arena));
    result_type = type_con(arena, string_new(arena, "Result"), result_args);
    register_builtin(checker, "write_file", type_fn(arena, params, result_type));

    /* append_file(String, String) -> Result(Int, Int) */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    TypeVec_push(arena, params, type_string(arena));
    result_args = TypeVec_new(arena);
    TypeVec_push(arena, result_args, type_int(arena));
    TypeVec_push(arena, result_args, type_int(arena));
    result_type = type_con(arena, string_new(arena, "Result"), result_args);
    register_builtin(checker, "append_file", type_fn(arena, params, result_type));

    /* file_exists(String) -> Bool */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    register_builtin(checker, "file_exists", type_fn(arena, params, type_bool(arena)));

    /* delete_file(String) -> Result(Int, Int) */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    result_args = TypeVec_new(arena);
    TypeVec_push(arena, result_args, type_int(arena));
    TypeVec_push(arena, result_args, type_int(arena));
    result_type = type_con(arena, string_new(arena, "Result"), result_args);
    register_builtin(checker, "delete_file", type_fn(arena, params, result_type));

    /* file_size(String) -> Result(Int, Int) */
    params = TypeVec_new(arena);
    TypeVec_push(arena, params, type_string(arena));
    result_args = TypeVec_new(arena);
    TypeVec_push(arena, result_args, type_int(arena));
    TypeVec_push(arena, result_args, type_int(arena));
    result_type = type_con(arena, string_new(arena, "Result"), result_args);
    register_builtin(checker, "file_size", type_fn(arena, params, result_type));
}

/**
 * Register Result type constructors Ok and Err.
 * @param checker The type checker context.
 */
static void register_result_constructors(Checker* checker) {
    assert(checker != NULL);
    assert(checker->arena != NULL);
    Arena* arena = checker->arena;
    
    /* Ok(a) -> Result(a, e) */
    TypeVec* params = TypeVec_new(arena);
    Type* ok_var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
    Type* err_var = type_var(arena, string_new(arena, "e"), type_fresh_var_id());
    TypeVec_push(arena, params, ok_var);
    TypeVec* result_args = TypeVec_new(arena);
    TypeVec_push(arena, result_args, ok_var);
    TypeVec_push(arena, result_args, err_var);
    Type* result_type = type_con(arena, string_new(arena, "Result"), result_args);
    register_builtin(checker, "Ok", type_fn(arena, params, result_type));
    
    /* Err(e) -> Result(a, e) */
    params = TypeVec_new(arena);
    ok_var = type_var(arena, string_new(arena, "a"), type_fresh_var_id());
    err_var = type_var(arena, string_new(arena, "e"), type_fresh_var_id());
    TypeVec_push(arena, params, err_var);
    result_args = TypeVec_new(arena);
    TypeVec_push(arena, result_args, ok_var);
    TypeVec_push(arena, result_args, err_var);
    result_type = type_con(arena, string_new(arena, "Result"), result_args);
    register_builtin(checker, "Err", type_fn(arena, params, result_type));
}

/* ========== Checker Creation ========== */

/**
 * Create a new type checker.
 * @param arena The arena for allocations.
 * @return The new type checker.
 */
Checker* checker_new(Arena* arena) {
    assert(arena != NULL);
    Checker* checker = arena_alloc(arena, sizeof(Checker));
    assert(checker != NULL);
    checker->arena = arena;
    checker->env = type_env_new(arena);
    checker->errors = NULL;
    checker->errors_tail = NULL;
    checker->current_return_type = NULL;
    checker->current_expr_loc = (SourceLoc){0};
    checker->require_executable = false;

    /* Register built-in functions from the runtime library */
    register_io_builtins(checker);
    register_string_builtins(checker);
    register_list_builtins(checker);
    register_file_builtins(checker);
    register_result_constructors(checker);

    return checker;
}

/**
 * Restrict a checker to language forms with implemented runtime execution.
 * @param checker Checker instance whose environment has been initialized.
 */
void checker_require_executable(Checker* checker) {
    assert(checker != NULL);
    assert(checker->env != NULL);
    checker->require_executable = true;
}

/* ========== Binary Operator Type Checking ========== */

/**
 * Check types for arithmetic operators (+, -, *, /, %, **).
 * @param checker The type checker context.
 * @param left The left operand type.
 * @param right The right operand type.
 * @param op_name The operator name for error messages.
 * @return The result type.
 */
static Type* check_arithmetic_op(Checker* checker, Type* left, Type* right, 
                                  const char* op_name) {
    /* Handle type variables through unification */
    /* Follow bound type variables */
    while (left->kind == TYPE_VAR && left->data.var.bound) {
        left = left->data.var.bound;
    }
    while (right->kind == TYPE_VAR && right->data.var.bound) {
        right = right->data.var.bound;
    }
    
    /* If one side is a type variable, try to unify with the other */
    if (left->kind == TYPE_VAR && !left->data.var.bound) {
        if (right->kind == TYPE_INT || right->kind == TYPE_FLOAT) {
            left->data.var.bound = right;
            return right;
        }
    }
    if (right->kind == TYPE_VAR && !right->data.var.bound) {
        if (left->kind == TYPE_INT || left->kind == TYPE_FLOAT) {
            right->data.var.bound = left;
            return left;
        }
    }
    
    /* Both operands must be the same numeric type */
    if (left->kind == TYPE_INT && right->kind == TYPE_INT) {
        return type_int(checker->arena);
    }
    if (left->kind == TYPE_FLOAT && right->kind == TYPE_FLOAT) {
        return type_float(checker->arena);
    }
    
    /* String concatenation with + */
    if (strcmp(op_name, "+") == 0 && 
        left->kind == TYPE_STRING && right->kind == TYPE_STRING) {
        return type_string(checker->arena);
    }
    
    return error_type(checker, "Cannot apply '%s' to %s and %s",
        op_name,
        string_cstr(type_to_string(checker->arena, left)),
        string_cstr(type_to_string(checker->arena, right)));
}

/**
 * Check types for comparison operators (<, <=, >, >=).
 * @param checker The type checker context.
 * @param left The left operand type.
 * @param right The right operand type.
 * @param op_name The operator name for error messages.
 * @return The result type (Bool).
 */
static Type* check_comparison_op(Checker* checker, Type* left, Type* right,
                                  const char* op_name) {
    /* Both operands must be the same comparable type */
    if (!type_equals(left, right)) {
        return error_type(checker, "Cannot compare %s with %s using '%s'",
            string_cstr(type_to_string(checker->arena, left)),
            string_cstr(type_to_string(checker->arena, right)),
            op_name);
    }
    
    if (!type_is_comparable(left)) {
        return error_type(checker, "Type %s is not comparable",
            string_cstr(type_to_string(checker->arena, left)));
    }
    
    return type_bool(checker->arena);
}

/**
 * Check types for equality operators (==, !=).
 * @param checker The type checker context.
 * @param left The left operand type.
 * @param right The right operand type.
 * @param op_name The operator name for error messages.
 * @return The result type (Bool).
 */
static Type* check_equality_op(Checker* checker, Type* left, Type* right,
                                const char* op_name) {
    (void)op_name;  /* Used for future error messages */
    
    /* Both operands must be the same type */
    if (!type_equals(left, right)) {
        return error_type(checker, "Cannot compare %s with %s for equality",
            string_cstr(type_to_string(checker->arena, left)),
            string_cstr(type_to_string(checker->arena, right)));
    }
    
    return type_bool(checker->arena);
}

/**
 * Check types for 'in' operator (list containment).
 * @param checker The type checker context.
 * @param left The element type (left operand).
 * @param right The list type (right operand).
 * @return Bool type on success, error type on failure.
 */
static Type* check_in_op(Checker* checker, Type* left, Type* right) {
    assert(checker != NULL);
    assert(left != NULL);
    assert(right != NULL);

    /* Right must be List(a) */
    if (right->kind != TYPE_CON ||
        strcmp(string_cstr(right->data.con.name), "List") != 0) {
        return error_type(checker, "'in' requires a List on the right, got %s",
            string_cstr(type_to_string(checker->arena, right)));
    }
    /* Element type must match */
    if (right->data.con.args->len > 0) {
        Type* elem_type = right->data.con.args->data[0];
        if (!unify(left, elem_type)) {
            return error_type(checker, "Cannot check if %s is in List(%s)",
                string_cstr(type_to_string(checker->arena, left)),
                string_cstr(type_to_string(checker->arena, elem_type)));
        }
    }
    return type_bool(checker->arena);
}

/**
 * Check types for logical operators (and, or).
 * @param checker The type checker context.
 * @param left The left operand type.
 * @param right The right operand type.
 * @param op_name The operator name for error messages.
 * @return The result type (Bool).
 */
static Type* check_logical_op(Checker* checker, Type* left, Type* right,
                               const char* op_name) {
    /* Both operands must be Bool */
    if (left->kind != TYPE_BOOL) {
        return error_type(checker, "Left operand of '%s' must be Bool, got %s",
            op_name, string_cstr(type_to_string(checker->arena, left)));
    }
    if (right->kind != TYPE_BOOL) {
        return error_type(checker, "Right operand of '%s' must be Bool, got %s",
            op_name, string_cstr(type_to_string(checker->arena, right)));
    }
    
    return type_bool(checker->arena);
}

/* ========== Pipe Operator Type Checking ========== */

/**
 * Check types for pipe operator (|>).
 * @param checker The type checker context.
 * @param expr The binary expression with pipe operator.
 * @return The result type.
 */
static Type* check_pipe_expr(Checker* checker, BinaryExpr* expr) {
    // FERN_STYLE: allow(function-length) pipe checking requires handling multiple cases cohesively
    assert(checker != NULL);
    assert(expr != NULL);
    /* Pipe: left |> right
     * 
     * The pipe operator passes the left value as the first argument to
     * the function call on the right. For example:
     *   x |> f(a, b)  is equivalent to  f(x, a, b)
     *   x |> f()      is equivalent to  f(x)
     *
     * The right side must be a call expression. We type-check it specially
     * by injecting the left value as an additional first argument.
     */
    
    /* Type check the left side first */
    Type* left_type = checker_infer_expr(checker, expr->left);
    if (left_type->kind == TYPE_ERROR) return left_type;
    
    /* Right side must be a call expression */
    Expr* right = expr->right;
    if (right->type != EXPR_CALL) {
        return error_type(checker, "Pipe target must be a function call");
    }
    
    CallExpr* call = &right->data.call;
    
    /* Get the function being called */
    Type* callee_type = checker_infer_expr(checker, call->func);
    if (callee_type->kind == TYPE_ERROR) return callee_type;
    
    /* Must be a function type */
    if (callee_type->kind != TYPE_FN) {
        return error_type(checker, "Cannot call non-function type %s",
            string_cstr(type_to_string(checker->arena, callee_type)));
    }
    
    /* Instantiate the function type with fresh type variables */
    VarMapping* var_map = NULL;
    Type* fn_type_inst = instantiate_type(checker->arena, callee_type, &var_map);
    TypeFn* fn = &fn_type_inst->data.fn;
    
    /* The total number of arguments is: 1 (piped) + explicit args */
    size_t total_args = 1 + call->args->len;
    size_t expected_count = fn->params->len;
    
    if (total_args != expected_count) {
        return error_type(checker, "Expected %zu arguments, got %zu (including piped value)",
            expected_count, total_args);
    }
    
    /* Unify the piped value (left) with the first parameter */
    Type* first_param = fn->params->data[0];
    if (!unify(first_param, left_type)) {
        return error_type(checker, "Pipe: cannot pass %s to function expecting %s",
            string_cstr(type_to_string(checker->arena, left_type)),
            string_cstr(type_to_string(checker->arena, first_param)));
    }
    
    /* Unify the explicit arguments with the remaining parameters */
    for (size_t i = 0; i < call->args->len; i++) {
        Type* expected = fn->params->data[i + 1]; /* +1 because first param is piped */
        Type* actual = checker_infer_expr(checker, call->args->data[i].value);
        
        if (actual->kind == TYPE_ERROR) return actual;
        
        if (!unify(expected, actual)) {
            return error_type(checker, "Argument %zu: expected %s, got %s",
                i + 2, /* +2 because arg 1 is piped, user sees 2-based */
                string_cstr(type_to_string(checker->arena, expected)),
                string_cstr(type_to_string(checker->arena, actual)));
        }
    }
    
    /* Return the result type with substitutions applied */
    return substitute(checker->arena, fn->result);
}

/**
 * Check types for binary expressions.
 * @param checker The type checker context.
 * @param expr The binary expression to check.
 * @return The result type.
 */
static Type* check_binary_expr(Checker* checker, BinaryExpr* expr) {
    assert(checker != NULL);
    assert(expr != NULL);
    /* Pipe operator needs special handling - don't type check right side yet */
    if (expr->op == BINOP_PIPE) {
        return check_pipe_expr(checker, expr);
    }
    
    Type* left = checker_infer_expr(checker, expr->left);
    Type* right = checker_infer_expr(checker, expr->right);
    
    /* Propagate errors */
    if (left->kind == TYPE_ERROR) return left;
    if (right->kind == TYPE_ERROR) return right;
    
    switch (expr->op) {
        case BINOP_ADD:
            return check_arithmetic_op(checker, left, right, "+");
        case BINOP_SUB:
            return check_arithmetic_op(checker, left, right, "-");
        case BINOP_MUL:
            return check_arithmetic_op(checker, left, right, "*");
        case BINOP_DIV:
            return check_arithmetic_op(checker, left, right, "/");
        case BINOP_MOD:
            return check_arithmetic_op(checker, left, right, "%");
        case BINOP_POW:
            return check_arithmetic_op(checker, left, right, "**");
            
        case BINOP_LT:
            return check_comparison_op(checker, left, right, "<");
        case BINOP_LE:
            return check_comparison_op(checker, left, right, "<=");
        case BINOP_GT:
            return check_comparison_op(checker, left, right, ">");
        case BINOP_GE:
            return check_comparison_op(checker, left, right, ">=");
            
        case BINOP_EQ:
            return check_equality_op(checker, left, right, "==");
        case BINOP_NE:
            return check_equality_op(checker, left, right, "!=");
            
        case BINOP_AND:
            return check_logical_op(checker, left, right, "and");
        case BINOP_OR:
            return check_logical_op(checker, left, right, "or");
            
        case BINOP_PIPE:
            /* Handled above by check_pipe_expr */
            return right; /* Should not reach here */
            
        case BINOP_IN:
            return check_in_op(checker, left, right);
    }
    
    return error_type(checker, "Unknown binary operator");
}

/* ========== Unary Operator Type Checking ========== */

/**
 * Check types for unary expressions (-, not).
 * @param checker The type checker context.
 * @param expr The unary expression to check.
 * @return The result type.
 */
static Type* check_unary_expr(Checker* checker, UnaryExpr* expr) {
    assert(checker != NULL);
    assert(expr != NULL);
    Type* operand = checker_infer_expr(checker, expr->operand);
    
    if (operand->kind == TYPE_ERROR) return operand;
    
    switch (expr->op) {
        case UNOP_NEG:
            if (operand->kind == TYPE_INT) {
                return type_int(checker->arena);
            }
            if (operand->kind == TYPE_FLOAT) {
                return type_float(checker->arena);
            }
            return error_type(checker, "Cannot negate %s, expected numeric type",
                string_cstr(type_to_string(checker->arena, operand)));
            
        case UNOP_NOT:
            if (operand->kind == TYPE_BOOL) {
                return type_bool(checker->arena);
            }
            return error_type(checker, "Cannot apply 'not' to %s, expected Bool",
                string_cstr(type_to_string(checker->arena, operand)));
    }
    
    return error_type(checker, "Unknown unary operator");
}

/* ========== Collection Type Checking ========== */

/**
 * Check types for list expressions.
 * @param checker The type checker context.
 * @param expr The list expression to check.
 * @return The list type.
 */
static Type* check_list_expr(Checker* checker, ListExpr* expr) {
    assert(checker != NULL);
    assert(expr != NULL);
    if (expr->elements->len == 0) {
        /* Empty list has type List(a) where a is a fresh type variable */
        int var_id = type_fresh_var_id();
        Type* elem_type = type_var(checker->arena, 
            string_new(checker->arena, "a"), var_id);
        return type_list(checker->arena, elem_type);
    }
    
    /* Infer type of first element */
    Type* elem_type = checker_infer_expr(checker, expr->elements->data[0]);
    if (elem_type->kind == TYPE_ERROR) return elem_type;
    
    /* Check all elements have the same type */
    for (size_t i = 1; i < expr->elements->len; i++) {
        Type* t = checker_infer_expr(checker, expr->elements->data[i]);
        if (t->kind == TYPE_ERROR) return t;
        
        if (!type_equals(elem_type, t)) {
            return error_type(checker, 
                "List element type mismatch: expected %s, got %s at index %zu",
                string_cstr(type_to_string(checker->arena, elem_type)),
                string_cstr(type_to_string(checker->arena, t)),
                i);
        }
    }
    
    return type_list(checker->arena, elem_type);
}

/**
 * Check types for tuple expressions.
 * @param checker The type checker context.
 * @param expr The tuple expression to check.
 * @return The tuple type.
 */
static Type* check_tuple_expr(Checker* checker, TupleExpr* expr) {
    assert(checker != NULL);
    assert(expr != NULL);
    
    /* Empty tuple () is Unit */
    if (expr->elements->len == 0) {
        return type_unit(checker->arena);
    }
    
    TypeVec* elem_types = TypeVec_new(checker->arena);
    
    for (size_t i = 0; i < expr->elements->len; i++) {
        Type* t = checker_infer_expr(checker, expr->elements->data[i]);
        if (t->kind == TYPE_ERROR) return t;
        TypeVec_push(checker->arena, elem_types, t);
    }
    
    return type_tuple(checker->arena, elem_types);
}

/* ========== Type Unification ========== */

/**
 * Check if a type contains a specific type variable (occurs check).
 * @param type The type to check.
 * @param var_id The variable ID to look for.
 * @return True if the variable is contained.
 */
static bool type_contains_var(Type* type, int var_id) {
    assert(var_id >= 0);
    if (!type) return false;
    assert(type->kind >= TYPE_INT && type->kind <= TYPE_ERROR);
    
    switch (type->kind) {
        case TYPE_VAR:
            if (type->data.var.bound) {
                return type_contains_var(type->data.var.bound, var_id);
            }
            return type->data.var.id == var_id;
            
        case TYPE_CON:
            if (type->data.con.args) {
                for (size_t i = 0; i < type->data.con.args->len; i++) {
                    if (type_contains_var(type->data.con.args->data[i], var_id)) {
                        return true;
                    }
                }
            }
            return false;
            
        case TYPE_FN:
            for (size_t i = 0; i < type->data.fn.params->len; i++) {
                if (type_contains_var(type->data.fn.params->data[i], var_id)) {
                    return true;
                }
            }
            return type_contains_var(type->data.fn.result, var_id);
            
        case TYPE_TUPLE:
            for (size_t i = 0; i < type->data.tuple.elements->len; i++) {
                if (type_contains_var(type->data.tuple.elements->data[i], var_id)) {
                    return true;
                }
            }
            return false;
            
        default:
            return false;
    }
}

/**
 * Unify two types, binding type variables as needed.
 * @param a The first type.
 * @param b The second type.
 * @return True if unification succeeded.
 */
static bool unify(Type* a, Type* b) {
    // FERN_STYLE: allow(function-length) type unification requires handling all type combinations
    /* NULL types are equal only to each other. */
    if (!a || !b) return a == b;
    assert(a->kind >= TYPE_INT && a->kind <= TYPE_ERROR);
    assert(b->kind >= TYPE_INT && b->kind <= TYPE_ERROR);
    
    /* Follow bound type variables */
    while (a->kind == TYPE_VAR && a->data.var.bound) {
        a = a->data.var.bound;
    }
    while (b->kind == TYPE_VAR && b->data.var.bound) {
        b = b->data.var.bound;
    }
    
    /* Same type variable */
    if (a->kind == TYPE_VAR && b->kind == TYPE_VAR && 
        a->data.var.id == b->data.var.id) {
        return true;
    }
    
    /* Bind unbound type variable to concrete type */
    if (a->kind == TYPE_VAR && !a->data.var.bound) {
        /* Occurs check: can't unify a with something containing a */
        if (type_contains_var(b, a->data.var.id)) {
            return false;
        }
        a->data.var.bound = b;
        return true;
    }
    
    if (b->kind == TYPE_VAR && !b->data.var.bound) {
        if (type_contains_var(a, b->data.var.id)) {
            return false;
        }
        b->data.var.bound = a;
        return true;
    }
    
    /* Different kinds */
    if (a->kind != b->kind) {
        return false;
    }
    
    /* Same kind - compare structurally */
    switch (a->kind) {
        case TYPE_INT:
        case TYPE_FLOAT:
        case TYPE_STRING:
        case TYPE_BOOL:
        case TYPE_UNIT:
        case TYPE_ERROR:
            return true;
            
        case TYPE_CON:
            if (!string_equal(a->data.con.name, b->data.con.name)) {
                return false;
            }
            if (!a->data.con.args && !b->data.con.args) return true;
            if (!a->data.con.args || !b->data.con.args) return false;
            if (a->data.con.args->len != b->data.con.args->len) return false;
            for (size_t i = 0; i < a->data.con.args->len; i++) {
                if (!unify(a->data.con.args->data[i], b->data.con.args->data[i])) {
                    return false;
                }
            }
            return true;
            
        case TYPE_FN:
            if (a->data.fn.params->len != b->data.fn.params->len) return false;
            for (size_t i = 0; i < a->data.fn.params->len; i++) {
                if (!unify(a->data.fn.params->data[i], b->data.fn.params->data[i])) {
                    return false;
                }
            }
            return unify(a->data.fn.result, b->data.fn.result);
            
        case TYPE_TUPLE:
            if (a->data.tuple.elements->len != b->data.tuple.elements->len) return false;
            for (size_t i = 0; i < a->data.tuple.elements->len; i++) {
                if (!unify(a->data.tuple.elements->data[i], b->data.tuple.elements->data[i])) {
                    return false;
                }
            }
            return true;
            
        case TYPE_VAR:
            /* Already handled above */
            return false;
    }
    
    return false;
}

/**
 * Substitute bound type variables with their concrete types.
 * @param arena The arena for allocations.
 * @param type The type to substitute in.
 * @return The substituted type.
 */
static Type* substitute(Arena* arena, Type* type) {
    assert(arena != NULL);
    if (!type) return NULL;
    assert(type->kind >= TYPE_INT && type->kind <= TYPE_ERROR);
    
    switch (type->kind) {
        case TYPE_VAR:
            if (type->data.var.bound) {
                return substitute(arena, type->data.var.bound);
            }
            return type;
            
        case TYPE_CON: {
            if (!type->data.con.args || type->data.con.args->len == 0) {
                return type;
            }
            TypeVec* new_args = TypeVec_new(arena);
            for (size_t i = 0; i < type->data.con.args->len; i++) {
                TypeVec_push(arena, new_args, 
                    substitute(arena, type->data.con.args->data[i]));
            }
            return type_con(arena, type->data.con.name, new_args);
        }
            
        case TYPE_FN: {
            TypeVec* new_params = TypeVec_new(arena);
            for (size_t i = 0; i < type->data.fn.params->len; i++) {
                TypeVec_push(arena, new_params,
                    substitute(arena, type->data.fn.params->data[i]));
            }
            return type_fn(arena, new_params, 
                substitute(arena, type->data.fn.result));
        }
            
        case TYPE_TUPLE: {
            TypeVec* new_elements = TypeVec_new(arena);
            for (size_t i = 0; i < type->data.tuple.elements->len; i++) {
                TypeVec_push(arena, new_elements,
                    substitute(arena, type->data.tuple.elements->data[i]));
            }
            return type_tuple(arena, new_elements);
        }
            
        default:
            return type;
    }
}

/* ========== Type Instantiation ========== */

/**
 * Find a mapping for a type variable ID.
 * @param map The variable mapping list.
 * @param var_id The variable ID to find.
 * @return The mapped type or NULL if not found.
 */
static Type* find_var_mapping(VarMapping* map, int var_id) {
    assert(var_id >= 0);
    /* map may be NULL on first call - that's valid. */
    for (VarMapping* m = map; m != NULL; m = m->next) {
        assert(m->new_var != NULL);
        if (m->old_id == var_id) {
            return m->new_var;
        }
    }
    return NULL;
}

/**
 * Instantiate a type by replacing type variables with fresh ones.
 * @param arena The arena for allocations.
 * @param type The type to instantiate.
 * @param map The variable mapping (updated in place).
 * @return The instantiated type.
 */
static Type* instantiate_type(Arena* arena, Type* type, VarMapping** map) {
    assert(arena != NULL);
    assert(map != NULL);
    if (!type) return NULL;
    
    switch (type->kind) {
        case TYPE_VAR: {
            /* Check if we already have a mapping for this variable */
            Type* existing = find_var_mapping(*map, type->data.var.id);
            if (existing) {
                return existing;
            }
            
            /* Create a fresh type variable with a new ID */
            int new_id = type_fresh_var_id();
            Type* fresh = type_var(arena, type->data.var.name, new_id);
            
            /* Add to mapping */
            VarMapping* new_map = arena_alloc(arena, sizeof(VarMapping));
            new_map->old_id = type->data.var.id;
            new_map->new_var = fresh;
            new_map->next = *map;
            *map = new_map;
            
            return fresh;
        }
            
        case TYPE_CON: {
            if (!type->data.con.args || type->data.con.args->len == 0) {
                return type;
            }
            TypeVec* new_args = TypeVec_new(arena);
            for (size_t i = 0; i < type->data.con.args->len; i++) {
                TypeVec_push(arena, new_args, 
                    instantiate_type(arena, type->data.con.args->data[i], map));
            }
            return type_con(arena, type->data.con.name, new_args);
        }
            
        case TYPE_FN: {
            TypeVec* new_params = TypeVec_new(arena);
            for (size_t i = 0; i < type->data.fn.params->len; i++) {
                TypeVec_push(arena, new_params,
                    instantiate_type(arena, type->data.fn.params->data[i], map));
            }
            return type_fn(arena, new_params, 
                instantiate_type(arena, type->data.fn.result, map));
        }
            
        case TYPE_TUPLE: {
            TypeVec* new_elements = TypeVec_new(arena);
            for (size_t i = 0; i < type->data.tuple.elements->len; i++) {
                TypeVec_push(arena, new_elements,
                    instantiate_type(arena, type->data.tuple.elements->data[i], map));
            }
            return type_tuple(arena, new_elements);
        }
            
        default:
            return type;
    }
}

/* ========== Lambda Bidirectional Checking ========== */

/**
 * Check a lambda expression against an expected function type.
 * This enables bidirectional type checking - when we know what type
 * a lambda should have, we use that to constrain its parameter types.
 * 
 * @param checker The type checker context.
 * @param lambda The lambda expression to check.
 * @param expected_fn_type The expected function type (must be TYPE_FN).
 * @return The lambda's function type, or error type on failure.
 */
static Type* check_lambda_against(Checker* checker, LambdaExpr* lambda, Type* expected_fn_type) {
    assert(checker != NULL);
    assert(lambda != NULL);
    assert(expected_fn_type != NULL);
    assert(expected_fn_type->kind == TYPE_FN);
    
    TypeFn* expected_fn = &expected_fn_type->data.fn;
    
    /* Check parameter count matches */
    if (lambda->params->len != expected_fn->params->len) {
        return error_type(checker, "Lambda has %zu parameters but expected %zu",
            lambda->params->len, expected_fn->params->len);
    }
    
    /* Push a new scope for lambda parameters */
    checker_push_scope(checker);
    
    /* Bind each parameter to its expected type (from the function type) */
    TypeVec* param_types = TypeVec_new(checker->arena);
    for (size_t i = 0; i < lambda->params->len; i++) {
        Type* param_type = expected_fn->params->data[i];
        /* Substitute any bound type variables */
        param_type = substitute(checker->arena, param_type);
        TypeVec_push(checker->arena, param_types, param_type);
        checker_define(checker, lambda->params->data[i], param_type);
    }
    
    /* Infer the body type with constrained parameters in scope */
    Type* body_type = checker_infer_expr(checker, lambda->body);
    
    checker_pop_scope(checker);
    
    if (body_type->kind == TYPE_ERROR) return body_type;
    
    /* Unify body type with expected return type */
    Type* expected_ret = substitute(checker->arena, expected_fn->result);
    if (!unify(expected_ret, body_type)) {
        return error_type(checker, "Lambda body has type %s but expected %s",
            string_cstr(type_to_string(checker->arena, body_type)),
            string_cstr(type_to_string(checker->arena, expected_ret)));
    }
    
    /* Return the function type */
    return type_fn(checker->arena, param_types, body_type);
}

/* ========== Function Call Type Checking ========== */

/**
 * Check types for function call expressions.
 * @param checker The type checker context.
 * @param call_expr The call expression to check.
 * @return The return type of the call.
 */
static Type* check_call_expr(Checker* checker, Expr* call_expr) {
    assert(checker != NULL);
    assert(call_expr != NULL);
    assert(call_expr->type == EXPR_CALL);
    
    CallExpr* expr = &call_expr->data.call;
    SourceLoc loc = call_expr->loc;

    if (expr->func->type == EXPR_IDENT &&
        strcmp(string_cstr(expr->func->data.ident.name), "spawn_link") == 0) {
        return check_spawn_link_call_expr(checker, expr, loc);
    }
    
    /* Infer the type of the callee */
    Type* callee_type = checker_infer_expr(checker, expr->func);
    if (callee_type->kind == TYPE_ERROR) return callee_type;
    
    /* Callee must be a function type */
    if (callee_type->kind != TYPE_FN) {
        return error_type_at(checker, loc, "Cannot call non-function type %s",
            string_cstr(type_to_string(checker->arena, callee_type)));
    }
    
    /* Instantiate the function type with fresh type variables.
     * This ensures that multiple calls to the same generic function
     * don't interfere with each other, and that type variables that
     * appear multiple times (like in (a) -> a) share the same binding. */
    VarMapping* var_map = NULL;
    Type* fn_type_inst = instantiate_type(checker->arena, callee_type, &var_map);
    TypeFn* fn = &fn_type_inst->data.fn;
    
    /* Check argument count */
    size_t expected_count = fn->params->len;
    size_t actual_count = expr->args->len;
    
    if (actual_count != expected_count) {
        return error_type_at(checker, loc, "Expected %zu arguments, got %zu",
            expected_count, actual_count);
    }
    
    /* Unify argument types with parameter types (handles generics) */
    for (size_t i = 0; i < actual_count; i++) {
        Type* expected = fn->params->data[i];
        Expr* arg_expr = expr->args->data[i].value;
        Type* actual;
        
        /* Bidirectional type checking for lambdas:
         * If the expected type is a function and the argument is a lambda,
         * check the lambda against the expected type instead of inferring.
         * This allows lambdas like (x) -> x > 3 to work when passed to
         * functions expecting (Int) -> Bool predicates. */
        Type* expected_subst = substitute(checker->arena, expected);
        if (expected_subst->kind == TYPE_FN && arg_expr->type == EXPR_LAMBDA) {
            actual = check_lambda_against(checker, &arg_expr->data.lambda, expected_subst);
        } else {
            actual = checker_infer_expr(checker, arg_expr);
        }
        
        if (actual->kind == TYPE_ERROR) return actual;
        
        if (!unify(expected, actual)) {
            return error_type_at(checker, loc, "Argument %zu: expected %s, got %s",
                i + 1,
                string_cstr(type_to_string(checker->arena, expected)),
                string_cstr(type_to_string(checker->arena, actual)));
        }
    }
    
    /* Substitute bound type variables in the result type */
    return substitute(checker->arena, fn->result);
}

/* ========== If Expression Type Checking ========== */

/**
 * Check types for if expressions.
 * @param checker The type checker context.
 * @param expr The if expression to check.
 * @return The result type.
 */
static Type* check_if_expr(Checker* checker, IfExpr* expr) {
    assert(checker != NULL);
    assert(expr != NULL);
    /* Check condition is Bool */
    Type* cond_type = checker_infer_expr(checker, expr->condition);
    if (cond_type->kind == TYPE_ERROR) return cond_type;
    
    if (cond_type->kind != TYPE_BOOL) {
        return error_type(checker, "If condition must be Bool, got %s",
            string_cstr(type_to_string(checker->arena, cond_type)));
    }
    
    /* Check then branch */
    Type* then_type = checker_infer_expr(checker, expr->then_branch);
    if (then_type->kind == TYPE_ERROR) return substitute(checker->arena, then_type);
    
    /* If no else branch, the whole expression returns Unit */
    if (expr->else_branch == NULL) {
        return type_unit(checker->arena);
    }
    
    /* Check else branch */
    Type* else_type = checker_infer_expr(checker, expr->else_branch);
    if (else_type->kind == TYPE_ERROR) return else_type;
    
    /* Both branches must have the same type */
    if (!unify(then_type, else_type)) {
        return error_type(checker, "If branches have different types: %s vs %s",
            string_cstr(type_to_string(checker->arena, then_type)),
            string_cstr(type_to_string(checker->arena, else_type)));
    }
    
    return substitute(checker->arena, then_type);
}

/* ========== Block Expression Type Checking ========== */

/**
 * Check types for block expressions.
 * @param checker The type checker context.
 * @param expr The block expression to check.
 * @return The result type.
 */
static Type* check_block_expr(Checker* checker, BlockExpr* expr) {
    assert(checker != NULL);
    assert(expr != NULL);
    /* Push a new scope for the block */
    checker_push_scope(checker);
    
    /* Type check all statements in the block */
    for (size_t i = 0; i < expr->stmts->len; i++) {
        if (!checker_check_stmt(checker, expr->stmts->data[i])) {
            checker_pop_scope(checker);
            return type_error(checker->arena, 
                string_new(checker->arena, "Statement type check failed"));
        }
    }
    
    /* The block's type is the type of the final expression, or Unit if none */
    Type* result;
    if (expr->final_expr != NULL) {
        result = checker_infer_expr(checker, expr->final_expr);
    } else {
        result = type_unit(checker->arena);
    }
    
    checker_pop_scope(checker);
    return result;
}

/* ========== Match Expression Type Checking ========== */

/**
 * Check types for match expressions.
 * @param checker The type checker context.
 * @param expr The match expression to check.
 * @return The result type.
 */
static Type* check_match_expr(Checker* checker, MatchExpr* expr) {
    // FERN_STYLE: allow(function-length) match expression checking requires handling all pattern types
    assert(checker != NULL);
    assert(expr != NULL);
    /* Infer type of the scrutinee */
    Type* scrutinee_type = checker_infer_expr(checker, expr->value);
    if (scrutinee_type->kind == TYPE_ERROR) return scrutinee_type;
    
    if (expr->arms->len == 0) {
        return error_type(checker, "Match expression must have at least one arm");
    }
    
    /* Type check the first arm to get the expected result type */
    MatchArm first_arm = expr->arms->data[0];
    
    /* Push scope for pattern bindings */
    checker_push_scope(checker);
    bind_pattern(checker, first_arm.pattern, scrutinee_type);
    
    /* Check guard if present */
    if (first_arm.guard) {
        Type* guard_type = checker_infer_expr(checker, first_arm.guard);
        if (guard_type->kind == TYPE_ERROR) {
            checker_pop_scope(checker);
            return guard_type;
        }
        if (guard_type->kind != TYPE_BOOL) {
            checker_pop_scope(checker);
            return error_type(checker, "Match guard must be Bool, got %s",
                string_cstr(type_to_string(checker->arena, guard_type)));
        }
    }
    
    Type* result_type = checker_infer_expr(checker, first_arm.body);
    checker_pop_scope(checker);
    
    if (result_type->kind == TYPE_ERROR) return result_type;
    
    /* Check remaining arms match the result type */
    for (size_t i = 1; i < expr->arms->len; i++) {
        MatchArm arm = expr->arms->data[i];
        
        /* Push scope for pattern bindings */
        checker_push_scope(checker);
        bind_pattern(checker, arm.pattern, scrutinee_type);
        
        /* Check guard if present */
        if (arm.guard) {
            Type* guard_type = checker_infer_expr(checker, arm.guard);
            if (guard_type->kind == TYPE_ERROR) {
                checker_pop_scope(checker);
                return guard_type;
            }
            if (guard_type->kind != TYPE_BOOL) {
                checker_pop_scope(checker);
                return error_type(checker, "Match guard must be Bool, got %s",
                    string_cstr(type_to_string(checker->arena, guard_type)));
            }
        }
        
        Type* arm_type = checker_infer_expr(checker, arm.body);
        checker_pop_scope(checker);
        
        if (arm_type->kind == TYPE_ERROR) return arm_type;
        
        if (!type_equals(arm_type, result_type)) {
            return error_type(checker, "Match arm types must be equal: expected %s, got %s",
                string_cstr(type_to_string(checker->arena, result_type)),
                string_cstr(type_to_string(checker->arena, arm_type)));
        }
    }

    if (!check_match_exhaustiveness(checker, expr, scrutinee_type)) {
        return type_error(checker->arena, string_new(checker->arena, "Non-exhaustive match"));
    }
    
    return result_type;
}

/**
 * Check types for record update expressions.
 * @param checker The type checker context.
 * @param expr The record update expression.
 * @param loc Source location of the expression.
 * @return The updated record type.
 */
static Type* check_record_update_expr(Checker* checker, RecordUpdateExpr* expr, SourceLoc loc) {
    assert(checker != NULL);
    assert(expr != NULL);

    Type* base_type = checker_infer_expr(checker, expr->base);
    if (base_type->kind == TYPE_ERROR) return base_type;

    for (size_t i = 0; i < expr->fields->len; i++) {
        RecordField* field = &expr->fields->data[i];
        Type* value_type = checker_infer_expr(checker, field->value);
        if (value_type->kind == TYPE_ERROR) {
            return error_type_at(checker, loc, "Invalid record update field '%s'",
                string_cstr(field->name));
        }
    }

    /* Record field compatibility checks require user-defined record metadata.
     * For now, preserve base type while ensuring field expressions are valid. */
    return base_type;
}

/**
 * Check types for spawn(expr) actor primitive.
 * @param checker The type checker context.
 * @param expr The spawn expression.
 * @param loc Source location of the expression.
 * @return Int actor id on success.
 */
static Type* check_spawn_expr(Checker* checker, SpawnExpr* expr, SourceLoc loc) {
    assert(checker != NULL);
    assert(expr != NULL);

    if (checker->require_executable) {
        return error_type_at(checker, loc,
            "actor execution is not implemented: spawn/spawn_link cannot run workers; "
            "actors.start/post/next provide explicit mailbox operations only");
    }

    Type* func_type = checker_infer_expr(checker, expr->func);
    if (func_type->kind == TYPE_ERROR) return func_type;

    if (func_type->kind != TYPE_FN) {
        return error_type_at(checker, loc, "spawn expects a function, got %s",
            string_cstr(type_to_string(checker->arena, func_type)));
    }

    if (func_type->data.fn.params->len != 0) {
        return error_type_at(checker, loc, "spawn expects a zero-argument function, got %zu parameters",
            func_type->data.fn.params->len);
    }

    return type_int(checker->arena);
}

/**
 * Check types for spawn_link(expr) supervision primitive.
 * @param checker The type checker context.
 * @param expr The call expression.
 * @param loc Source location of the expression.
 * @return Int actor id on success.
 */
static Type* check_spawn_link_call_expr(Checker* checker, CallExpr* expr, SourceLoc loc) {
    assert(checker != NULL);
    assert(expr != NULL);

    if (expr->args->len != 1) {
        return error_type_at(checker, loc, "spawn_link expects exactly 1 argument, got %zu",
            expr->args->len);
    }

    SpawnExpr spawn_expr;
    spawn_expr.func = expr->args->data[0].value;
    return check_spawn_expr(checker, &spawn_expr, loc);
}

/**
 * Check types for send(pid, message) actor primitive.
 * @param checker The type checker context.
 * @param expr The send expression.
 * @param loc Source location of the expression.
 * @return Result(Int, Int) indicating send status.
 */
static Type* check_send_expr(Checker* checker, SendExpr* expr, SourceLoc loc) {
    assert(checker != NULL);
    assert(expr != NULL);

    Type* pid_type = checker_infer_expr(checker, expr->pid);
    if (pid_type->kind == TYPE_ERROR) return pid_type;

    if (pid_type->kind != TYPE_INT) {
        return error_type_at(checker, loc, "send expects Int pid, got %s",
            string_cstr(type_to_string(checker->arena, pid_type)));
    }

    Type* msg_type = checker_infer_expr(checker, expr->message);
    if (msg_type->kind == TYPE_ERROR) return msg_type;

    if (msg_type->kind != TYPE_STRING) {
        return error_type_at(checker, loc, "send expects String message, got %s",
            string_cstr(type_to_string(checker->arena, msg_type)));
    }

    return type_result(checker->arena, type_int(checker->arena), type_int(checker->arena));
}

/**
 * Check types for receive: pattern -> body actor primitive.
 * @param checker The type checker context.
 * @param expr The receive expression.
 * @param loc Source location of the expression.
 * @return The common arm result type.
 */
static Type* check_receive_expr(Checker* checker, ReceiveExpr* expr, SourceLoc loc) {
    assert(checker != NULL);
    assert(expr != NULL);

    if (expr->arms->len == 0 && expr->after_body == NULL) {
        return error_type_at(checker, loc, "receive requires at least one arm or an after clause");
    }

    Type* result_type = NULL;
    Type* message_type = type_string(checker->arena);

    for (size_t i = 0; i < expr->arms->len; i++) {
        MatchArm* arm = &expr->arms->data[i];

        checker_push_scope(checker);
        bind_pattern(checker, arm->pattern, message_type);

        if (arm->guard != NULL) {
            Type* guard_type = checker_infer_expr(checker, arm->guard);
            if (guard_type->kind == TYPE_ERROR) {
                checker_pop_scope(checker);
                return guard_type;
            }
            if (guard_type->kind != TYPE_BOOL) {
                checker_pop_scope(checker);
                return error_type_at(checker, loc, "receive guard must be Bool, got %s",
                    string_cstr(type_to_string(checker->arena, guard_type)));
            }
        }

        Type* arm_type = checker_infer_expr(checker, arm->body);
        checker_pop_scope(checker);

        if (arm_type->kind == TYPE_ERROR) return arm_type;

        if (result_type == NULL) {
            result_type = arm_type;
        } else if (!type_equals(result_type, arm_type)) {
            return error_type_at(checker, loc, "receive arm types must be equal: expected %s, got %s",
                string_cstr(type_to_string(checker->arena, result_type)),
                string_cstr(type_to_string(checker->arena, arm_type)));
        }
    }

    if (expr->after_timeout != NULL || expr->after_body != NULL) {
        if (expr->after_timeout == NULL || expr->after_body == NULL) {
            return error_type_at(checker, loc, "receive after clause must provide both timeout and body");
        }

        Type* timeout_type = checker_infer_expr(checker, expr->after_timeout);
        if (timeout_type->kind == TYPE_ERROR) return timeout_type;
        if (timeout_type->kind != TYPE_INT) {
            return error_type_at(checker, loc, "receive timeout must be Int, got %s",
                string_cstr(type_to_string(checker->arena, timeout_type)));
        }

        Type* after_type = checker_infer_expr(checker, expr->after_body);
        if (after_type->kind == TYPE_ERROR) return after_type;

        if (result_type == NULL) {
            result_type = after_type;
        } else if (!type_equals(result_type, after_type)) {
            return error_type_at(checker, loc, "receive after arm type must match receive arm type: expected %s, got %s",
                string_cstr(type_to_string(checker->arena, result_type)),
                string_cstr(type_to_string(checker->arena, after_type)));
        }
    }

    if (result_type == NULL) return type_unit(checker->arena);
    return result_type;
}

/* ========== Main Type Inference ========== */

/**
 * Infer the type of an expression.
 * @param checker The type checker context.
 * @param expr The expression to type check.
 * @return The inferred type.
 */
static Type* checker_infer_expr_impl(Checker* checker, Expr* expr) {
    // FERN_STYLE: allow(function-length) main type inference handles all expression types in one switch
    assert(checker != NULL);
    assert(expr != NULL);
    switch (expr->type) {
        case EXPR_INT_LIT:
            return type_int(checker->arena);
            
        case EXPR_FLOAT_LIT:
            return type_float(checker->arena);
            
        case EXPR_STRING_LIT:
            return type_string(checker->arena);
            
        case EXPR_BOOL_LIT:
            return type_bool(checker->arena);
            
        case EXPR_IDENT: {
            Type* t = type_env_lookup(checker->env, expr->data.ident.name);
            if (!t) {
                return error_type_at(checker, expr->loc, "Undefined variable: %s",
                    string_cstr(expr->data.ident.name));
            }
            return t;
        }
            
        case EXPR_BINARY:
            return check_binary_expr(checker, &expr->data.binary);
            
        case EXPR_UNARY:
            return check_unary_expr(checker, &expr->data.unary);
            
        case EXPR_LIST:
            return check_list_expr(checker, &expr->data.list);
            
        case EXPR_TUPLE:
            return check_tuple_expr(checker, &expr->data.tuple);
            
        case EXPR_CALL:
            return check_call_expr(checker, expr);
            
        case EXPR_IF:
            return check_if_expr(checker, &expr->data.if_expr);
            
        case EXPR_BLOCK:
            return check_block_expr(checker, &expr->data.block);
            
        case EXPR_MATCH:
            return check_match_expr(checker, &expr->data.match_expr);
            
        case EXPR_BIND: {
            /* Bind expression: name <- value
             * Value must be Result(ok, err), binds name to ok type.
             * The expression itself returns the ok type (for use in blocks).
             */
            BindExpr* bind = &expr->data.bind;
            Type* value_type = checker_infer_expr(checker, bind->value);
            if (value_type->kind == TYPE_ERROR) return value_type;
            
            /* Value must be a Result type */
            if (!type_is_result(value_type)) {
                return error_type_at(checker, expr->loc, "The <- operator requires a Result type, got %s",
                    string_cstr(type_to_string(checker->arena, value_type)));
            }
            
            /* Extract the Ok type (first type argument of Result) */
            Type* ok_type = value_type->data.con.args->data[0];
            
            /* Bind the name to the Ok type in the environment */
            checker_define(checker, bind->name, ok_type);
            
            return ok_type;
        }
            
        case EXPR_WITH: {
            /* With expression: with bindings do body [else arms]
             * Each binding must be Result type, binds name to ok type.
             * Body is evaluated with all bindings in scope.
             */
            WithExpr* with = &expr->data.with_expr;
            Type* with_err_type = NULL;
            
            /* Push a new scope for the bindings */
            checker_push_scope(checker);
            
            /* Process each binding */
            for (size_t i = 0; i < with->bindings->len; i++) {
                WithBinding* binding = &with->bindings->data[i];
                Type* value_type = checker_infer_expr(checker, binding->value);
                
                if (value_type->kind == TYPE_ERROR) {
                    checker_pop_scope(checker);
                    return value_type;
                }
                
                /* Value must be a Result type */
                if (!type_is_result(value_type)) {
                    checker_pop_scope(checker);
                    return error_type(checker, "with binding requires Result type, got %s",
                        string_cstr(type_to_string(checker->arena, value_type)));
                }
                
                /* Extract Ok type and bind the name */
                Type* ok_type = value_type->data.con.args->data[0];
                Type* err_type = value_type->data.con.args->data[1];

                if (with_err_type == NULL) {
                    with_err_type = err_type;
                } else if (!type_equals(err_type, with_err_type)) {
                    checker_pop_scope(checker);
                    return error_type_at(checker, expr->loc,
                        "with bindings must share the same error type: expected %s, got %s",
                        string_cstr(type_to_string(checker->arena, with_err_type)),
                        string_cstr(type_to_string(checker->arena, err_type)));
                }
                checker_define(checker, binding->name, ok_type);
            }
            
            /* Type check the body with bindings in scope */
            Type* body_type = checker_infer_expr(checker, with->body);
            
            checker_pop_scope(checker);
            
            if (body_type->kind == TYPE_ERROR) return body_type;
            
            if (with->else_arms && with->else_arms->len > 0) {
                if (with_err_type == NULL) {
                    return error_type_at(checker, expr->loc,
                        "with expression must have at least one binding");
                }

                Type* failed_result_type = type_result(checker->arena, body_type, with_err_type);
                for (size_t i = 0; i < with->else_arms->len; i++) {
                    MatchArm arm = with->else_arms->data[i];

                    checker_push_scope(checker);
                    bind_pattern(checker, arm.pattern, failed_result_type);

                    Type* arm_type = checker_infer_expr(checker, arm.body);
                    checker_pop_scope(checker);

                    if (arm_type->kind == TYPE_ERROR) return arm_type;

                    if (!type_equals(arm_type, body_type)) {
                        return error_type_at(checker, expr->loc,
                            "with else arm types must match do body type: expected %s, got %s",
                            string_cstr(type_to_string(checker->arena, body_type)),
                            string_cstr(type_to_string(checker->arena, arm_type)));
                    }
                }
            } else {
                if (with_err_type == NULL) {
                    return error_type_at(checker, expr->loc,
                        "with expression must have at least one binding");
                }
                Type* ctx_err = require_result_propagation_context(
                    checker, expr->loc, with_err_type, "with");
                if (ctx_err != NULL) return ctx_err;
            }
            
            return body_type;
        }
            
        case EXPR_LAMBDA: {
            /* Lambda expression: (params) -> body
             * Creates a function type with fresh type variables for params.
             */
            LambdaExpr* lambda = &expr->data.lambda;
            
            /* Push a new scope for lambda parameters */
            checker_push_scope(checker);
            
            /* Create fresh type variables for each parameter */
            TypeVec* param_types = TypeVec_new(checker->arena);
            for (size_t i = 0; i < lambda->params->len; i++) {
                int var_id = type_fresh_var_id();
                Type* param_type = type_var(checker->arena, lambda->params->data[i], var_id);
                TypeVec_push(checker->arena, param_types, param_type);
                checker_define(checker, lambda->params->data[i], param_type);
            }
            
            /* Infer the body type with parameters in scope */
            Type* body_type = checker_infer_expr(checker, lambda->body);
            
            checker_pop_scope(checker);
            
            if (body_type->kind == TYPE_ERROR) return body_type;
            
            /* Create and return function type */
            return type_fn(checker->arena, param_types, body_type);
        }
            
        case EXPR_FOR: {
            /* For loop: for var in iterable: body
             * Iterable must be a List or Range, var is bound to element type.
             * For loop returns Unit (side-effect only).
             */
            ForExpr* for_loop = &expr->data.for_loop;
            
            /* Infer type of iterable */
            Type* iter_type = checker_infer_expr(checker, for_loop->iterable);
            if (iter_type->kind == TYPE_ERROR) return iter_type;
            
            /* Iterable must be a List or Range */
            Type* elem_type = NULL;
            if (iter_type->kind == TYPE_CON) {
                const char* type_name = string_cstr(iter_type->data.con.name);
                if (strcmp(type_name, "List") == 0) {
                    /* Extract element type from List(elem) */
                    elem_type = iter_type->data.con.args->data[0];
                } else if (strcmp(type_name, "Range") == 0) {
                    /* Extract element type from Range(elem) */
                    elem_type = iter_type->data.con.args->data[0];
                }
            }
            
            if (elem_type == NULL) {
                return error_type_at(checker, expr->loc, "for loop requires List or Range, got %s",
                    string_cstr(type_to_string(checker->arena, iter_type)));
            }
            
            /* Push scope and bind loop variable */
            checker_push_scope(checker);
            checker_define(checker, for_loop->var_name, elem_type);
            
            /* Type check body (result is discarded) */
            Type* body_type = checker_infer_expr(checker, for_loop->body);
            (void)body_type; /* Body type is not used */
            
            checker_pop_scope(checker);
            
            /* For loop returns Unit */
            return type_unit(checker->arena);
        }
            
        case EXPR_INDEX: {
            /* Index expression: object[index]
             * For List, index must be Int, returns element type.
             * For Map, index must be key type, returns value type.
             */
            IndexExpr* index = &expr->data.index_expr;
            
            Type* obj_type = checker_infer_expr(checker, index->object);
            if (obj_type->kind == TYPE_ERROR) return obj_type;
            
            Type* idx_type = checker_infer_expr(checker, index->index);
            if (idx_type->kind == TYPE_ERROR) return idx_type;
            
            /* Check if indexing a List */
            if (obj_type->kind == TYPE_CON && 
                strcmp(string_cstr(obj_type->data.con.name), "List") == 0) {
                /* Index must be Int */
                if (idx_type->kind != TYPE_INT) {
                    return error_type_at(checker, expr->loc, "List index must be Int, got %s",
                        string_cstr(type_to_string(checker->arena, idx_type)));
                }
                /* Return element type */
                return obj_type->data.con.args->data[0];
            }
            
            /* Check if indexing a Map */
            if (obj_type->kind == TYPE_CON &&
                strcmp(string_cstr(obj_type->data.con.name), "Map") == 0) {
                /* Index must match key type */
                Type* key_type = obj_type->data.con.args->data[0];
                if (!type_equals(idx_type, key_type)) {
                    return error_type_at(checker, expr->loc, "Map key must be %s, got %s",
                        string_cstr(type_to_string(checker->arena, key_type)),
                        string_cstr(type_to_string(checker->arena, idx_type)));
                }
                /* Return value type */
                return obj_type->data.con.args->data[1];
            }
            
            return error_type_at(checker, expr->loc, "Cannot index type %s",
                string_cstr(type_to_string(checker->arena, obj_type)));
        }
            
        case EXPR_DOT: {
            /* Dot expression: object.field
             * For built-in modules: String.len, Tui.Panel.new, etc.
             * For tuples: returns the field type at the given index
             * For records: would look up field type (not yet implemented)
             */
            DotExpr* dot = &expr->data.dot;
            
            /* Check for built-in module access: Module.function or Nested.Module.function */
            /* Try to build a module path from the left side */
            String* module_path = try_build_module_path(checker->arena, dot->object);
            if (module_path != NULL) {
                const char* module_name = string_cstr(module_path);
                if (is_builtin_module(module_name)) {
                    const char* func_name = string_cstr(dot->field);
                    Type* fn_type = lookup_module_function(checker, module_name, func_name);
                    if (fn_type) {
                        return fn_type;
                    }
                    return error_type_at(checker, expr->loc,
                        "Unknown function '%s' in module '%s'",
                        func_name, module_name);
                }
            }
            
            /* Not a module access - evaluate the left side as an expression */
            Type* obj_type = checker_infer_expr(checker, dot->object);
            if (obj_type->kind == TYPE_ERROR) return obj_type;
            
            /* Tuple field access: tuple.0, tuple.1, etc. */
            if (obj_type->kind == TYPE_TUPLE) {
                const char* field_str = string_cstr(dot->field);
                /* Parse field as index */
                char* endptr;
                long index = strtol(field_str, &endptr, 10);
                if (*endptr != '\0' || index < 0) {
                    return error_type_at(checker, expr->loc, 
                        "Invalid tuple field: %s (expected numeric index)", field_str);
                }
                
                size_t idx = (size_t)index;
                if (idx >= obj_type->data.tuple.elements->len) {
                    return error_type_at(checker, expr->loc,
                        "Tuple index %zu out of bounds (tuple has %zu elements)",
                        idx, obj_type->data.tuple.elements->len);
                }
                
                return obj_type->data.tuple.elements->data[idx];
            }
            
            /* TODO: Record field access */
            return error_type_at(checker, expr->loc, "Cannot access field '%s' on type %s",
                string_cstr(dot->field),
                string_cstr(type_to_string(checker->arena, obj_type)));
        }
            
        case EXPR_RANGE: {
            /* Range expression: start..end or start..=end
             * Both bounds must be the same type (typically Int)
             * Returns Range(element_type)
             */
            RangeExpr* range = &expr->data.range;
            Type* start_type = checker_infer_expr(checker, range->start);
            if (start_type->kind == TYPE_ERROR) return start_type;
            
            Type* end_type = checker_infer_expr(checker, range->end);
            if (end_type->kind == TYPE_ERROR) return end_type;
            
            /* Bounds must have the same type */
            if (!type_equals(start_type, end_type)) {
                return error_type_at(checker, expr->loc,
                    "Range bounds must have same type: %s vs %s",
                    string_cstr(type_to_string(checker->arena, start_type)),
                    string_cstr(type_to_string(checker->arena, end_type)));
            }
            
            /* Return Range(element_type) */
            TypeVec* args = TypeVec_new(checker->arena);
            TypeVec_push(checker->arena, args, start_type);
            return type_con(checker->arena, string_new(checker->arena, "Range"), args);
        }
            
        case EXPR_MAP: {
            /* Map literal: %{ key: value, ... }
             * All keys must have same type, all values must have same type
             * Returns Map(key_type, value_type)
             */
            MapExpr* map = &expr->data.map;
            
            if (map->entries->len == 0) {
                /* Empty map has type Map(a, b) with fresh type variables */
                int key_var = type_fresh_var_id();
                int val_var = type_fresh_var_id();
                Type* key_type = type_var(checker->arena, string_new(checker->arena, "k"), key_var);
                Type* val_type = type_var(checker->arena, string_new(checker->arena, "v"), val_var);
                return type_map(checker->arena, key_type, val_type);
            }
            
            /* Infer types from first entry */
            Type* key_type = checker_infer_expr(checker, map->entries->data[0].key);
            if (key_type->kind == TYPE_ERROR) return key_type;
            
            Type* val_type = checker_infer_expr(checker, map->entries->data[0].value);
            if (val_type->kind == TYPE_ERROR) return val_type;
            
            /* Check all entries have consistent types */
            for (size_t i = 1; i < map->entries->len; i++) {
                Type* k = checker_infer_expr(checker, map->entries->data[i].key);
                if (k->kind == TYPE_ERROR) return k;
                
                if (!type_equals(key_type, k)) {
                    return error_type_at(checker, expr->loc,
                        "Map key type mismatch at entry %zu: expected %s, got %s",
                        i, string_cstr(type_to_string(checker->arena, key_type)),
                        string_cstr(type_to_string(checker->arena, k)));
                }
                
                Type* v = checker_infer_expr(checker, map->entries->data[i].value);
                if (v->kind == TYPE_ERROR) return v;
                
                if (!type_equals(val_type, v)) {
                    return error_type_at(checker, expr->loc,
                        "Map value type mismatch at entry %zu: expected %s, got %s",
                        i, string_cstr(type_to_string(checker->arena, val_type)),
                        string_cstr(type_to_string(checker->arena, v)));
                }
            }
            
            return type_map(checker->arena, key_type, val_type);
        }
            
        case EXPR_LIST_COMP: {
            /* List comprehension: [body for var in iterable if condition] */
            ListCompExpr* comp = &expr->data.list_comp;
            
            /* Infer type of iterable */
            Type* iter_type = checker_infer_expr(checker, comp->iterable);
            if (iter_type->kind == TYPE_ERROR) return iter_type;
            
            /* Iterable must be List(T) or Range */
            Type* elem_type = NULL;
            if (iter_type->kind == TYPE_CON) {
                const char* name = string_cstr(iter_type->data.con.name);
                if (strcmp(name, "List") == 0 && iter_type->data.con.args && 
                    iter_type->data.con.args->len > 0) {
                    elem_type = iter_type->data.con.args->data[0];
                } else if (strcmp(name, "Range") == 0) {
                    /* Range iterates over Int */
                    elem_type = type_int(checker->arena);
                }
            }
            
            if (!elem_type) {
                return error_type_at(checker, expr->loc, 
                    "List comprehension requires an iterable (List or Range), got %s",
                    string_cstr(type_to_string(checker->arena, iter_type)));
            }
            
            /* Push scope and bind loop variable */
            checker_push_scope(checker);
            checker_define(checker, comp->var_name, elem_type);
            
            /* Check filter condition if present */
            if (comp->condition) {
                Type* cond_type = checker_infer_expr(checker, comp->condition);
                if (cond_type->kind == TYPE_ERROR) {
                    checker_pop_scope(checker);
                    return cond_type;
                }
                if (cond_type->kind != TYPE_BOOL) {
                    checker_pop_scope(checker);
                    return error_type_at(checker, expr->loc,
                        "List comprehension filter must be Bool, got %s",
                        string_cstr(type_to_string(checker->arena, cond_type)));
                }
            }
            
            /* Infer body type */
            Type* body_type = checker_infer_expr(checker, comp->body);
            checker_pop_scope(checker);
            
            if (body_type->kind == TYPE_ERROR) return body_type;
            
            /* Result is List(body_type) */
            return type_list(checker->arena, body_type);
        }
            
        case EXPR_INTERP_STRING: {
            /* String interpolation: "Hello, {name}!" */
            InterpStringExpr* interp = &expr->data.interp_string;
            
            /* Type check each part - string literals and interpolated expressions */
            for (size_t i = 0; i < interp->parts->len; i++) {
                Type* part_type = checker_infer_expr(checker, interp->parts->data[i]);
                if (part_type->kind == TYPE_ERROR) return part_type;
                /* All parts get implicitly converted to string (assume Show trait) */
            }
            
            /* Result is always String */
            return type_string(checker->arena);
        }
            
        case EXPR_RECORD_UPDATE:
            return check_record_update_expr(checker, &expr->data.record_update, expr->loc);

        case EXPR_TRY: {
            /* The ? operator unwraps Result types */
            Type* operand_type = checker_infer_expr(checker, expr->data.try_expr.operand);
            if (operand_type->kind == TYPE_ERROR) return operand_type;
            
            /* Operand must be Result(ok, err) */
            if (!type_is_result(operand_type)) {
                return error_type_at(checker, expr->loc, "The ? operator requires a Result type, got %s",
                    string_cstr(type_to_string(checker->arena, operand_type)));
            }

            Type* propagated_err = operand_type->data.con.args->data[1];
            Type* ctx_err = require_result_propagation_context(
                checker, expr->loc, propagated_err, "?");
            if (ctx_err != NULL) return ctx_err;
            
            /* Return the Ok type (first type argument) */
            return operand_type->data.con.args->data[0];
        }
            
        case EXPR_SPAWN:
            return check_spawn_expr(checker, &expr->data.spawn_expr, expr->loc);

        case EXPR_SEND:
            return check_send_expr(checker, &expr->data.send_expr, expr->loc);

        case EXPR_RECEIVE:
            if (checker->require_executable) {
                return error_type_at(checker, expr->loc,
                    "actor execution is not implemented: receive cannot wait for messages; "
                    "actors.start/post/next provide explicit mailbox operations only");
            }
            return check_receive_expr(checker, &expr->data.receive_expr, expr->loc);
    }
    
    return error_type(checker, "Unknown expression type");
}

/**
 * Infer the type of an expression with location-aware diagnostics.
 * @param checker The type checker context.
 * @param expr The expression to type check.
 * @return The inferred type.
 */
Type* checker_infer_expr(Checker* checker, Expr* expr) {
    assert(checker != NULL);
    assert(expr != NULL);

    SourceLoc previous_loc = checker->current_expr_loc;
    checker->current_expr_loc = expr->loc;
    Type* result = checker_infer_expr_impl(checker, expr);
    expr->checked_type = result;
    checker->current_expr_loc = previous_loc;
    return result;
}

/* ========== Type Expression Resolution ========== */

/**
 * Convert a TypeExpr (AST type annotation) to a Type.
 * @param checker The type checker context.
 * @param type_expr The type expression to resolve.
 * @return The resolved type.
 */
static Type* resolve_type_expr(Checker* checker, TypeExpr* type_expr) {
    // FERN_STYLE: allow(function-length) type expression resolution handles all type forms
    assert(checker != NULL);
    if (!type_expr) return NULL;
    assert(type_expr->kind == TYPEEXPR_NAMED || type_expr->kind == TYPEEXPR_FUNCTION || type_expr->kind == TYPEEXPR_TUPLE);
    
    switch (type_expr->kind) {
        case TYPEEXPR_NAMED: {
            String* name = type_expr->data.named.name;
            const char* name_str = string_cstr(name);
            
            /* Check for built-in primitive types */
            if (strcmp(name_str, "Int") == 0) {
                return type_int(checker->arena);
            }
            if (strcmp(name_str, "Float") == 0) {
                return type_float(checker->arena);
            }
            if (strcmp(name_str, "String") == 0) {
                return type_string(checker->arena);
            }
            if (strcmp(name_str, "Bool") == 0) {
                return type_bool(checker->arena);
            }
            if (strcmp(name_str, "()") == 0 || strcmp(name_str, "Unit") == 0) {
                return type_unit(checker->arena);
            }
            
            /* Check for parameterized types */
            TypeExprVec* args = type_expr->data.named.args;
            if (args && args->len > 0) {
                /* Resolve type arguments */
                TypeVec* resolved_args = TypeVec_new(checker->arena);
                for (size_t i = 0; i < args->len; i++) {
                    Type* arg = resolve_type_expr(checker, args->data[i]);
                    if (!arg || arg->kind == TYPE_ERROR) return arg;
                    TypeVec_push(checker->arena, resolved_args, arg);
                }
                
                /* Common parameterized types */
                if (strcmp(name_str, "List") == 0 && resolved_args->len == 1) {
                    return type_list(checker->arena, resolved_args->data[0]);
                }
                if (strcmp(name_str, "Option") == 0 && resolved_args->len == 1) {
                    return type_option(checker->arena, resolved_args->data[0]);
                }
                if (strcmp(name_str, "Result") == 0 && resolved_args->len == 2) {
                    return type_result(checker->arena, 
                        resolved_args->data[0], resolved_args->data[1]);
                }
                if (strcmp(name_str, "Map") == 0 && resolved_args->len == 2) {
                    return type_map(checker->arena,
                        resolved_args->data[0], resolved_args->data[1]);
                }
                
                /* User-defined parameterized type */
                return type_con(checker->arena, name, resolved_args);
            }
            
            /* Look up in type environment */
            Type* defined = type_env_lookup_type(checker->env, name);
            if (defined) {
                return defined;
            }
            
            /* Unknown type - treat as a simple type constructor for flexibility */
            return type_con(checker->arena, name, NULL);
        }
        
        case TYPEEXPR_FUNCTION: {
            /* Resolve parameter types */
            TypeVec* params = TypeVec_new(checker->arena);
            for (size_t i = 0; i < type_expr->data.func.params->len; i++) {
                Type* param = resolve_type_expr(checker, 
                    type_expr->data.func.params->data[i]);
                if (!param || param->kind == TYPE_ERROR) return param;
                TypeVec_push(checker->arena, params, param);
            }
            
            /* Resolve return type */
            Type* ret = resolve_type_expr(checker, type_expr->data.func.return_type);
            if (!ret || ret->kind == TYPE_ERROR) return ret;
            
            return type_fn(checker->arena, params, ret);
        }

        case TYPEEXPR_TUPLE: {
            /* Empty tuple () is Unit */
            if (type_expr->data.tuple.elements->len == 0) {
                return type_unit(checker->arena);
            }
            /* Resolve element types */
            TypeVec* elements = TypeVec_new(checker->arena);
            for (size_t i = 0; i < type_expr->data.tuple.elements->len; i++) {
                Type* elem = resolve_type_expr(checker,
                    type_expr->data.tuple.elements->data[i]);
                if (!elem || elem->kind == TYPE_ERROR) return elem;
                TypeVec_push(checker->arena, elements, elem);
            }
            return type_tuple(checker->arena, elements);
        }
    }
    
    return error_type(checker, "Unknown type expression kind");
}

/**
 * Resolve type expression strictly, erroring on unknown types.
 * @param checker The type checker context.
 * @param type_expr The type expression to resolve.
 * @return The resolved type or error.
 */
static Type* resolve_type_expr_strict(Checker* checker, TypeExpr* type_expr) {
    assert(checker != NULL);
    if (!type_expr) return NULL;
    assert(type_expr->kind == TYPEEXPR_NAMED || type_expr->kind == TYPEEXPR_FUNCTION || type_expr->kind == TYPEEXPR_TUPLE);
    
    if (type_expr->kind == TYPEEXPR_NAMED) {
        String* name = type_expr->data.named.name;
        const char* name_str = string_cstr(name);
        
        /* Check for built-in primitive types */
        if (strcmp(name_str, "Int") == 0 ||
            strcmp(name_str, "Float") == 0 ||
            strcmp(name_str, "String") == 0 ||
            strcmp(name_str, "Bool") == 0 || strcmp(name_str, "Unit") == 0) {
            return resolve_type_expr(checker, type_expr);
        }
        
        /* Check for parameterized types (List, Option, Result, Map) */
        TypeExprVec* args = type_expr->data.named.args;
        if (args && args->len > 0) {
            if (strcmp(name_str, "List") == 0 ||
                strcmp(name_str, "Option") == 0 ||
                strcmp(name_str, "Result") == 0 ||
                strcmp(name_str, "Map") == 0) {
                /* Recursively validate arguments with strict checking */
                for (size_t i = 0; i < args->len; i++) {
                    Type* arg = resolve_type_expr_strict(checker, args->data[i]);
                    if (!arg || arg->kind == TYPE_ERROR) return arg;
                }
                return resolve_type_expr(checker, type_expr);
            }
        }
        
        /* Look up in type environment */
        Type* defined = type_env_lookup_type(checker->env, name);
        if (defined) {
            return defined;
        }
        
        /* Unknown type - error in strict mode */
        return error_type(checker, "Unknown type '%s'", name_str);
    }
    
    /* For function types, use regular resolution */
    return resolve_type_expr(checker, type_expr);
}

/* ========== Statement Type Checking ========== */

/**
 * Bind a pattern to a type in the environment.
 * @param checker The type checker context.
 * @param pattern The pattern to bind.
 * @param type The type to bind to.
 * @return True if binding succeeded.
 */
static bool bind_pattern(Checker* checker, Pattern* pattern, Type* type) {
    // FERN_STYLE: allow(function-length) pattern binding handles all pattern types recursively
    assert(checker != NULL);
    assert(pattern != NULL);
    switch (pattern->type) {
        case PATTERN_IDENT:
            pattern->checked_type = type;
            checker_define(checker, pattern->data.ident, type);
            return true;
            
        case PATTERN_WILDCARD:
            /* _ doesn't bind anything */
            return true;
            
        case PATTERN_LIT:
            /* Literal patterns don't bind */
            return true;
            
        case PATTERN_TUPLE: {
            /* The empty tuple spelling denotes Unit, not an arbitrary tuple. */
            if (type->kind == TYPE_UNIT && pattern->data.tuple->len == 0) return true;
            /* Type must be a tuple with matching arity */
            if (type->kind != TYPE_TUPLE) {
                add_error(checker, "Cannot destructure non-tuple type %s",
                    string_cstr(type_to_string(checker->arena, type)));
                return false;
            }
            PatternVec* sub = pattern->data.tuple;
            TypeVec* elem_types = type->data.tuple.elements;
            if (sub->len != elem_types->len) {
                add_error(checker, "Tuple pattern has %zu elements but type has %zu",
                    sub->len, elem_types->len);
                return false;
            }
            for (size_t i = 0; i < sub->len; i++) {
                if (!bind_pattern(checker, sub->data[i], elem_types->data[i])) {
                    return false;
                }
            }
            return true;
        }
            
        case PATTERN_CONSTRUCTOR: {
            /* Constructor pattern: Some(x), Ok(value), Err(msg), None, etc.
             * We need to determine what type the sub-patterns bind to based on
             * the constructor name and the scrutinee type.
             */
            ConstructorPattern* ctor = &pattern->data.constructor;
            const char* ctor_name = string_cstr(ctor->name);
            
            /* Handle Option constructors */
            if (type->kind == TYPE_CON && 
                strcmp(string_cstr(type->data.con.name), "Option") == 0) {
                if (strcmp(ctor_name, "Some") == 0) {
                    /* Some(x) binds x to the inner type of Option(a) */
                    if (ctor->args && ctor->args->len == 1) {
                        Type* inner_type = type->data.con.args->data[0];
                        return bind_pattern(checker, ctor->args->data[0], inner_type);
                    }
                } else if (strcmp(ctor_name, "None") == 0) {
                    /* None doesn't bind anything */
                    return true;
                }
            }
            
            /* Handle Result constructors */
            if (type->kind == TYPE_CON &&
                strcmp(string_cstr(type->data.con.name), "Result") == 0) {
                if (strcmp(ctor_name, "Ok") == 0) {
                    /* Ok(x) binds x to the first type arg of Result(ok, err) */
                    if (ctor->args && ctor->args->len == 1) {
                        Type* ok_type = type->data.con.args->data[0];
                        return bind_pattern(checker, ctor->args->data[0], ok_type);
                    }
                } else if (strcmp(ctor_name, "Err") == 0) {
                    /* Err(e) binds e to the second type arg of Result(ok, err) */
                    if (ctor->args && ctor->args->len == 1) {
                        Type* err_type = type->data.con.args->data[1];
                        return bind_pattern(checker, ctor->args->data[0], err_type);
                    }
                }
            }
            
            /* For other constructors, we'd need a type environment with
             * constructor definitions. For now, just return true. */
            return true;
        }
            
        case PATTERN_REST:
            /* TODO: Implement rest pattern binding */
            return true;
    }
    
    return false;
}

/**
 * Check whether a pattern is catch-all (covers any value).
 * @param pattern Pattern to inspect.
 * @return True if pattern catches all values.
 */
static bool is_catch_all_pattern(Pattern* pattern) {
    assert(pattern != NULL);
    assert(pattern->type >= PATTERN_IDENT && pattern->type <= PATTERN_REST);
    return pattern->type == PATTERN_WILDCARD || pattern->type == PATTERN_IDENT;
}

/**
 * Check if a match expression is exhaustive for built-in sum types.
 * Currently checks Bool, Option, and Result.
 * @param checker Type checker context.
 * @param expr Match expression.
 * @param scrutinee_type Type being matched.
 * @return True if exhaustive or if type is not yet checked for exhaustiveness.
 */
static bool check_match_exhaustiveness(Checker* checker, MatchExpr* expr, Type* scrutinee_type) {
    // FERN_STYLE: allow(function-length) exhaustiveness logic handles Bool/Option/Result coverage in one pass
    assert(checker != NULL);
    assert(expr != NULL);
    assert(scrutinee_type != NULL);

    bool has_catch_all = false;
    bool has_true = false;
    bool has_false = false;
    bool has_some = false;
    bool has_none = false;
    bool has_ok = false;
    bool has_err = false;

    for (size_t i = 0; i < expr->arms->len; i++) {
        MatchArm arm = expr->arms->data[i];
        Pattern* pattern = arm.pattern;

        assert(pattern != NULL);

        /* Guarded arms are not total coverage for a pattern. */
        if (arm.guard != NULL) {
            continue;
        }

        if (is_catch_all_pattern(pattern)) {
            has_catch_all = true;
            break;
        }

        if (scrutinee_type->kind == TYPE_BOOL &&
            pattern->type == PATTERN_LIT &&
            pattern->data.literal != NULL &&
            pattern->data.literal->type == EXPR_BOOL_LIT) {
            if (pattern->data.literal->data.bool_lit.value) {
                has_true = true;
            } else {
                has_false = true;
            }
            continue;
        }

        if (scrutinee_type->kind == TYPE_CON && pattern->type == PATTERN_CONSTRUCTOR) {
            const char* type_name = string_cstr(scrutinee_type->data.con.name);
            const char* ctor_name = string_cstr(pattern->data.constructor.name);

            if (strcmp(type_name, "Option") == 0) {
                if (strcmp(ctor_name, "Some") == 0) {
                    has_some = true;
                } else if (strcmp(ctor_name, "None") == 0) {
                    has_none = true;
                }
            } else if (strcmp(type_name, "Result") == 0) {
                if (strcmp(ctor_name, "Ok") == 0) {
                    has_ok = true;
                } else if (strcmp(ctor_name, "Err") == 0) {
                    has_err = true;
                }
            }
        }
    }

    if (has_catch_all) {
        return true;
    }

    if (scrutinee_type->kind == TYPE_BOOL) {
        if (has_true && has_false) {
            return true;
        }
        add_error(checker, "Non-exhaustive match for Bool: missing %s",
            has_true ? "false arm" : "true arm");
        return false;
    }

    if (type_is_option(scrutinee_type)) {
        if (has_some && has_none) {
            return true;
        }
        add_error(checker, "Non-exhaustive match for Option: missing %s",
            has_some ? "None arm" : "Some arm");
        return false;
    }

    if (type_is_result(scrutinee_type)) {
        if (has_ok && has_err) {
            return true;
        }
        add_error(checker, "Non-exhaustive match for Result: missing %s",
            has_ok ? "Err arm" : "Ok arm");
        return false;
    }

    return true;
}

/**
 * Ensure Result-propagating operators are used in a compatible function context.
 * @param checker Type checker context.
 * @param loc Source location for diagnostics.
 * @param propagated_err Error type being propagated.
 * @param op_name Operator label for diagnostics.
 * @return NULL on success, or an error type on failure.
 */
static Type* require_result_propagation_context(Checker* checker, SourceLoc loc, Type* propagated_err, const char* op_name) {
    assert(checker != NULL);
    assert(propagated_err != NULL);
    assert(op_name != NULL);

    if (checker->current_return_type == NULL) {
        return error_type_at(checker, loc,
            "The %s operator can only be used inside functions returning Result(...)", op_name);
    }

    int var_id = type_fresh_var_id();
    Type* any_ok = type_var(checker->arena, string_new(checker->arena, "_ok"), var_id);
    Type* expected_ret = type_result(checker->arena, any_ok, propagated_err);

    if (!unify(checker->current_return_type, expected_ret)) {
        Type* current_ret = substitute(checker->arena, checker->current_return_type);
        return error_type_at(checker, loc,
            "The %s operator requires function return type Result(_, %s), but function returns %s",
            op_name,
            string_cstr(type_to_string(checker->arena, propagated_err)),
            string_cstr(type_to_string(checker->arena, current_ret)));
    }

    return NULL;
}

/**
 * Check types for let statements.
 * @param checker The type checker context.
 * @param stmt The let statement to check.
 * @return True if type checking succeeded.
 */
static bool check_let_stmt(Checker* checker, LetStmt* stmt) {
    assert(checker != NULL);
    assert(stmt != NULL);
    /* Infer type of the value expression */
    Type* value_type = checker_infer_expr(checker, stmt->value);
    if (value_type->kind == TYPE_ERROR) {
        return false;
    }
    
    /* If there's a type annotation, resolve it and check compatibility */
    if (stmt->type_ann) {
        Type* annotated = resolve_type_expr(checker, stmt->type_ann);
        if (!annotated || annotated->kind == TYPE_ERROR) {
            return false;
        }
        
        /* Use unify instead of type_assignable to handle type variables
         * (e.g., empty list [] has type List(a) which should unify with List(Concrete)) */
        if (!unify(value_type, annotated)) {
            add_error(checker, "Type mismatch: expected %s, got %s",
                string_cstr(type_to_string(checker->arena, annotated)),
                string_cstr(type_to_string(checker->arena, value_type)));
            return false;
        }
        
        /* Use the annotated type for the binding */
        value_type = annotated;
    }
    
    /* Bind the pattern to the inferred/annotated type */
    return bind_pattern(checker, stmt->pattern, value_type);
}

/**
 * Check types for a single statement.
 * @param checker The type checker context.
 * @param stmt The statement to check.
 * @return True if type checking succeeded.
 */
bool checker_check_stmt(Checker* checker, Stmt* stmt) {
    // FERN_STYLE: allow(function-length) statement checking handles all statement types
    assert(checker != NULL);
    assert(stmt != NULL);
    switch (stmt->type) {
        case STMT_LET:
            return check_let_stmt(checker, &stmt->data.let);
            
        case STMT_EXPR:
            /* Type check the expression but discard the result */
            {
                Type* expr_type = checker_infer_expr(checker, stmt->data.expr.expr);
                if (expr_type->kind == TYPE_ERROR) {
                    return false;
                }
                if (type_is_result(expr_type)) {
                    add_error(checker, "Unhandled Result value of type %s; use match, with, or ?",
                        string_cstr(type_to_string(checker->arena, expr_type)));
                    return false;
                }
                return !checker_has_errors(checker);
            }
            
        case STMT_FN: {
            /* Function definition: fn name(params) -> return_type: body
             * - Add parameters to scope
             * - Type check body
             * - Verify body type matches return type annotation (if present)
             */
            FunctionDef* fn = &stmt->data.fn;
            Type* prev_return_type = checker->current_return_type;
            
            /* Push scope for function parameters */
            checker_push_scope(checker);
            
            /* Add parameters to scope with their annotated types */
            if (fn->params) {
                for (size_t i = 0; i < fn->params->len; i++) {
                    Parameter* param = &fn->params->data[i];
                    Type* param_type = NULL;
                    
                    if (param->type_ann) {
                        param_type = resolve_type_expr(checker, param->type_ann);
                        if (!param_type || param_type->kind == TYPE_ERROR) {
                            checker_pop_scope(checker);
                            checker->current_return_type = prev_return_type;
                            return false;
                        }
                    } else {
                        /* No type annotation - use fresh type variable */
                        int var_id = type_fresh_var_id();
                        param_type = type_var(checker->arena, param->name, var_id);
                    }
                    
                    checker_define(checker, param->name, param_type);
                }
            }

            /* Set current function return type for Result-propagating operators. */
            Type* fn_sig = type_env_lookup(checker->env, fn->name);
            if (fn_sig && fn_sig->kind == TYPE_FN) {
                checker->current_return_type = fn_sig->data.fn.result;
            } else {
                checker->current_return_type = NULL;
            }
            
            /* Type check the function body */
            Type* body_type = checker_infer_expr(checker, fn->body);
            checker->current_return_type = prev_return_type;
            
            checker_pop_scope(checker);
            
            if (body_type->kind == TYPE_ERROR) {
                return false;
            }
            
            /* If there's a return type annotation, verify body matches */
            if (fn->return_type) {
                Type* return_type = resolve_type_expr(checker, fn->return_type);
                if (!return_type || return_type->kind == TYPE_ERROR) {
                    return false;
                }
                
                /* Use unify instead of type_assignable to handle type variables
                 * (e.g., Ok(42) returns Result(Int, e) which should unify with Result(Int, String)) */
                if (!unify(body_type, return_type)) {
                    SourceLoc err_loc = fn->body ? fn->body->loc : stmt->loc;
                    add_error_at(checker, err_loc,
                        "Function '%s' body has type %s, but declared return type is %s",
                        string_cstr(fn->name),
                        string_cstr(type_to_string(checker->arena, body_type)),
                        string_cstr(type_to_string(checker->arena, return_type)));
                    return false;
                }
            }
            
            /* TODO: Register function in environment for recursive calls */
            return true;
        }
            
        case STMT_TYPE_DEF: {
            TypeDef* def = &stmt->data.type_def;
            
            // Validate variant field types
            if (def->variants) {
                for (size_t i = 0; i < def->variants->len; i++) {
                    TypeVariant* variant = &def->variants->data[i];
                    if (variant->fields) {
                        for (size_t j = 0; j < variant->fields->len; j++) {
                            TypeField* field = &variant->fields->data[j];
                            if (field->type_ann) {
                                Type* field_type = resolve_type_expr_strict(checker, field->type_ann);
                                if (!field_type || field_type->kind == TYPE_ERROR) {
                                    add_error(checker, "Unknown type in variant '%s' field '%s'",
                                        string_cstr(variant->name),
                                        string_cstr(field->name));
                                    return false;
                                }
                            }
                        }
                    }
                }
            }
            
            // Validate record field types
            if (def->record_fields) {
                for (size_t i = 0; i < def->record_fields->len; i++) {
                    TypeField* field = &def->record_fields->data[i];
                    if (field->type_ann) {
                        Type* field_type = resolve_type_expr_strict(checker, field->type_ann);
                        if (!field_type || field_type->kind == TYPE_ERROR) {
                            add_error(checker, "Unknown type in record field '%s'",
                                string_cstr(field->name));
                            return false;
                        }
                    }
                }
            }
            
            /* TODO: Register type in environment for use in other definitions */
            return true;
        }
            
        case STMT_RETURN:
        case STMT_IMPORT:
        case STMT_DEFER:
        case STMT_BREAK:
        case STMT_CONTINUE:
        case STMT_TRAIT:
        case STMT_IMPL:
        case STMT_NEWTYPE:
        case STMT_MODULE:
            /* TODO: Implement other statement types */
            return true;
    }
    
    return false;
}

/**
 * Check types for a list of statements.
 * Uses two passes: first registers all function signatures, then type checks bodies.
 * @param checker The type checker context.
 * @param stmts The statements to check.
 * @return True if type checking succeeded.
 */
bool checker_check_stmts(Checker* checker, StmtVec* stmts) {
    assert(checker != NULL);
    assert(stmts != NULL);
    
    /* Pass 1: Register all function signatures in the environment.
     * This allows functions to call each other regardless of definition order,
     * and enables recursive calls.
     */
    for (size_t i = 0; i < stmts->len; i++) {
        Stmt* stmt = stmts->data[i];
        if (stmt->type == STMT_FN) {
            FunctionDef* fn = &stmt->data.fn;
            
            /* Build function type from parameter and return type annotations */
            TypeVec* param_types = TypeVec_new(checker->arena);
            
            if (fn->params) {
                for (size_t j = 0; j < fn->params->len; j++) {
                    Parameter* param = &fn->params->data[j];
                    Type* param_type = NULL;
                    
                    if (param->type_ann) {
                        param_type = resolve_type_expr(checker, param->type_ann);
                        if (!param_type || param_type->kind == TYPE_ERROR) {
                            /* Use a placeholder type variable on error */
                            param_type = type_var(checker->arena, param->name, type_fresh_var_id());
                        }
                    } else {
                        /* No annotation: use fresh type variable */
                        param_type = type_var(checker->arena, param->name, type_fresh_var_id());
                    }
                    
                    TypeVec_push(checker->arena, param_types, param_type);
                }
            }
            
            /* Determine return type */
            Type* return_type = NULL;
            if (fn->return_type) {
                return_type = resolve_type_expr(checker, fn->return_type);
                if (!return_type || return_type->kind == TYPE_ERROR) {
                    return_type = type_var(checker->arena, string_new(checker->arena, "result"), type_fresh_var_id());
                }
            } else {
                /* No return type annotation:
                 * - For main(), default to Unit (allows `fn main():` shorthand)
                 * - For other functions, use fresh type variable for inference
                 */
                if (strcmp(string_cstr(fn->name), "main") == 0) {
                    return_type = type_unit(checker->arena);
                } else {
                    return_type = type_var(checker->arena, string_new(checker->arena, "result"), type_fresh_var_id());
                }
            }
            
            /* Create function type and register it */
            Type* fn_type = type_fn(checker->arena, param_types, return_type);
            checker_define(checker, fn->name, fn_type);
        }
    }
    
    /* Pass 2: Type check all statements (including function bodies) */
    for (size_t i = 0; i < stmts->len; i++) {
        if (!checker_check_stmt(checker, stmts->data[i])) {
            return false;
        }
    }
    return true;
}

/* ========== Error Handling ========== */

/**
 * Check if the checker has recorded any errors.
 * @param checker The type checker context.
 * @return True if there are errors.
 */
bool checker_has_errors(Checker* checker) {
    assert(checker != NULL);
    assert(checker->arena != NULL);
    return checker->errors != NULL;
}

/**
 * Get the first error message, or NULL if none.
 * @param checker The type checker context.
 * @return The first error message or NULL.
 */
const char* checker_first_error(Checker* checker) {
    assert(checker != NULL);
    assert(checker->arena != NULL);
    if (!checker->errors) return NULL;
    return string_cstr(checker->errors->message);
}

/**
 * @brief Clear all accumulated errors.
 *
 * Resets the error list so the checker can be reused for a new
 * evaluation (e.g., in a REPL).
 *
 * @param checker Checker instance (must not be NULL)
 */
void checker_clear_errors(Checker* checker) {
    // FERN_STYLE: allow(assertion-density) simple error reset
    assert(checker != NULL);
    checker->errors = NULL;
    checker->errors_tail = NULL;
}

/* ========== Environment Access ========== */

/**
 * Get the checker's type environment.
 * @param checker The type checker context.
 * @return The type environment.
 */
TypeEnv* checker_env(Checker* checker) {
    assert(checker != NULL);
    assert(checker->env != NULL);
    return checker->env;
}

/**
 * Define a variable with a type in the current scope.
 * @param checker The type checker context.
 * @param name The variable name.
 * @param type The variable type.
 */
void checker_define(Checker* checker, String* name, Type* type) {
    assert(checker != NULL);
    assert(name != NULL);
    type_env_define(checker->env, name, type);
}

/**
 * Push a new scope onto the checker's environment.
 * @param checker The type checker context.
 */
void checker_push_scope(Checker* checker) {
    assert(checker != NULL);
    assert(checker->env != NULL);
    type_env_push_scope(checker->env);
}

/**
 * Pop the current scope from the checker's environment.
 * @param checker The type checker context.
 */
void checker_pop_scope(Checker* checker) {
    assert(checker != NULL);
    assert(checker->env != NULL);
    type_env_pop_scope(checker->env);
}
