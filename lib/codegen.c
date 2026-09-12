/* Fern Code Generator
 *
 * Generates QBE IR from type-checked Fern AST.
 */

#include "codegen.h"
#include "ast.h"
#include "type.h"
#include "arena.h"
#include "fern_string.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <assert.h>

/* ========== Codegen Structure ========== */

/* Maximum number of deferred expressions in a single function */
#define MAX_DEFERS 64

/* Maximum number of tracked wide (64-bit) variables */
#define MAX_WIDE_VARS 256

/* Maximum number of tracked functions that return tuples (pointers) */
#define MAX_TUPLE_FUNCS 128

struct Codegen {
    Arena* arena;
    StmtVec* program_stmts; /* Signatures retain semantic types for output dispatch. */
    String* output;      /* Accumulated QBE IR (functions) */
    String* data_section;/* Accumulated data section (strings, etc.) */
    int temp_counter;    /* For generating unique temporaries %t0, %t1, ... */
    int label_counter;   /* For generating unique labels @L0, @L1, ... */
    int string_counter;  /* For generating unique string labels $str0, $str1, ... */
    
    /* Defer stack for current function - expressions run in LIFO order */
    Expr* defer_stack[MAX_DEFERS];
    int defer_count;
    
    /* Track variables that are 64-bit (pointers: lists, strings) */
    String* wide_vars[MAX_WIDE_VARS];
    int wide_var_count;
    Pattern* bound_patterns[MAX_WIDE_VARS];
    int bound_pattern_count;

    /* Track named owned pointer variables for constrained dup/drop insertion. */
    String* owned_ptr_vars[MAX_WIDE_VARS];
    int owned_ptr_var_count;
    
    /* Raw function pointers are callable locals, never reference-counted objects. */
    String* function_vars[MAX_WIDE_VARS];
    int function_var_count;

    /* Track functions that return tuples (pointers) */
    String* tuple_return_funcs[MAX_TUPLE_FUNCS];
    int tuple_func_count;
    
    /* Track if a return statement was emitted - avoids unreachable code */
    bool returned;
};

/* ========== Helper Functions ========== */

/**
 * Append text to the output buffer.
 * @param cg The codegen context.
 * @param fmt The format string.
 * @param ... The format arguments.
 */
static void emit(Codegen* cg, const char* fmt, ...) {
    assert(cg != NULL);
    assert(fmt != NULL);
    char buf[1024];
    va_list args;
    va_start(args, fmt);
    vsnprintf(buf, sizeof(buf), fmt, args);
    va_end(args);
    
    cg->output = string_concat(cg->arena, cg->output, string_new(cg->arena, buf));
}

/**
 * Generate a fresh temporary name.
 * @param cg The codegen context.
 * @return The new temporary name.
 */
static String* fresh_temp(Codegen* cg) {
    assert(cg != NULL);
    assert(cg->temp_counter >= 0);
    char buf[32];
    snprintf(buf, sizeof(buf), "%%t%d", cg->temp_counter++);
    return string_new(cg->arena, buf);
}

/**
 * Generate a fresh label name.
 * @param cg The codegen context.
 * @return The new label name.
 */
static String* fresh_label(Codegen* cg) {
    assert(cg != NULL);
    assert(cg->label_counter >= 0);
    char buf[32];
    snprintf(buf, sizeof(buf), "@L%d", cg->label_counter++);
    return string_new(cg->arena, buf);
}

/**
 * Emit a label and reset the returned flag.
 * After a label, code is reachable again (it's a branch target).
 * @param cg The codegen context.
 * @param label The label to emit (without @).
 */
static void emit_label(Codegen* cg, const char* label) {
    assert(cg != NULL);
    assert(label != NULL);
    emit(cg, "%s\n", label);
    cg->returned = false;  /* Code after a label is reachable */
}

/* ========== Module Path Helpers ========== */

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
        strcmp(name, "Option") == 0 ||
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
 * Check if a name is a built-in module (for codegen).
 * @param name The name to check.
 * @return True if the name is a built-in module.
 */
static bool is_builtin_module(const char* name) {
    assert(name != NULL);
    assert(strlen(name) < 256);  /* Module names have reasonable length */
    return canonical_builtin_module_name(name) != NULL;
}

/* ========== QBE Type Helpers ========== */

/**
 * Register a variable as wide (64-bit pointer type).
 * @param cg The codegen context.
 * @param name The variable name.
 */
static void register_wide_var(Codegen* cg, String* name) {
    assert(cg != NULL);
    assert(name != NULL);
    assert(cg->wide_var_count < MAX_WIDE_VARS);
    cg->wide_vars[cg->wide_var_count++] = name;
}

/**
 * Check if a variable is wide (64-bit pointer type).
 * @param cg The codegen context.
 * @param name The variable name.
 * @return True if the variable is wide.
 */
static bool is_wide_var(Codegen* cg, String* name) {
    assert(cg != NULL);
    assert(name != NULL);
    for (int i = 0; i < cg->wide_var_count; i++) {
        if (strcmp(string_cstr(cg->wide_vars[i]), string_cstr(name)) == 0) {
            return true;
        }
    }
    return false;
}

/**
 * Resolve checker-recorded binder types without interpreting transport width as a type.
 * @param cg Current codegen scopes.
 * @param name Identifier of the bound variable.
 * @return Resolved semantic type, or NULL for unchecked legacy ASTs.
 */
static Type* bound_pattern_type(Codegen* cg, String* name) {
    assert(cg != NULL);
    assert(name != NULL);
    for (int index = cg->bound_pattern_count - 1; index >= 0; index--) {
        Pattern* pattern = cg->bound_patterns[index];
        if (!string_equal(pattern->data.ident, name)) continue;
        Type* type = pattern->checked_type;
        for (int depth = 0; depth < MAX_WIDE_VARS && type && type->kind == TYPE_VAR; depth++) {
            if (!type->data.var.bound) break;
            type = type->data.var.bound;
        }
        return type;
    }
    return NULL;
}

/**
 * Choose storage for a checked binder; preserve fallback for parser-only codegen tests.
 * @param type Semantic type, possibly NULL.
 * @param fallback Prior storage convention for unchecked ASTs.
 * @return QBE width for the checked semantic type.
 */
static char pattern_width(Type* type, char fallback) {
    assert(fallback == 'w' || fallback == 'l' || fallback == 'd');
    if (!type || type->kind == TYPE_VAR) return fallback;
    assert(type->kind != TYPE_ERROR);
    if (type->kind == TYPE_FLOAT) return 'd';
    if (type->kind == TYPE_INT || type->kind == TYPE_STRING || type->kind == TYPE_CON ||
        type->kind == TYPE_TUPLE || type->kind == TYPE_FN) return 'l';
    return 'w';
}

/**
 * Read resolved expression metadata after checker unification, without source-name guesses.
 * @param expr AST node with an optional checker-owned type in the same live arena.
 * @return Resolved type, or NULL for parser-only expressions.
 */
static Type* checked_expr_type(Expr* expr) {
    assert(expr != NULL);
    assert(expr->loc.line >= 0);
    Type* type = expr->checked_type;
    for (int depth = 0; depth < MAX_WIDE_VARS && type && type->kind == TYPE_VAR; depth++) {
        if (!type->data.var.bound) break;
        type = type->data.var.bound;
    }
    return type;
}

/**
 * Bind a constructor payload using checker semantics, retaining pointer and Float bits.
 * @param cg Current context.
 * @param pattern Identifier binder with checker-owned semantic metadata.
 * @param value Full-width payload already loaded after its constructor tag check.
 * @param fallback Storage convention for unchecked legacy ASTs.
 */
static void emit_pattern_binding(Codegen* cg, Pattern* pattern, String* value, char fallback) {
    assert(cg != NULL && pattern != NULL);
    assert(pattern->type == PATTERN_IDENT && value != NULL);
    assert(cg->bound_pattern_count < MAX_WIDE_VARS);
    cg->bound_patterns[cg->bound_pattern_count++] = pattern;
    char width = pattern_width(bound_pattern_type(cg, pattern->data.ident), fallback);
    emit(cg, "    %%%s =%c %s %s\n", string_cstr(pattern->data.ident), width,
        width == 'd' ? "cast" : "copy", string_cstr(value));
    if (width == 'l') register_wide_var(cg, pattern->data.ident);
}

/**
 * Identify a lexically bound callable, including typed constructor payload binders.
 * @param cg Current code generation context.
 * @param name Source local name.
 * @return Whether the identifier requires an indirect call.
 */
static bool is_function_var(Codegen* cg, String* name) {
    assert(cg != NULL);
    assert(name != NULL);
    Type* bound = bound_pattern_type(cg, name);
    if (bound && bound->kind == TYPE_FN) return true;
    for (int i = cg->function_var_count - 1; i >= 0; i--) {
        if (string_equal(cg->function_vars[i], name)) return true;
    }
    return false;
}

/**
 * Check whether a callable identifier names a declared source function.
 * @param cg Context containing source declarations and lexical callable bindings.
 * @param name Identifier being emitted as a value.
 * @return True for a global function that is not shadowed by a local callable.
 */
static bool is_global_function(Codegen* cg, String* name) {
    assert(cg != NULL);
    assert(name != NULL);
    if (is_function_var(cg, name) || !cg->program_stmts) return false;
    for (size_t i = 0; i < cg->program_stmts->len; i++) {
        Stmt* stmt = cg->program_stmts->data[i];
        if (stmt->type == STMT_FN && string_equal(stmt->data.fn.name, name)) return true;
    }
    return false;
}

/**
 * Remember a raw callable binding within the current lexical scope.
 * @param cg Current code generation context.
 * @param name Source local name.
 */
static void register_function_var(Codegen* cg, String* name) {
    assert(cg != NULL && name != NULL);
    assert(cg->function_var_count < MAX_WIDE_VARS);
    cg->function_vars[cg->function_var_count++] = name;
}

/**
 * Bind checked tuple payload leaves after the surrounding Result tag succeeds.
 * @param cg Current code generation context.
 * @param pattern Irrefutable tuple, identifier or wildcard payload pattern.
 * @param value Full-width tuple address or scalar payload.
 * @param depth Bounded recursive tuple nesting.
 */
static void emit_payload_bindings(Codegen* cg, Pattern* pattern, String* value, int depth) {
    assert(cg != NULL && pattern != NULL && value != NULL);
    assert(depth < MAX_WIDE_VARS);
    if (pattern->type == PATTERN_IDENT) {
        emit_pattern_binding(cg, pattern, value, 'l');
    } else if (pattern->type == PATTERN_TUPLE) {
        PatternVec* fields = pattern->data.tuple;
        for (size_t i = 0; i < fields->len; i++) {
            String* address = fresh_temp(cg);
            String* field = fresh_temp(cg);
            emit(cg, "    %s =l add %s, %zu\n", string_cstr(address), string_cstr(value), i * 8);
            emit(cg, "    %s =l loadl %s\n", string_cstr(field), string_cstr(address));
            emit_payload_bindings(cg, fields->data[i], field, depth + 1);
        }
    }
}

static void clear_wide_vars(Codegen* cg) __attribute__((unused));
/**
 * Clear wide variable tracking (call at start of each function).
 * Currently unused but available for future per-function scope tracking.
 * @param cg The codegen context.
 */
static void clear_wide_vars(Codegen* cg) {
    /* FERN_STYLE: allow(assertion-density) - simple reset function */
    assert(cg != NULL);
    cg->wide_var_count = 0;
    cg->bound_pattern_count = 0;
}

/**
 * Check if a named variable is tracked as an owned pointer.
 * @param cg The codegen context.
 * @param name The variable name.
 * @return True if tracked as owned pointer.
 */
static bool is_owned_ptr_var(Codegen* cg, String* name) {
    assert(cg != NULL);
    assert(name != NULL);
    for (int i = 0; i < cg->owned_ptr_var_count; i++) {
        if (strcmp(string_cstr(cg->owned_ptr_vars[i]), string_cstr(name)) == 0) {
            return true;
        }
    }
    return false;
}

/**
 * Register a named variable as an owned pointer once.
 * @param cg The codegen context.
 * @param name The variable name.
 */
static void register_owned_ptr_var(Codegen* cg, String* name) {
    assert(cg != NULL);
    assert(name != NULL);
    assert(cg->owned_ptr_var_count < MAX_WIDE_VARS);
    if (is_owned_ptr_var(cg, name)) return;
    cg->owned_ptr_vars[cg->owned_ptr_var_count++] = name;
}

/**
 * Emit semantic drops for owned pointers, optionally preserving one name.
 * @param cg The codegen context.
 * @param preserve_name Variable name to preserve, or NULL.
 */
static void emit_owned_ptr_drops(Codegen* cg, const char* preserve_name) {
    assert(cg != NULL);
    assert(cg->owned_ptr_var_count >= 0);
    for (int i = cg->owned_ptr_var_count - 1; i >= 0; i--) {
        String* name = cg->owned_ptr_vars[i];
        assert(name != NULL);
        const char* raw_name = string_cstr(name);
        if (preserve_name != NULL && strcmp(raw_name, preserve_name) == 0) continue;
        emit(cg, "    call $fern_drop(l %%%s)\n", raw_name);
    }
}

/**
 * Find the owned pointer identifier preserved as the return value.
 * Supports constrained Step C shapes: direct identifier and block final identifier.
 * @param cg The codegen context.
 * @param expr The returned expression.
 * @return Preserved variable name, or NULL.
 */
static const char* preserved_owned_ptr_name(Codegen* cg, Expr* expr) {
    assert(cg != NULL);
    assert(expr != NULL);
    if (expr->type == EXPR_IDENT && is_owned_ptr_var(cg, expr->data.ident.name)) {
        return string_cstr(expr->data.ident.name);
    }
    if (expr->type == EXPR_BLOCK) {
        BlockExpr* block = &expr->data.block;
        if (block->final_expr != NULL) {
            return preserved_owned_ptr_name(cg, block->final_expr);
        }
    }
    return NULL;
}

/**
 * Register a function that returns a tuple (pointer type).
 * @param cg The codegen context.
 * @param name The function name.
 */
static void register_tuple_return_func(Codegen* cg, String* name) {
    assert(cg != NULL);
    assert(name != NULL);
    assert(cg->tuple_func_count < MAX_TUPLE_FUNCS);
    cg->tuple_return_funcs[cg->tuple_func_count++] = name;
}

/**
 * Check if a function returns a tuple (pointer type).
 * @param cg The codegen context.
 * @param name The function name.
 * @return True if the function returns a tuple.
 */
static bool is_tuple_return_func(Codegen* cg, const char* name) {
    assert(cg != NULL);
    assert(name != NULL);
    for (int i = 0; i < cg->tuple_func_count; i++) {
        if (strcmp(string_cstr(cg->tuple_return_funcs[i]), name) == 0) {
            return true;
        }
    }
    return false;
}

/**
 * Check if a function name returns a pointer type (string or list).
 * @param name The function name.
 * @return True if the function returns a pointer type.
 */
static bool fn_returns_pointer(const char* name) {
    /* FERN_STYLE: allow(assertion-density) - simple lookup */
    if (name == NULL) return false;
    
    /* String functions that return String */
    if (strcmp(name, "str_concat") == 0) return true;
    if (strcmp(name, "str_slice") == 0) return true;
    if (strcmp(name, "str_trim") == 0) return true;
    if (strcmp(name, "str_trim_start") == 0) return true;
    if (strcmp(name, "str_trim_end") == 0) return true;
    if (strcmp(name, "str_to_upper") == 0) return true;
    if (strcmp(name, "str_to_lower") == 0) return true;
    if (strcmp(name, "str_replace") == 0) return true;
    if (strcmp(name, "str_repeat") == 0) return true;
    if (strcmp(name, "str_join") == 0) return true;
    
    /* List functions that return List */
    if (strcmp(name, "list_push") == 0) return true;
    if (strcmp(name, "list_filter") == 0) return true;
    if (strcmp(name, "list_map") == 0) return true;
    if (strcmp(name, "list_reverse") == 0) return true;
    if (strcmp(name, "list_concat") == 0) return true;
    if (strcmp(name, "list_tail") == 0) return true;
    
    return false;
}

/* Print type for polymorphic print/println */
typedef enum {
    PRINT_INT,
    PRINT_STRING,
    PRINT_BOOL
} PrintType;

/**
 * Look up the declared semantic return type of a user function.
 * @param cg Code generator containing the program declarations.
 * @param name Function name at a direct call site.
 * @return String/Bool output kind, or integer output for other return types.
 */
static PrintType function_print_type(Codegen* cg, String* name) {
    assert(cg != NULL);
    assert(name != NULL);
    if (!cg->program_stmts) {
        return PRINT_INT;
    }
    for (size_t i = 0; i < cg->program_stmts->len; i++) {
        Stmt* stmt = cg->program_stmts->data[i];
        if (stmt->type != STMT_FN || !string_equal(stmt->data.fn.name, name)) {
            continue;
        }
        TypeExpr* type = stmt->data.fn.return_type;
        if (type && type->kind == TYPEEXPR_NAMED) {
            const char* type_name = string_cstr(type->data.named.name);
            if (strcmp(type_name, "String") == 0) {
                return PRINT_STRING;
            }
            if (strcmp(type_name, "Bool") == 0) {
                return PRINT_BOOL;
            }
        }
        return PRINT_INT;
    }
    return PRINT_INT;
}

/**
 * Determine the print type for an expression.
 * Used to select the correct runtime print function.
 * @param cg The codegen context.
 * @param expr The expression to check.
 * @return The print type (PRINT_INT, PRINT_STRING, or PRINT_BOOL).
 */
static PrintType get_print_type(Codegen* cg, Expr* expr) {
    // FERN_STYLE: allow(function-length) type dispatch for all expression print types
    assert(cg != NULL);
    assert(cg->arena != NULL);
    if (expr == NULL) return PRINT_INT;
    Type* semantic = checked_expr_type(expr);
    if (semantic && semantic->kind != TYPE_VAR) {
        if (semantic->kind == TYPE_STRING) return PRINT_STRING;
        if (semantic->kind == TYPE_BOOL) return PRINT_BOOL;
        return PRINT_INT;
    }
    switch (expr->type) {
        case EXPR_STRING_LIT:
        case EXPR_INTERP_STRING:
            return PRINT_STRING;
        
        case EXPR_BOOL_LIT:
            return PRINT_BOOL;
        
        case EXPR_INT_LIT:
        case EXPR_FLOAT_LIT:
            return PRINT_INT;
        
        case EXPR_IDENT: {
            Type* bound = bound_pattern_type(cg, expr->data.ident.name);
            if (bound && bound->kind != TYPE_VAR) {
                if (bound->kind == TYPE_STRING) return PRINT_STRING;
                if (bound->kind == TYPE_BOOL) return PRINT_BOOL;
                return PRINT_INT;
            }
            /* Unchecked legacy ASTs retain the existing fallback. */
            if (is_wide_var(cg, expr->data.ident.name)) {
                return PRINT_STRING;
            }
            return PRINT_INT;
        }
        case EXPR_DOT: {
            /* Field access - without type info, default to PRINT_INT
             * User can use type annotation or explicit conversion for strings */
            return PRINT_INT;
        }
        
        case EXPR_CALL: {
            CallExpr* call = &expr->data.call;
            /* Check for module.function calls that return strings */
            if (call->func->type == EXPR_DOT) {
                DotExpr* dot = &call->func->data.dot;
                /* Try to build module path for nested modules like Tui.Style */
                String* module_path = try_build_module_path(cg->arena, dot->object);
                if (module_path != NULL) {
                    const char* module = string_cstr(module_path);
                    const char* func = string_cstr(dot->field);
                    if (strcmp(module, "Regex") == 0 && strcmp(func, "is_match") == 0) {
                        return PRINT_BOOL;
                    }
                    /* String module functions that return String */
                    if (strcmp(module, "String") == 0) {
                        if (strcmp(func, "concat") == 0 ||
                            strcmp(func, "slice") == 0 ||
                            strcmp(func, "trim") == 0 ||
                            strcmp(func, "trim_start") == 0 ||
                            strcmp(func, "trim_end") == 0 ||
                            strcmp(func, "to_upper") == 0 ||
                            strcmp(func, "to_lower") == 0 ||
                            strcmp(func, "replace") == 0 ||
                            strcmp(func, "repeat") == 0) {
                            return PRINT_STRING;
                        }
                    }
                    if (strcmp(module, "Tui.Log") == 0 ||
                        (strcmp(module, "Tui.Tree") == 0 && strcmp(func, "render") == 0)) {
                        return PRINT_STRING;
                    }
                    /* Tui.Style module functions all return String */
                    if (strcmp(module, "Tui.Style") == 0) {
                        return PRINT_STRING;
                    }
                    /* Tui.Status module functions all return String */
                    if (strcmp(module, "Tui.Status") == 0) {
                        return PRINT_STRING;
                    }
                    /* Tui.Panel.render() returns String */
                    if (strcmp(module, "Tui.Panel") == 0 && strcmp(func, "render") == 0) {
                        return PRINT_STRING;
                    }
                    /* Tui.Table.render() returns String */
                    if (strcmp(module, "Tui.Table") == 0 && strcmp(func, "render") == 0) {
                        return PRINT_STRING;
                    }
                    /* Progress.render() returns String */
                    if (strcmp(module, "Progress") == 0 && strcmp(func, "render") == 0) {
                        return PRINT_STRING;
                    }
                    /* Spinner.render() returns String */
                    if (strcmp(module, "Spinner") == 0 && strcmp(func, "render") == 0) {
                        return PRINT_STRING;
                    }
                    /* Tui.Prompt.input() and Tui.Prompt.password() return String */
                    if (strcmp(module, "Tui.Prompt") == 0 && 
                        (strcmp(func, "input") == 0 || strcmp(func, "password") == 0)) {
                        return PRINT_STRING;
                    }
                }
            }
            if (call->func->type == EXPR_IDENT) {
                const char* fn_name = string_cstr(call->func->data.ident.name);
                /* String functions return strings */
                if (strncmp(fn_name, "str_", 4) == 0) {
                    return PRINT_STRING;
                }
                return function_print_type(cg, call->func->data.ident.name);
            }
            return PRINT_INT;
        }
        
        case EXPR_BINARY: {
            /* Binary expressions: check if string concatenation */
            BinaryExpr* bin = &expr->data.binary;
            if ((bin->op >= BINOP_EQ && bin->op <= BINOP_OR)) {
                return PRINT_BOOL;
            }
            if (bin->op == BINOP_PIPE && bin->right->type == EXPR_IDENT) {
                return function_print_type(cg, bin->right->data.ident.name);
            }
            if (bin->op == BINOP_PIPE && bin->right->type == EXPR_CALL) {
                return get_print_type(cg, bin->right);
            }
            if (bin->op == BINOP_ADD) {
                /* If either operand is a string, result is string */
                PrintType left_type = get_print_type(cg, bin->left);
                PrintType right_type = get_print_type(cg, bin->right);
                if (left_type == PRINT_STRING || right_type == PRINT_STRING) {
                    return PRINT_STRING;
                }
            }
            return PRINT_INT;
        }
        
        case EXPR_IF:
            return get_print_type(cg, expr->data.if_expr.then_branch);
        case EXPR_BLOCK: {
            BlockExpr* block = &expr->data.block;
            return get_print_type(cg, block->final_expr);
        }
        case EXPR_MATCH:
            if (expr->data.match_expr.arms && expr->data.match_expr.arms->len > 0) {
                return get_print_type(cg, expr->data.match_expr.arms->data[0].body);
            }
            return PRINT_INT;
        default:
            return PRINT_INT;
    }
}

/**
 * Get QBE type specifier for an expression.
 * Returns 'l' for Int and pointers, 'w' for Bool/Unit, and 'd' for Float.
 * @param cg The codegen context (for checking wide variables).
 * @param expr The expression to check.
 * @return 'l' for 64-bit, 'w' for 32-bit.
 */
static char qbe_type_for_expr(Codegen* cg, Expr* expr) {
    // FERN_STYLE: allow(function-length) type dispatch handles all module return types
    /* FERN_STYLE: allow(assertion-density) - simple type lookup */
    if (expr == NULL) return 'w';
    Type* semantic = checked_expr_type(expr);
    if (semantic && semantic->kind != TYPE_VAR) return pattern_width(semantic, 'w');
    
    switch (expr->type) {
        /* Pointer types (64-bit) */
        case EXPR_LIST:
        case EXPR_STRING_LIT:
        case EXPR_INTERP_STRING:
        case EXPR_TUPLE:
            return 'l';
        
        /* Word types (32-bit) */
        case EXPR_INT_LIT:
            return 'l';
        case EXPR_BOOL_LIT:
            return 'w';
        
        /* Identifiers: check if tracked as wide variable */
        case EXPR_IDENT:
            if (cg != NULL && bound_pattern_type(cg, expr->data.ident.name)) {
                return pattern_width(bound_pattern_type(cg, expr->data.ident.name), 'w');
            }
            if (cg != NULL && is_wide_var(cg, expr->data.ident.name)) {
                return 'l';
            }
            return 'w';
        
        /* Check if function call returns pointer type */
        case EXPR_CALL: {
            CallExpr* call = &expr->data.call;
            /* Check direct function calls (user-defined functions) */
            if (call->func->type == EXPR_IDENT) {
                const char* func_name = string_cstr(call->func->data.ident.name);
                if (cg != NULL && is_tuple_return_func(cg, func_name)) {
                    return 'l';
                }
            }
            /* Check module.function calls */
            if (call->func->type == EXPR_DOT) {
                DotExpr* dot = &call->func->data.dot;
                /* Try to build module path for nested modules like Tui.Style */
                String* module_path = try_build_module_path(cg ? cg->arena : NULL, dot->object);
                if (module_path != NULL) {
                    const char* module = string_cstr(module_path);
                    const char* func = string_cstr(dot->field);
                    /* String module functions returning String or pointer types */
                    if (strcmp(module, "String") == 0) {
                        if (strcmp(func, "concat") == 0 ||
                            strcmp(func, "slice") == 0 ||
                            strcmp(func, "trim") == 0 ||
                            strcmp(func, "trim_start") == 0 ||
                            strcmp(func, "trim_end") == 0 ||
                            strcmp(func, "to_upper") == 0 ||
                            strcmp(func, "to_lower") == 0 ||
                            strcmp(func, "replace") == 0 ||
                            strcmp(func, "repeat") == 0 ||
                            strcmp(func, "split") == 0 ||
                            strcmp(func, "lines") == 0 ||
                            strcmp(func, "join") == 0 ||
                            strcmp(func, "index_of") == 0 ||
                            strcmp(func, "char_at") == 0) {
                            return 'l';
                        }
                    }
                    /* List module functions returning List (pointer) */
                    if (strcmp(module, "List") == 0) {
                        if (strcmp(func, "push") == 0 ||
                            strcmp(func, "reverse") == 0 ||
                            strcmp(func, "concat") == 0 ||
                            strcmp(func, "tail") == 0) {
                            return 'l';
                        }
                    }
                    /* List module functions returning pointers */
                    if (strcmp(module, "List") == 0) {
                        /* Functions returning List (pointer) */
                        if (strcmp(func, "new") == 0 ||
                            strcmp(func, "push") == 0 ||
                            strcmp(func, "concat") == 0 ||
                            strcmp(func, "reverse") == 0 ||
                            strcmp(func, "tail") == 0 ||
                            strcmp(func, "filter") == 0 ||
                            strcmp(func, "map") == 0) {
                            return 'l';
                        }
                        /* Functions returning element (pointer for string lists) */
                        if (strcmp(func, "head") == 0 ||
                            strcmp(func, "get") == 0) {
                            return 'l';  /* List elements are pointers (strings, etc.) */
                        }
                    }
                    /* File module functions returning Result or List (pointer) */
                    if (strcmp(module, "File") == 0) {
                        if (strcmp(func, "read") == 0 ||
                            strcmp(func, "write") == 0 ||
                            strcmp(func, "append") == 0 ||
                            strcmp(func, "delete") == 0 ||
                            strcmp(func, "size") == 0 ||
                            strcmp(func, "list_dir") == 0) {
                            return 'l';
                        }
                    }
                    /* System module functions returning pointers */
                    if (strcmp(module, "System") == 0) {
                        if (strcmp(func, "args") == 0 ||
                            strcmp(func, "arg") == 0 ||
                            strcmp(func, "exec") == 0 ||
                            strcmp(func, "exec_args") == 0 ||
                            strcmp(func, "getenv") == 0 ||
                            strcmp(func, "cwd") == 0 ||
                            strcmp(func, "hostname") == 0 ||
                            strcmp(func, "user") == 0 ||
                            strcmp(func, "home") == 0) {
                            return 'l';
                        }
                    }
                    /* Regex module functions returning pointers */
                    if (strcmp(module, "Regex") == 0) {
                        if (strcmp(func, "find") == 0 ||
                            strcmp(func, "find_all") == 0 ||
                            strcmp(func, "replace") == 0 ||
                            strcmp(func, "replace_all") == 0 ||
                            strcmp(func, "split") == 0 ||
                            strcmp(func, "captures") == 0) {
                            return 'l';
                        }
                    }
                    /* Tui.Term module functions returning pointers */
                    if (strcmp(module, "Tui.Term") == 0) {
                        if (strcmp(func, "size") == 0) {
                            return 'l';
                        }
                    }
                    if (strcmp(module, "Tui.Tree") == 0 || strcmp(module, "Tui.Log") == 0) {
                        return 'l';
                    }
                    /* Tui.Style module functions all return String (pointer) */
                    if (strcmp(module, "Tui.Style") == 0) {
                        return 'l';
                    }
                    /* Tui.Status module functions all return String (pointer) */
                    if (strcmp(module, "Tui.Status") == 0) {
                        return 'l';
                    }
                    /* Tui.Panel module functions return Panel or String (pointers) */
                    if (strcmp(module, "Tui.Panel") == 0) {
                        return 'l';
                    }
                    /* Tui.Table module functions return Table or String (pointers) */
                    if (strcmp(module, "Tui.Table") == 0) {
                        return 'l';
                    }
                    /* Tui.Live module functions return Unit (w) */
                    if (strcmp(module, "Tui.Live") == 0) {
                        return 'w';
                    }
                    /* Progress module functions return Progress or String (pointers) */
                    if (strcmp(module, "Progress") == 0) {
                        return 'l';
                    }
                    /* Spinner module functions return Spinner or String (pointers) */
                    if (strcmp(module, "Spinner") == 0) {
                        return 'l';
                    }
                    /* Tui.Prompt module: input/password return String (l), confirm returns Bool (w), select/int return Int (w) */
                    if (strcmp(module, "Tui.Prompt") == 0) {
                        if (strcmp(func, "input") == 0 || strcmp(func, "password") == 0) {
                            return 'l';
                        }
                        return 'w';
                    }
                }
            }
            if (call->func->type == EXPR_IDENT) {
                const char* fn_name = string_cstr(call->func->data.ident.name);
                if (fn_returns_pointer(fn_name)) {
                    return 'l';
                }
            }
            return 'w';
        }
        
        /* Binary expressions: check if string concatenation or pipe */
        case EXPR_BINARY: {
            BinaryExpr* bin = &expr->data.binary;
            if (bin->op == BINOP_ADD) {
                /* Check if either operand is actually a string type
                 * (not just 64-bit - tuple field access returns 'l' but may be Int) */
                PrintType left_pt = get_print_type(cg, bin->left);
                PrintType right_pt = get_print_type(cg, bin->right);
                if (left_pt == PRINT_STRING || right_pt == PRINT_STRING) {
                    return 'l';  /* String concatenation returns pointer */
                }
            }
            /* Pipe expression: check the function being called to determine return type */
            if (bin->op == BINOP_PIPE) {
                /* The right side is a call expression - check what it returns */
                if (bin->right->type == EXPR_CALL) {
                    CallExpr* call = &bin->right->data.call;
                    if (call->func->type == EXPR_DOT) {
                        DotExpr* dot = &call->func->data.dot;
                        String* module_path = try_build_module_path(cg->arena, dot->object);
                        if (module_path != NULL) {
                            const char* module = string_cstr(module_path);
                            if (strcmp(module, "Tui.Tree") == 0 || strcmp(module, "Tui.Log") == 0) {
                                return 'l';
                            }
                            /* Tui.Panel functions return Panel (pointer) */
                            if (strcmp(module, "Tui.Panel") == 0) {
                                return 'l';
                            }
                            /* Tui.Table functions return Table (pointer) */
                            if (strcmp(module, "Tui.Table") == 0) {
                                return 'l';
                            }
                        }
                    }
                }
            }
            return bin->op >= BINOP_EQ && bin->op <= BINOP_OR ? 'w' : 'l';
        }
        
        /* Block expressions: check the final expression's type */
        case EXPR_BLOCK:
            if (expr->data.block.final_expr) {
                return qbe_type_for_expr(cg, expr->data.block.final_expr);
            }
            return 'w';
        
        /* If expressions: check the then branch's type */
        case EXPR_IF:
            if (expr->data.if_expr.then_branch) {
                return qbe_type_for_expr(cg, expr->data.if_expr.then_branch);
            }
            return 'w';
        
        /* Match expressions: check the first arm's body type */
        case EXPR_MATCH:
            if (expr->data.match_expr.arms && expr->data.match_expr.arms->len > 0) {
                return qbe_type_for_expr(cg, expr->data.match_expr.arms->data[0].body);
            }
            return 'w';

        /* With expressions: result type matches do/else body type */
        case EXPR_WITH:
            if (expr->data.with_expr.body) {
                return qbe_type_for_expr(cg, expr->data.with_expr.body);
            }
            return 'w';
        
        /* Dot expressions: tuple field access loads 64-bit values */
        case EXPR_DOT: {
            DotExpr* dot = &expr->data.dot;
            const char* field = string_cstr(dot->field);
            /* Numeric field access (tuple.0, tuple.1, etc.) always loads as 64-bit */
            if (field[0] >= '0' && field[0] <= '9') {
                return 'l';
            }
            return 'w';
        }
        
        /* For other exprs, we'd need type info - default to word */
        /* TODO: track types through codegen for proper handling */
        case EXPR_UNARY:
        default:
            return 'w';
    }
}

/* ========== Codegen Creation ========== */

/**
 * Create a new code generator.
 * @param arena The arena for allocations.
 * @return The new codegen context.
 */
Codegen* codegen_new(Arena* arena) {
    assert(arena != NULL);
    Codegen* cg = arena_alloc(arena, sizeof(Codegen));
    assert(cg != NULL);
    cg->arena = arena;
    cg->program_stmts = NULL;
    cg->output = string_new(arena, "");
    cg->data_section = string_new(arena, "");
    cg->temp_counter = 0;
    cg->label_counter = 0;
    cg->string_counter = 0;
    cg->defer_count = 0;
    cg->wide_var_count = 0;
    cg->bound_pattern_count = 0;
    cg->owned_ptr_var_count = 0;
    cg->function_var_count = 0;
    cg->tuple_func_count = 0;
    cg->returned = false;
    return cg;
}

/**
 * Push a deferred expression onto the stack.
 * @param cg The codegen context.
 * @param expr The expression to defer.
 */
static void push_defer(Codegen* cg, Expr* expr) {
    assert(cg != NULL);
    assert(expr != NULL);
    assert(cg->defer_count < MAX_DEFERS);
    cg->defer_stack[cg->defer_count++] = expr;
}

/**
 * Emit all deferred expressions in reverse order (LIFO).
 * @param cg The codegen context.
 */
static void emit_defers(Codegen* cg) {
    assert(cg != NULL);
    assert(cg->defer_count >= 0 && cg->defer_count <= MAX_DEFERS);
    /* Emit in reverse order: last defer runs first */
    for (int i = cg->defer_count - 1; i >= 0; i--) {
        assert(cg->defer_stack[i] != NULL);
        codegen_expr(cg, cg->defer_stack[i]);
    }
}

/**
 * Clear the defer stack (called at function boundaries).
 * @param cg The codegen context.
 */
static void clear_defers(Codegen* cg) {
    assert(cg != NULL);
    assert(cg->defer_count >= 0);
    cg->defer_count = 0;
}

/**
 * Emit to data section.
 * @param cg The codegen context.
 * @param fmt The format string.
 * @param ... The format arguments.
 */
static void emit_data(Codegen* cg, const char* fmt, ...) {
    assert(cg != NULL);
    assert(fmt != NULL);
    char buf[1024];
    va_list args;
    va_start(args, fmt);
    vsnprintf(buf, sizeof(buf), fmt, args);
    va_end(args);
    
    cg->data_section = string_concat(cg->arena, cg->data_section, string_new(cg->arena, buf));
}

/**
 * Emit decoded string bytes without exposing escapes to QBE or the assembler.
 * @param cg The codegen context.
 * @param label The data symbol for the string.
 * @param value The decoded, length-tracked string value.
 */
static void emit_string_data(Codegen* cg, String* label, String* value) {
    assert(cg != NULL);
    assert(label != NULL);
    assert(value != NULL);
    const unsigned char* bytes = (const unsigned char*)string_cstr(value);
    size_t length = string_len(value);
    emit_data(cg, "data %s = { ", string_cstr(label));
    for (size_t i = 0; i < length;) {
        size_t start = i;
        /* Bound text runs below emit_data's formatting buffer size. */
        while (i < length && i - start < 256 && bytes[i] >= 32 && bytes[i] <= 126 &&
               bytes[i] != '"' && bytes[i] != '\\') {
            i++;
        }
        if (i > start) {
            emit_data(cg, "b \"%.*s\", ", (int)(i - start), bytes + start);
        } else {
            /* Numeric bytes also preserve Unicode and embedded control characters. */
            emit_data(cg, "b %u, ", (unsigned)bytes[i]);
            i++;
        }
    }
    emit_data(cg, "b 0 }\n");
}

/**
 * Generate a fresh string label.
 * @param cg The codegen context.
 * @return The new string label.
 */
static String* fresh_string_label(Codegen* cg) {
    assert(cg != NULL);
    assert(cg->string_counter >= 0);
    char buf[32];
    snprintf(buf, sizeof(buf), "$str%d", cg->string_counter++);
    return string_new(cg->arena, buf);
}

/* ========== Expression Code Generation ========== */

/**
 * Evaluate a Result payload once and pass all 64 bits to the native constructor.
 * @param cg Codegen context.
 * @param expr Checked payload, or parser-only test expression.
 * @return A long QBE value preserving pointer, signed integer, or Float representation.
 */
static String* codegen_result_payload(Codegen* cg, Expr* expr) {
    assert(cg != NULL);
    assert(expr != NULL);
    String* value = codegen_expr(cg, expr);
    char width = qbe_type_for_expr(cg, expr);
    if (width == 'l') return value;
    String* payload = fresh_temp(cg);
    emit(cg, "    %s =l %s %s\n", string_cstr(payload), width == 'd' ? "cast" : "extsw", string_cstr(value));
    return payload;
}

/**
 * Wrap the sole signed division overflow instead of executing a target-dependent trap.
 * @param cg Current emitter and label allocator.
 * @param result Destination for the selected64-bit result.
 * @param left Already evaluated numerator, never evaluated again.
 * @param right Already evaluated denominator, never evaluated again.
 * @param remainder True for remainder, false for quotient.
 */
static void codegen_int_division(Codegen* cg, String* result, String* left,
                                 String* right, bool remainder) {
    assert(cg != NULL && result != NULL);
    assert(left != NULL && right != NULL);
    String* minimum = fresh_temp(cg);
    String* negative_one = fresh_temp(cg);
    String* overflow = fresh_temp(cg);
    String* wrapped = fresh_label(cg);
    String* normal = fresh_label(cg);
    String* done = fresh_label(cg);
    String* quotient = fresh_temp(cg);
    emit(cg, "    %s =w ceql %s, -9223372036854775808\n",
        string_cstr(minimum), string_cstr(left));
    emit(cg, "    %s =w ceql %s, -1\n", string_cstr(negative_one), string_cstr(right));
    emit(cg, "    %s =w and %s, %s\n", string_cstr(overflow),
        string_cstr(minimum), string_cstr(negative_one));
    emit(cg, "    jnz %s, %s, %s\n", string_cstr(overflow),
        string_cstr(wrapped), string_cstr(normal));
    emit(cg, "%s\n    jmp %s\n", string_cstr(wrapped), string_cstr(done));
    emit(cg, "%s\n    %s =l %s %s, %s\n    jmp %s\n", string_cstr(normal),
        string_cstr(quotient), remainder ? "rem" : "div", string_cstr(left),
        string_cstr(right), string_cstr(done));
    emit(cg, "%s\n    %s =l phi %s %s, %s %s\n", string_cstr(done), string_cstr(result),
        string_cstr(wrapped), remainder ? "0" : "-9223372036854775808",
        string_cstr(normal), string_cstr(quotient));
}

/**
 * Generate QBE IR code for an expression.
 * @param cg The codegen context.
 * @param expr The expression to compile.
 * @return The temporary holding the result.
 */
String* codegen_expr(Codegen* cg, Expr* expr) {
    // FERN_STYLE: allow(function-length) code generation handles all expression types in one switch
    assert(cg != NULL);
    assert(expr != NULL);
    
    switch (expr->type) {
        case EXPR_INT_LIT: {
            String* tmp = fresh_temp(cg);
            emit(cg, "    %s =l copy %lld\n", string_cstr(tmp), expr->data.int_lit.value);
            return tmp;
        }
        
        case EXPR_FLOAT_LIT: {
            String* tmp = fresh_temp(cg);
            emit(cg, "    %s =d copy d_%g\n", string_cstr(tmp), expr->data.float_lit.value);
            return tmp;
        }
        
        case EXPR_BOOL_LIT: {
            String* tmp = fresh_temp(cg);
            emit(cg, "    %s =w copy %d\n", string_cstr(tmp), expr->data.bool_lit.value ? 1 : 0);
            return tmp;
        }
        
        case EXPR_STRING_LIT: {
            /* Create a data section entry for the string */
            String* label = fresh_string_label(cg);
            String* tmp = fresh_temp(cg);
            
            emit_string_data(cg, label, expr->data.string_lit.value);
            
            /* Load pointer to string */
            emit(cg, "    %s =l copy %s\n", string_cstr(tmp), string_cstr(label));
            return tmp;
        }
        
        case EXPR_INTERP_STRING: {
            /* String interpolation: "Hello, {name}!" 
             * Parts is a vector alternating string literals and expressions.
             * We convert each part to a string and concatenate them all.
             */
            InterpStringExpr* interp = &expr->data.interp_string;
            assert(interp->parts != NULL);
            
            if (interp->parts->len == 0) {
                /* Empty interpolated string - return empty string */
                String* label = fresh_string_label(cg);
                String* tmp = fresh_temp(cg);
                emit_data(cg, "data %s = { b \"\", b 0 }\n", string_cstr(label));
                emit(cg, "    %s =l copy %s\n", string_cstr(tmp), string_cstr(label));
                return tmp;
            }
            
            /* Process first part */
            Expr* first = interp->parts->data[0];
            String* result;
            
            if (first->type == EXPR_STRING_LIT) {
                result = codegen_expr(cg, first);
            } else {
                /* Convert expression to string based on its type */
                String* val = codegen_expr(cg, first);
                PrintType pt = get_print_type(cg, first);
                char val_type = qbe_type_for_expr(cg, first);
                result = fresh_temp(cg);
                switch (pt) {
                    case PRINT_STRING:
                        emit(cg, "    %s =l copy %s\n", string_cstr(result), string_cstr(val));
                        break;
                    case PRINT_BOOL:
                        emit(cg, "    %s =l call $fern_bool_to_str(%c %s)\n", 
                            string_cstr(result), val_type, string_cstr(val));
                        break;
                    case PRINT_INT:
                    default:
                        emit(cg, "    %s =l call $fern_int_to_str(%c %s)\n", 
                            string_cstr(result), val_type, string_cstr(val));
                        break;
                }
            }
            
            /* Concatenate remaining parts */
            for (size_t i = 1; i < interp->parts->len; i++) {
                Expr* part = interp->parts->data[i];
                String* part_str;
                
                if (part->type == EXPR_STRING_LIT) {
                    part_str = codegen_expr(cg, part);
                } else {
                    /* Convert expression to string */
                    String* val = codegen_expr(cg, part);
                    PrintType pt = get_print_type(cg, part);
                    char val_type = qbe_type_for_expr(cg, part);
                    part_str = fresh_temp(cg);
                    switch (pt) {
                        case PRINT_STRING:
                            emit(cg, "    %s =l copy %s\n", string_cstr(part_str), string_cstr(val));
                            break;
                        case PRINT_BOOL:
                            emit(cg, "    %s =l call $fern_bool_to_str(%c %s)\n", 
                                string_cstr(part_str), val_type, string_cstr(val));
                            break;
                        case PRINT_INT:
                        default:
                            emit(cg, "    %s =l call $fern_int_to_str(%c %s)\n", 
                                string_cstr(part_str), val_type, string_cstr(val));
                            break;
                    }
                }
                
                /* Concatenate result with this part */
                String* new_result = fresh_temp(cg);
                emit(cg, "    %s =l call $fern_str_concat(l %s, l %s)\n",
                    string_cstr(new_result), string_cstr(result), string_cstr(part_str));
                result = new_result;
            }
            
            return result;
        }
        
        case EXPR_BINARY: {
            BinaryExpr* bin = &expr->data.binary;
            String* tmp = fresh_temp(cg);
            
            /* Handle pipe operator: left |> right(args) => right(left, args) */
            if (bin->op == BINOP_PIPE) {
                /* Right side must be a call expression (checker verified this) */
                assert(bin->right->type == EXPR_CALL);
                CallExpr* call = &bin->right->data.call;
                
                /* Generate code for the piped value first */
                String* piped_val = codegen_expr(cg, bin->left);
                
                /* Create a modified call with the piped value as first argument */
                /* We build the argument list manually: piped_val, then original args */
                
                /* Check for module.function calls (same as EXPR_CALL handling) */
                if (call->func->type == EXPR_DOT) {
                    DotExpr* dot = &call->func->data.dot;
                    String* module_path = try_build_module_path(cg->arena, dot->object);
                    if (module_path != NULL && is_builtin_module(string_cstr(module_path))) {
                        const char* module = string_cstr(module_path);
                        const char* func = string_cstr(dot->field);
                        
                        /* Handle Tui.Panel builder pattern */
                        if (strcmp(module, "Tui.Panel") == 0) {
                            /* All Tui.Panel functions take panel as first arg and return panel */
                            if (strcmp(func, "title") == 0 && call->args->len == 1) {
                                String* title = codegen_expr(cg, call->args->data[0].value);
                                emit(cg, "    %s =l call $fern_panel_title(l %s, l %s)\n",
                                    string_cstr(tmp), string_cstr(piped_val), string_cstr(title));
                                register_wide_var(cg, tmp);
                                return tmp;
                            }
                            if (strcmp(func, "subtitle") == 0 && call->args->len == 1) {
                                String* subtitle = codegen_expr(cg, call->args->data[0].value);
                                emit(cg, "    %s =l call $fern_panel_subtitle(l %s, l %s)\n",
                                    string_cstr(tmp), string_cstr(piped_val), string_cstr(subtitle));
                                register_wide_var(cg, tmp);
                                return tmp;
                            }
                            if (strcmp(func, "border") == 0 && call->args->len == 1) {
                                String* style = codegen_expr(cg, call->args->data[0].value);
                                emit(cg, "    %s =l call $fern_panel_border_str(l %s, l %s)\n",
                                    string_cstr(tmp), string_cstr(piped_val), string_cstr(style));
                                register_wide_var(cg, tmp);
                                return tmp;
                            }
                            if (strcmp(func, "border_color") == 0 && call->args->len == 1) {
                                String* color = codegen_expr(cg, call->args->data[0].value);
                                emit(cg, "    %s =l call $fern_panel_border_color(l %s, l %s)\n",
                                    string_cstr(tmp), string_cstr(piped_val), string_cstr(color));
                                register_wide_var(cg, tmp);
                                return tmp;
                            }
                            if (strcmp(func, "padding") == 0 && call->args->len == 1) {
                                String* pad = codegen_expr(cg, call->args->data[0].value);
                                emit(cg, "    %s =l call $fern_panel_padding(l %s, l %s)\n",
                                    string_cstr(tmp), string_cstr(piped_val), string_cstr(pad));
                                register_wide_var(cg, tmp);
                                return tmp;
                            }
                            if (strcmp(func, "width") == 0 && call->args->len == 1) {
                                String* w = codegen_expr(cg, call->args->data[0].value);
                                emit(cg, "    %s =l call $fern_panel_width(l %s, l %s)\n",
                                    string_cstr(tmp), string_cstr(piped_val), string_cstr(w));
                                register_wide_var(cg, tmp);
                                return tmp;
                            }
                            if (strcmp(func, "render") == 0 && call->args->len == 0) {
                                emit(cg, "    %s =l call $fern_panel_render(l %s)\n",
                                    string_cstr(tmp), string_cstr(piped_val));
                                register_wide_var(cg, tmp);
                                return tmp;
                            }
                        }
                        
                        /* Handle Tui.Table builder pattern */
                        if (strcmp(module, "Tui.Table") == 0) {
                            if (strcmp(func, "add_row") == 0 && call->args->len == 1) {
                                String* row = codegen_expr(cg, call->args->data[0].value);
                                emit(cg, "    %s =l call $fern_table_add_row(l %s, l %s)\n",
                                    string_cstr(tmp), string_cstr(piped_val), string_cstr(row));
                                register_wide_var(cg, tmp);
                                return tmp;
                            }
                            if (strcmp(func, "add_column") == 0 && call->args->len == 1) {
                                String* header = codegen_expr(cg, call->args->data[0].value);
                                emit(cg, "    %s =l call $fern_table_add_column(l %s, l %s)\n",
                                    string_cstr(tmp), string_cstr(piped_val), string_cstr(header));
                                register_wide_var(cg, tmp);
                                return tmp;
                            }
                            if (strcmp(func, "border") == 0 && call->args->len == 1) {
                                String* style = codegen_expr(cg, call->args->data[0].value);
                                emit(cg, "    %s =l call $fern_table_border(l %s, l %s)\n",
                                    string_cstr(tmp), string_cstr(piped_val), string_cstr(style));
                                register_wide_var(cg, tmp);
                                return tmp;
                            }
                            if (strcmp(func, "render") == 0 && call->args->len == 0) {
                                emit(cg, "    %s =l call $fern_table_render(l %s)\n",
                                    string_cstr(tmp), string_cstr(piped_val));
                                register_wide_var(cg, tmp);
                                return tmp;
                            }
                        }
                    }
                }
                
                /* Generic pipe fallback:
                 * left |> f(a, b) => f(left, a, b)
                 * Also supports method-like lowering for unknown dot targets:
                 * left |> obj.method(a) => method(left, a)
                 */
                char piped_type = qbe_type_for_expr(cg, bin->left);
                if (call->func->type == EXPR_IDENT || call->func->type == EXPR_DOT) {
                    const char* target_name = NULL;
                    char ret_type = qbe_type_for_expr(cg, expr);

                    if (call->func->type == EXPR_IDENT) {
                        target_name = string_cstr(call->func->data.ident.name);
                        if (is_tuple_return_func(cg, target_name) || fn_returns_pointer(target_name)) {
                            ret_type = 'l';
                        }
                    } else {
                        target_name = string_cstr(call->func->data.dot.field);
                    }

                    if (ret_type == 'l') {
                        register_wide_var(cg, tmp);
                    }

                    String** arguments = arena_alloc(cg->arena,
                        sizeof(String*) * (call->args->len + 1));
                    for (size_t i = 0; i < call->args->len; i++) {
                        arguments[i] = codegen_expr(cg, call->args->data[i].value);
                    }
                    emit(cg, "    %s =%c call $%s(",
                        string_cstr(tmp), ret_type, target_name);
                    emit(cg, "%c %s", piped_type, string_cstr(piped_val));
                    for (size_t i = 0; i < call->args->len; i++) {
                        char arg_type = qbe_type_for_expr(cg, call->args->data[i].value);
                        emit(cg, ", %c %s", arg_type, string_cstr(arguments[i]));
                    }
                    emit(cg, ")\n");
                    return tmp;
                }

                emit(cg, "    # unsupported pipe target\n");
                emit(cg, "    %s =w copy 0\n", string_cstr(tmp));
                return tmp;
            }
            
            /* Handle 'in' operator: elem in list -> List.contains(list, elem) */
            if (bin->op == BINOP_IN) {
                String* elem = codegen_expr(cg, bin->left);
                String* list = codegen_expr(cg, bin->right);
                /* Use string version for string elements */
                PrintType elem_pt = get_print_type(cg, bin->left);
                if (elem_pt == PRINT_STRING) {
                    emit(cg, "    %s =l call $fern_list_contains_str(l %s, l %s)\n",
                        string_cstr(tmp), string_cstr(list), string_cstr(elem));
                } else {
                    char elem_type = qbe_type_for_expr(cg, bin->left);
                    emit(cg, "    %s =l call $fern_list_contains(l %s, %c %s)\n",
                        string_cstr(tmp), string_cstr(list), elem_type, string_cstr(elem));
                }
                return tmp;
            }
            
            /* Check if this is string concatenation (+ with string operands)
             * Use get_print_type to check for actual string types, not just 64-bit values
             * (tuple field access returns 'l' but may not be a string) */
            if (bin->op == BINOP_ADD) {
                PrintType left_pt = get_print_type(cg, bin->left);
                PrintType right_pt = get_print_type(cg, bin->right);
                /* If either operand is a string, use string concat */
                if (left_pt == PRINT_STRING || right_pt == PRINT_STRING) {
                    String* left = codegen_expr(cg, bin->left);
                    String* right = codegen_expr(cg, bin->right);
                    emit(cg, "    %s =l call $fern_str_concat(l %s, l %s)\n",
                        string_cstr(tmp), string_cstr(left), string_cstr(right));
                    return tmp;
                }
            }
            
            /* Arithmetic/comparison operations */
            String* left = codegen_expr(cg, bin->left);
            String* right = codegen_expr(cg, bin->right);
            
            if ((bin->op == BINOP_DIV || bin->op == BINOP_MOD) &&
                qbe_type_for_expr(cg, bin->left) == 'l') {
                codegen_int_division(cg, tmp, left, right, bin->op == BINOP_MOD);
                return tmp;
            }
            const char* op = NULL;
            switch (bin->op) {
                case BINOP_ADD: op = "add"; break;
                case BINOP_SUB: op = "sub"; break;
                case BINOP_MUL: op = "mul"; break;
                case BINOP_DIV: op = "div"; break;
                case BINOP_MOD: op = "rem"; break;
                case BINOP_EQ:  op = "ceq"; break;
                case BINOP_NE:  op = "cne"; break;
                case BINOP_LT:  op = "cslt"; break;
                case BINOP_LE:  op = "csle"; break;
                case BINOP_GT:  op = "csgt"; break;
                case BINOP_GE:  op = "csge"; break;
                case BINOP_AND: op = "and"; break;  /* Bitwise/logical AND (eager) */
                case BINOP_OR:  op = "or"; break;   /* Bitwise/logical OR (eager) */
                default:
                    /* TODO: Handle other operators */
                    emit(cg, "    # TODO: unhandled binary op %d\n", bin->op);
                    return tmp;
            }
            
            bool comparison = bin->op >= BINOP_EQ && bin->op <= BINOP_GE;
            char result_width = qbe_type_for_expr(cg, expr);
            char comparison_op[8];
            if (comparison) {
                char operand_width = qbe_type_for_expr(cg, bin->left);
                if (operand_width == 'd' && op[1] == 's') {
                    snprintf(comparison_op, sizeof(comparison_op), "c%sd", op + 2);
                } else {
                    snprintf(comparison_op, sizeof(comparison_op), "%s%c", op, operand_width);
                }
                op = comparison_op;
                result_width = 'w';
            }
            emit(cg, "    %s =%c %s %s, %s\n",
                string_cstr(tmp), result_width, op, string_cstr(left), string_cstr(right));
            return tmp;
        }
        
        case EXPR_UNARY: {
            UnaryExpr* unary = &expr->data.unary;
            String* operand = codegen_expr(cg, unary->operand);
            String* tmp = fresh_temp(cg);
            
            switch (unary->op) {
                case UNOP_NEG:
                    emit(cg, "    %s =l sub 0, %s\n", string_cstr(tmp), string_cstr(operand));
                    break;
                case UNOP_NOT:
                    emit(cg, "    %s =w ceqw %s, 0\n", string_cstr(tmp), string_cstr(operand));
                    break;
                default:
                    emit(cg, "    # TODO: unhandled unary op %d\n", unary->op);
            }
            return tmp;
        }
        
        case EXPR_IDENT: {
            /* Variable reference - return the variable name as a QBE temporary */
            String* tmp = fresh_temp(cg);
            /* Check if variable is a wide type (pointer) */
            char type_spec = qbe_type_for_expr(cg, expr);
            Type* semantic = checked_expr_type(expr);
            bool global = semantic && semantic->kind == TYPE_FN &&
                is_global_function(cg, expr->data.ident.name);
            emit(cg, "    %s =%c copy %c%s\n", string_cstr(tmp), type_spec,
                global ? '$' : '%', string_cstr(expr->data.ident.name));
            return tmp;
        }
        
        case EXPR_BLOCK: {
            BlockExpr* block = &expr->data.block;
            int function_scope = cg->function_var_count;
            String* last = NULL;
            
            /* Generate code for each statement, stopping if we hit a return */
            for (size_t i = 0; i < block->stmts->len; i++) {
                codegen_stmt(cg, block->stmts->data[i]);
                if (cg->returned) {
                    /* A return was hit - don't generate more unreachable code */
                    last = fresh_temp(cg);  /* Dummy value, won't be used */
                    cg->function_var_count = function_scope;
                    return last;
                }
            }
            
            /* Generate code for the final expression if present */
            if (block->final_expr) {
                last = codegen_expr(cg, block->final_expr);
            } else {
                /* Unit value */
                last = fresh_temp(cg);
                emit(cg, "    %s =w copy 0\n", string_cstr(last));
            }
            
            cg->function_var_count = function_scope;
            return last;
        }
        
        case EXPR_IF: {
            IfExpr* if_expr = &expr->data.if_expr;
            String* cond = codegen_expr(cg, if_expr->condition);
            String* then_label = fresh_label(cg);
            String* else_label = fresh_label(cg);
            String* end_label = fresh_label(cg);
            String* result = fresh_temp(cg);
            bool then_returned = false;
            bool else_returned = false;
            
            emit(cg, "    jnz %s, %s, %s\n", 
                string_cstr(cond), string_cstr(then_label), string_cstr(else_label));
            
            /* Then branch */
            emit_label(cg, string_cstr(then_label));
            String* then_val = codegen_expr(cg, if_expr->then_branch);
            then_returned = cg->returned;
            if (!then_returned) {
                char then_type = qbe_type_for_expr(cg, if_expr->then_branch);
                emit(cg, "    %s =%c copy %s\n", string_cstr(result), then_type, string_cstr(then_val));
                if (then_type == 'l') {
                    register_wide_var(cg, result);
                }
                emit(cg, "    jmp %s\n", string_cstr(end_label));
            }
            
            /* Else branch */
            emit_label(cg, string_cstr(else_label));
            if (if_expr->else_branch) {
                String* else_val = codegen_expr(cg, if_expr->else_branch);
                else_returned = cg->returned;
                if (!else_returned) {
                    char else_type = qbe_type_for_expr(cg, if_expr->else_branch);
                    emit(cg, "    %s =%c copy %s\n", string_cstr(result), else_type, string_cstr(else_val));
                    emit(cg, "    jmp %s\n", string_cstr(end_label));
                }
            } else {
                emit(cg, "    %s =w copy 0\n", string_cstr(result));
                emit(cg, "    jmp %s\n", string_cstr(end_label));
            }
            
            /* Only emit end label if at least one branch doesn't return */
            if (!then_returned || !else_returned) {
                emit_label(cg, string_cstr(end_label));
            }
            /* Reset returned flag - the if expression as a whole didn't return
             * unless BOTH branches returned */
            cg->returned = then_returned && else_returned;
            return result;
        }
        
        case EXPR_MATCH: {
            MatchExpr* match = &expr->data.match_expr;
            String* scrutinee = codegen_expr(cg, match->value);
            String* result = fresh_temp(cg);
            String* end_label = fresh_label(cg);
            
            /* Generate code for each arm */
            for (size_t i = 0; i < match->arms->len; i++) {
                MatchArm* arm = &match->arms->data[i];
                int bound_scope = cg->bound_pattern_count;
                int wide_scope = cg->wide_var_count;
                String* next_arm_label = fresh_label(cg);
                String* arm_body_label = fresh_label(cg);
                
                /* Pattern matching */
                switch (arm->pattern->type) {
                    case PATTERN_WILDCARD:
                        /* Wildcard always matches - just jump to body */
                        emit(cg, "    jmp %s\n", string_cstr(arm_body_label));
                        break;
                        
                    case PATTERN_IDENT:
                        /* Identifier pattern - bind value and jump to body */
                        emit_pattern_binding(cg, arm->pattern, scrutinee, qbe_type_for_expr(cg, match->value));
                        emit(cg, "    jmp %s\n", string_cstr(arm_body_label));
                        break;
                        
                    case PATTERN_LIT: {
                        /* Literal pattern - compare and branch */
                        Expr* lit = arm->pattern->data.literal;
                        String* lit_temp = codegen_expr(cg, lit);
                        String* cmp = fresh_temp(cg);
                        emit(cg, "    %s =w ceq%c %s, %s\n",
                            string_cstr(cmp), qbe_type_for_expr(cg, lit),
                            string_cstr(scrutinee), string_cstr(lit_temp));
                        emit(cg, "    jnz %s, %s, %s\n",
                            string_cstr(cmp), string_cstr(arm_body_label), string_cstr(next_arm_label));
                        break;
                    }
                    
                    case PATTERN_CONSTRUCTOR: {
                        /* Constructor pattern: Some(x), Ok(value), Err(msg), None, etc. */
                        ConstructorPattern* ctor = &arm->pattern->data.constructor;
                        const char* ctor_name = string_cstr(ctor->name);
                        
                        /* Option types: Some/None - packed 64-bit value */
                        if (strcmp(ctor_name, "Some") == 0) {
                            /* Check tag == 1 (Some) */
                            String* tag = fresh_temp(cg);
                            String* cmp = fresh_temp(cg);
                            emit(cg, "    %s =l and %s, 4294967295\n", string_cstr(tag), string_cstr(scrutinee));
                            emit(cg, "    %s =w ceql %s, 1\n", string_cstr(cmp), string_cstr(tag));
                            emit(cg, "    jnz %s, %s, %s\n",
                                string_cstr(cmp), string_cstr(arm_body_label), string_cstr(next_arm_label));
                        } else if (strcmp(ctor_name, "None") == 0) {
                            /* Check tag == 0 (None) */
                            String* tag = fresh_temp(cg);
                            String* cmp = fresh_temp(cg);
                            emit(cg, "    %s =l and %s, 4294967295\n", string_cstr(tag), string_cstr(scrutinee));
                            emit(cg, "    %s =w ceql %s, 0\n", string_cstr(cmp), string_cstr(tag));
                            emit(cg, "    jnz %s, %s, %s\n",
                                string_cstr(cmp), string_cstr(arm_body_label), string_cstr(next_arm_label));
                        }
                        /* Result types: Ok/Err - heap-allocated struct pointer */
                        else if (strcmp(ctor_name, "Ok") == 0) {
                            /* Check tag == 0 (Ok) at offset 0 */
                            String* tag = fresh_temp(cg);
                            String* cmp = fresh_temp(cg);
                            emit(cg, "    %s =w loadw %s\n", string_cstr(tag), string_cstr(scrutinee));
                            emit(cg, "    %s =w ceqw %s, 0\n", string_cstr(cmp), string_cstr(tag));
                            emit(cg, "    jnz %s, %s, %s\n",
                                string_cstr(cmp), string_cstr(arm_body_label), string_cstr(next_arm_label));
                        } else if (strcmp(ctor_name, "Err") == 0) {
                            /* Check tag == 1 (Err) at offset 0 */
                            String* tag = fresh_temp(cg);
                            String* cmp = fresh_temp(cg);
                            emit(cg, "    %s =w loadw %s\n", string_cstr(tag), string_cstr(scrutinee));
                            emit(cg, "    %s =w ceqw %s, 1\n", string_cstr(cmp), string_cstr(tag));
                            emit(cg, "    jnz %s, %s, %s\n",
                                string_cstr(cmp), string_cstr(arm_body_label), string_cstr(next_arm_label));
                        } else {
                            /* Unknown constructor - treat as always matching for now */
                            emit(cg, "    # TODO: constructor %s\n", ctor_name);
                            emit(cg, "    jmp %s\n", string_cstr(arm_body_label));
                        }
                        break;
                    }
                    
                    default:
                        emit(cg, "    # TODO: pattern type %d\n", arm->pattern->type);
                        emit(cg, "    jmp %s\n", string_cstr(arm_body_label));
                }
                
                /* Arm body - emit label first */
                emit(cg, "%s\n", string_cstr(arm_body_label));
                
                /* For constructor patterns, bind the extracted value AFTER the label */
                if (arm->pattern->type == PATTERN_CONSTRUCTOR) {
                    ConstructorPattern* ctor = &arm->pattern->data.constructor;
                    const char* ctor_name = string_cstr(ctor->name);
                    
                    if (strcmp(ctor_name, "Some") == 0 && ctor->args && ctor->args->len > 0) {
                        Pattern* inner = ctor->args->data[0];
                        if (inner->type == PATTERN_IDENT) {
                            String* val = fresh_temp(cg);
                            emit(cg, "    %s =l shr %s, 32\n", string_cstr(val), string_cstr(scrutinee));
                            emit_pattern_binding(cg, inner, val, 'w');
                        }
                    } else if (strcmp(ctor_name, "Ok") == 0 && ctor->args && ctor->args->len > 0) {
                        Pattern* inner = ctor->args->data[0];
                        if (inner->type == PATTERN_IDENT || inner->type == PATTERN_TUPLE) {
                            String* val_ptr = fresh_temp(cg);
                            String* val = fresh_temp(cg);
                            emit(cg, "    %s =l add %s, 8\n", string_cstr(val_ptr), string_cstr(scrutinee));
                            emit(cg, "    %s =l loadl %s\n", string_cstr(val), string_cstr(val_ptr));
                            emit_payload_bindings(cg, inner, val, 0);
                        }
                    } else if (strcmp(ctor_name, "Err") == 0 && ctor->args && ctor->args->len > 0) {
                        Pattern* inner = ctor->args->data[0];
                        if (inner->type == PATTERN_IDENT || inner->type == PATTERN_TUPLE) {
                            String* val_ptr = fresh_temp(cg);
                            String* val = fresh_temp(cg);
                            emit(cg, "    %s =l add %s, 8\n", string_cstr(val_ptr), string_cstr(scrutinee));
                            emit(cg, "    %s =l loadl %s\n", string_cstr(val), string_cstr(val_ptr));
                            emit_payload_bindings(cg, inner, val, 0);
                        }
                    }
                }
                
                String* arm_val = codegen_expr(cg, arm->body);
                char arm_type = qbe_type_for_expr(cg, arm->body);
                cg->bound_pattern_count = bound_scope;
                cg->wide_var_count = wide_scope;
                emit(cg, "    %s =%c copy %s\n", string_cstr(result), arm_type, string_cstr(arm_val));
                if (arm_type == 'l') {
                    register_wide_var(cg, result);
                }
                emit(cg, "    jmp %s\n", string_cstr(end_label));
                
                /* Next arm label */
                emit(cg, "%s\n", string_cstr(next_arm_label));
            }
            
            /* If we fall through all arms, return 0 (should not happen with exhaustive matching) */
            char result_type = qbe_type_for_expr(cg, expr);
            emit(cg, "    %s =%c copy 0\n", string_cstr(result), result_type);
            emit(cg, "    jmp %s\n", string_cstr(end_label));
            
            emit(cg, "%s\n", string_cstr(end_label));
            return result;
        }
        
        case EXPR_CALL: {
            CallExpr* call = &expr->data.call;
            String* result = fresh_temp(cg);

            if (call->func->type == EXPR_IDENT &&
                strcmp(string_cstr(call->func->data.ident.name), "spawn_link") == 0 &&
                call->args->len == 1) {
                Expr* target = call->args->data[0].value;
                const char* actor_name = "anonymous";

                if (target->type == EXPR_IDENT) {
                    actor_name = string_cstr(target->data.ident.name);
                } else {
                    (void)codegen_expr(cg, target);
                }

                String* label = fresh_string_label(cg);
                emit_data(cg, "data %s = { b \"%s\", b 0 }\n",
                    string_cstr(label), actor_name);
                emit(cg, "    %s =l call $fern_actor_spawn_link(l %s)\n",
                    string_cstr(result), string_cstr(label));
                return result;
            }
            
            /* Check for module.function calls (e.g., String.len, Tui.Panel.new) */
            if (call->func->type == EXPR_DOT) {
                DotExpr* dot = &call->func->data.dot;
                /* Try to build module path for nested modules like Tui.Panel */
                String* module_path = try_build_module_path(cg->arena, dot->object);
                if (module_path != NULL && is_builtin_module(string_cstr(module_path))) {
                    const char* module = canonical_builtin_module_name(string_cstr(module_path));
                    assert(module != NULL);
                    const char* func = string_cstr(dot->field);
                    
                    if (strcmp(module, "Option") == 0 && strcmp(func, "unwrap_or") == 0 &&
                        call->args->len == 2) {
                        String* option = codegen_expr(cg, call->args->data[0].value);
                        String* fallback = codegen_expr(cg, call->args->data[1].value);
                        emit(cg, "    %s =l call $fern_option_unwrap_or(l %s, l %s)\n",
                            string_cstr(result), string_cstr(option), string_cstr(fallback));
                        return result;
                    }

                    /* ===== String module ===== */
                    if (strcmp(module, "String") == 0) {
                        /* String.len(s) -> Int */
                        if (strcmp(func, "len") == 0 && call->args->len == 1) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_str_len(l %s)\n",
                                string_cstr(result), string_cstr(s));
                            return result;
                        }
                        /* String.concat(a, b) -> String */
                        if (strcmp(func, "concat") == 0 && call->args->len == 2) {
                            String* a = codegen_expr(cg, call->args->data[0].value);
                            String* b = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_str_concat(l %s, l %s)\n",
                                string_cstr(result), string_cstr(a), string_cstr(b));
                            return result;
                        }
                        /* String.eq(a, b) -> Bool */
                        if (strcmp(func, "eq") == 0 && call->args->len == 2) {
                            String* a = codegen_expr(cg, call->args->data[0].value);
                            String* b = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_str_eq(l %s, l %s)\n",
                                string_cstr(result), string_cstr(a), string_cstr(b));
                            return result;
                        }
                        /* String.starts_with(s, prefix) -> Bool */
                        if (strcmp(func, "starts_with") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* prefix = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_str_starts_with(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(prefix));
                            return result;
                        }
                        /* String.ends_with(s, suffix) -> Bool */
                        if (strcmp(func, "ends_with") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* suffix = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_str_ends_with(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(suffix));
                            return result;
                        }
                        /* String.contains(s, substr) -> Bool */
                        if (strcmp(func, "contains") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* substr = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_str_contains(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(substr));
                            return result;
                        }
                        /* String.slice(s, start, end) -> String */
                        if (strcmp(func, "slice") == 0 && call->args->len == 3) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* start = codegen_expr(cg, call->args->data[1].value);
                            String* end = codegen_expr(cg, call->args->data[2].value);
                            emit(cg, "    %s =l call $fern_str_slice(l %s, l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(start), string_cstr(end));
                            return result;
                        }
                        /* String.trim(s) -> String */
                        if (strcmp(func, "trim") == 0 && call->args->len == 1) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_str_trim(l %s)\n",
                                string_cstr(result), string_cstr(s));
                            return result;
                        }
                        /* String.trim_start(s) -> String */
                        if (strcmp(func, "trim_start") == 0 && call->args->len == 1) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_str_trim_start(l %s)\n",
                                string_cstr(result), string_cstr(s));
                            return result;
                        }
                        /* String.trim_end(s) -> String */
                        if (strcmp(func, "trim_end") == 0 && call->args->len == 1) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_str_trim_end(l %s)\n",
                                string_cstr(result), string_cstr(s));
                            return result;
                        }
                        /* String.to_upper(s) -> String */
                        if (strcmp(func, "to_upper") == 0 && call->args->len == 1) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_str_to_upper(l %s)\n",
                                string_cstr(result), string_cstr(s));
                            return result;
                        }
                        /* String.to_lower(s) -> String */
                        if (strcmp(func, "to_lower") == 0 && call->args->len == 1) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_str_to_lower(l %s)\n",
                                string_cstr(result), string_cstr(s));
                            return result;
                        }
                        /* String.replace(s, old, new) -> String */
                        if (strcmp(func, "replace") == 0 && call->args->len == 3) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* old_str = codegen_expr(cg, call->args->data[1].value);
                            String* new_str = codegen_expr(cg, call->args->data[2].value);
                            emit(cg, "    %s =l call $fern_str_replace(l %s, l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(old_str), string_cstr(new_str));
                            return result;
                        }
                        /* String.repeat(s, n) -> String */
                        if (strcmp(func, "repeat") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* n = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_str_repeat(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(n));
                            return result;
                        }
                        /* String.is_empty(s) -> Bool */
                        if (strcmp(func, "is_empty") == 0 && call->args->len == 1) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_str_is_empty(l %s)\n",
                                string_cstr(result), string_cstr(s));
                            return result;
                        }
                        /* String.is_decimal(s) -> Bool */
                        if (strcmp(func, "is_decimal") == 0 && call->args->len == 1) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_str_is_decimal(l %s)\n",
                                string_cstr(result), string_cstr(s));
                            return result;
                        }
                        /* String.split(s, delim) -> List(String) */
                        if (strcmp(func, "split") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* delim = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_str_split(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(delim));
                            return result;
                        }
                        /* String.lines(s) -> List(String) */
                        if (strcmp(func, "lines") == 0 && call->args->len == 1) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_str_lines(l %s)\n",
                                string_cstr(result), string_cstr(s));
                            return result;
                        }
                        /* String.index_of(s, substr) -> Option(Int) */
                        if (strcmp(func, "index_of") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* substr = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_str_index_of(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(substr));
                            return result;
                        }
                        /* String.char_at(s, index) -> Option(Int) */
                        if (strcmp(func, "char_at") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* idx = codegen_expr(cg, call->args->data[1].value);
                            /* Extend index to 64-bit for runtime call */
                            String* idx_ext = fresh_temp(cg);
                            emit(cg, "    %s =l copy %s\n", string_cstr(idx_ext), string_cstr(idx));
                            emit(cg, "    %s =l call $fern_str_char_at(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(idx_ext));
                            return result;
                        }
                        /* String.join(list, sep) -> String */
                        if (strcmp(func, "join") == 0 && call->args->len == 2) {
                            String* list = codegen_expr(cg, call->args->data[0].value);
                            String* sep = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_str_join(l %s, l %s)\n",
                                string_cstr(result), string_cstr(list), string_cstr(sep));
                            return result;
                        }
                    }
                    
                    /* ===== List module ===== */
                    if (strcmp(module, "List") == 0) {
                        /* List.len(list) -> Int */
                        if (strcmp(func, "len") == 0 && call->args->len == 1) {
                            String* list = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_list_len(l %s)\n",
                                string_cstr(result), string_cstr(list));
                            return result;
                        }
                        /* List.get(list, index) -> elem (returns l for pointer elements) */
                        if (strcmp(func, "get") == 0 && call->args->len == 2) {
                            String* list = codegen_expr(cg, call->args->data[0].value);
                            String* index = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_list_get(l %s, l %s)\n",
                                string_cstr(result), string_cstr(list), string_cstr(index));
                            register_wide_var(cg, result);
                            return result;
                        }
                        /* List.push(list, elem) -> List */
                        if (strcmp(func, "push") == 0 && call->args->len == 2) {
                            String* list = codegen_expr(cg, call->args->data[0].value);
                            String* elem = codegen_expr(cg, call->args->data[1].value);
                            char elem_type = qbe_type_for_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_list_push(l %s, %c %s)\n",
                                string_cstr(result), string_cstr(list), elem_type, string_cstr(elem));
                            return result;
                        }
                        /* List.reverse(list) -> List */
                        if (strcmp(func, "reverse") == 0 && call->args->len == 1) {
                            String* list = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_list_reverse(l %s)\n",
                                string_cstr(result), string_cstr(list));
                            return result;
                        }
                        /* List.concat(a, b) -> List */
                        if (strcmp(func, "concat") == 0 && call->args->len == 2) {
                            String* a = codegen_expr(cg, call->args->data[0].value);
                            String* b = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_list_concat(l %s, l %s)\n",
                                string_cstr(result), string_cstr(a), string_cstr(b));
                            return result;
                        }
                        /* List.head(list) -> elem (returns pointer type for list elements) */
                        if (strcmp(func, "head") == 0 && call->args->len == 1) {
                            String* list = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_list_head(l %s)\n",
                                string_cstr(result), string_cstr(list));
                            register_wide_var(cg, result);
                            return result;
                        }
                        /* List.tail(list) -> List */
                        if (strcmp(func, "tail") == 0 && call->args->len == 1) {
                            String* list = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_list_tail(l %s)\n",
                                string_cstr(result), string_cstr(list));
                            return result;
                        }
                        /* List.is_empty(list) -> Bool */
                        if (strcmp(func, "is_empty") == 0 && call->args->len == 1) {
                            String* list = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_list_is_empty(l %s)\n",
                                string_cstr(result), string_cstr(list));
                            return result;
                        }
                        /* List.contains(list, elem) -> Bool */
                        if (strcmp(func, "contains") == 0 && call->args->len == 2) {
                            String* list = codegen_expr(cg, call->args->data[0].value);
                            String* elem = codegen_expr(cg, call->args->data[1].value);
                            /* Use string version for string elements */
                            PrintType elem_pt = get_print_type(cg, call->args->data[1].value);
                            if (elem_pt == PRINT_STRING) {
                                emit(cg, "    %s =l call $fern_list_contains_str(l %s, l %s)\n",
                                    string_cstr(result), string_cstr(list), string_cstr(elem));
                            } else {
                                char elem_type = qbe_type_for_expr(cg, call->args->data[1].value);
                                emit(cg, "    %s =l call $fern_list_contains(l %s, %c %s)\n",
                                    string_cstr(result), string_cstr(list), elem_type, string_cstr(elem));
                            }
                            return result;
                        }
                        /* List.any(list, pred) -> Bool */
                        if (strcmp(func, "any") == 0 && call->args->len == 2) {
                            String* list = codegen_expr(cg, call->args->data[0].value);
                            String* pred = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_list_any(l %s, l %s)\n",
                                string_cstr(result), string_cstr(list), string_cstr(pred));
                            return result;
                        }
                        /* List.all(list, pred) -> Bool */
                        if (strcmp(func, "all") == 0 && call->args->len == 2) {
                            String* list = codegen_expr(cg, call->args->data[0].value);
                            String* pred = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_list_all(l %s, l %s)\n",
                                string_cstr(result), string_cstr(list), string_cstr(pred));
                            return result;
                        }
                    }
                    
                    /* ===== File module ===== */
                    if (strcmp(module, "File") == 0) {
                        /* File.read(path) -> Result(String, Int) */
                        if (strcmp(func, "read") == 0 && call->args->len == 1) {
                            String* path = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_read_file(l %s)\n",
                                string_cstr(result), string_cstr(path));
                            return result;
                        }
                        /* File.write(path, contents) -> Result(Int, Int) */
                        if (strcmp(func, "write") == 0 && call->args->len == 2) {
                            String* path = codegen_expr(cg, call->args->data[0].value);
                            String* contents = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_write_file(l %s, l %s)\n",
                                string_cstr(result), string_cstr(path), string_cstr(contents));
                            return result;
                        }
                        /* File.append(path, contents) -> Result(Int, Int) */
                        if (strcmp(func, "append") == 0 && call->args->len == 2) {
                            String* path = codegen_expr(cg, call->args->data[0].value);
                            String* contents = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_append_file(l %s, l %s)\n",
                                string_cstr(result), string_cstr(path), string_cstr(contents));
                            return result;
                        }
                        /* File.exists(path) -> Bool */
                        if (strcmp(func, "exists") == 0 && call->args->len == 1) {
                            String* path = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_file_exists(l %s)\n",
                                string_cstr(result), string_cstr(path));
                            return result;
                        }
                        /* File.delete(path) -> Result(Int, Int) */
                        if (strcmp(func, "delete") == 0 && call->args->len == 1) {
                            String* path = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_delete_file(l %s)\n",
                                string_cstr(result), string_cstr(path));
                            return result;
                        }
                        /* File.size(path) -> Result(Int, Int) */
                        if (strcmp(func, "size") == 0 && call->args->len == 1) {
                            String* path = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_file_size(l %s)\n",
                                string_cstr(result), string_cstr(path));
                            return result;
                        }
                        /* File.is_dir(path) -> Bool */
                        if (strcmp(func, "is_dir") == 0 && call->args->len == 1) {
                            String* path = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_is_dir(l %s)\n",
                                string_cstr(result), string_cstr(path));
                            return result;
                        }
                        /* File.list_dir(path) -> Result(List(String), Int) */
                        if (strcmp(func, "list_dir") == 0 && call->args->len == 1) {
                            String* path = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_read_dir_result(l %s)\n",
                                string_cstr(result), string_cstr(path));
                            return result;
                        }
                    }

                    /* ===== json module ===== */
                    if (strcmp(module, "json") == 0) {
                        /* json.parse(text) -> Result(String, Int) */
                        if (strcmp(func, "parse") == 0 && call->args->len == 1) {
                            String* text = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_json_parse(l %s)\n",
                                string_cstr(result), string_cstr(text));
                            return result;
                        }
                        /* json.stringify(text) -> Result(String, Int) */
                        if (strcmp(func, "stringify") == 0 && call->args->len == 1) {
                            String* text = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_json_stringify(l %s)\n",
                                string_cstr(result), string_cstr(text));
                            return result;
                        }
                    }

                    /* ===== http module ===== */
                    if (strcmp(module, "http") == 0) {
                        /* http.get(url) -> Result(String, Int) */
                        if (strcmp(func, "get") == 0 && call->args->len == 1) {
                            String* url = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_http_get(l %s)\n",
                                string_cstr(result), string_cstr(url));
                            return result;
                        }
                        /* http.post(url, body) -> Result(String, Int) */
                        if (strcmp(func, "post") == 0 && call->args->len == 2) {
                            String* url = codegen_expr(cg, call->args->data[0].value);
                            String* body = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_http_post(l %s, l %s)\n",
                                string_cstr(result), string_cstr(url), string_cstr(body));
                            return result;
                        }
                    }

                    /* ===== sql module ===== */
                    if (strcmp(module, "sql") == 0) {
                        if (strcmp(func, "close") == 0 && call->args->len == 1) {
                            String* handle = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_sql_close(l %s)\n",
                                string_cstr(result), string_cstr(handle));
                            return result;
                        }

                        /* sql.open(path) -> Result(Int, Int) */
                        if (strcmp(func, "open") == 0 && call->args->len == 1) {
                            String* path = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_sql_open(l %s)\n",
                                string_cstr(result), string_cstr(path));
                            return result;
                        }
                        /* sql.execute(handle, query) -> Result(Int, Int) */
                        if (strcmp(func, "execute") == 0 && call->args->len == 2) {
                            String* handle = codegen_expr(cg, call->args->data[0].value);
                            String* query = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_sql_execute(l %s, l %s)\n",
                                string_cstr(result), string_cstr(handle), string_cstr(query));
                            return result;
                        }
                    }

                    /* ===== actors module ===== */
                    if (strcmp(module, "actors") == 0) {
                        /* actors.start(name) -> Int */
                        if (strcmp(func, "start") == 0 && call->args->len == 1) {
                            String* name = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_actor_start(l %s)\n",
                                string_cstr(result), string_cstr(name));
                            return result;
                        }
                        /* actors.post(actor_id, msg) -> Result(Int, Int) */
                        if (strcmp(func, "post") == 0 && call->args->len == 2) {
                            String* actor_id = codegen_expr(cg, call->args->data[0].value);
                            String* msg = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_actor_post(l %s, l %s)\n",
                                string_cstr(result), string_cstr(actor_id), string_cstr(msg));
                            return result;
                        }
                        /* actors.next(actor_id) -> Result(String, Int) */
                        if (strcmp(func, "next") == 0 && call->args->len == 1) {
                            String* actor_id = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_actor_next(l %s)\n",
                                string_cstr(result), string_cstr(actor_id));
                            return result;
                        }
                        /* actors.monitor(supervisor, worker) -> Result(Int, Int) */
                        if (strcmp(func, "monitor") == 0 && call->args->len == 2) {
                            String* supervisor_id = codegen_expr(cg, call->args->data[0].value);
                            String* worker_id = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_actor_monitor(l %s, l %s)\n",
                                string_cstr(result), string_cstr(supervisor_id), string_cstr(worker_id));
                            return result;
                        }
                        /* actors.demonitor(supervisor, worker) -> Result(Int, Int) */
                        if (strcmp(func, "demonitor") == 0 && call->args->len == 2) {
                            String* supervisor_id = codegen_expr(cg, call->args->data[0].value);
                            String* worker_id = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_actor_demonitor(l %s, l %s)\n",
                                string_cstr(result), string_cstr(supervisor_id), string_cstr(worker_id));
                            return result;
                        }
                        /* actors.restart(actor_id) -> Result(Int, Int) */
                        if (strcmp(func, "restart") == 0 && call->args->len == 1) {
                            String* actor_id = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_actor_restart(l %s)\n",
                                string_cstr(result), string_cstr(actor_id));
                            return result;
                        }
                        /* actors.supervise(supervisor, worker, max_restarts, period_sec) -> Result(Int, Int) */
                        if (strcmp(func, "supervise") == 0 && call->args->len == 4) {
                            String* supervisor_id = codegen_expr(cg, call->args->data[0].value);
                            String* worker_id = codegen_expr(cg, call->args->data[1].value);
                            String* max_restarts = codegen_expr(cg, call->args->data[2].value);
                            String* period_sec = codegen_expr(cg, call->args->data[3].value);
                            emit(cg, "    %s =l call $fern_actor_supervise(l %s, l %s, l %s, l %s)\n",
                                string_cstr(result), string_cstr(supervisor_id), string_cstr(worker_id),
                                string_cstr(max_restarts), string_cstr(period_sec));
                            return result;
                        }
                        /* actors.supervise_one_for_all(supervisor, worker, max_restarts, period_sec) -> Result(Int, Int) */
                        if (strcmp(func, "supervise_one_for_all") == 0 && call->args->len == 4) {
                            String* supervisor_id = codegen_expr(cg, call->args->data[0].value);
                            String* worker_id = codegen_expr(cg, call->args->data[1].value);
                            String* max_restarts = codegen_expr(cg, call->args->data[2].value);
                            String* period_sec = codegen_expr(cg, call->args->data[3].value);
                            emit(cg, "    %s =l call $fern_actor_supervise_one_for_all(l %s, l %s, l %s, l %s)\n",
                                string_cstr(result), string_cstr(supervisor_id), string_cstr(worker_id),
                                string_cstr(max_restarts), string_cstr(period_sec));
                            return result;
                        }
                        /* actors.supervise_rest_for_one(supervisor, worker, max_restarts, period_sec) -> Result(Int, Int) */
                        if (strcmp(func, "supervise_rest_for_one") == 0 && call->args->len == 4) {
                            String* supervisor_id = codegen_expr(cg, call->args->data[0].value);
                            String* worker_id = codegen_expr(cg, call->args->data[1].value);
                            String* max_restarts = codegen_expr(cg, call->args->data[2].value);
                            String* period_sec = codegen_expr(cg, call->args->data[3].value);
                            emit(cg, "    %s =l call $fern_actor_supervise_rest_for_one(l %s, l %s, l %s, l %s)\n",
                                string_cstr(result), string_cstr(supervisor_id), string_cstr(worker_id),
                                string_cstr(max_restarts), string_cstr(period_sec));
                            return result;
                        }
                    }

                    /* ===== System module ===== */
                    if (strcmp(module, "System") == 0) {
                        /* System.args() -> List(String) */
                        if (strcmp(func, "args") == 0 && call->args->len == 0) {
                            emit(cg, "    %s =l call $fern_args()\n", string_cstr(result));
                            return result;
                        }
                        /* System.args_count() -> Int */
                        if (strcmp(func, "args_count") == 0 && call->args->len == 0) {
                            emit(cg, "    %s =l call $fern_args_count()\n", string_cstr(result));
                            return result;
                        }
                        /* System.arg(index) -> String */
                        if (strcmp(func, "arg") == 0 && call->args->len == 1) {
                            String* idx = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_arg(l %s)\n",
                                string_cstr(result), string_cstr(idx));
                            return result;
                        }
                        /* System.exit(code) -> Unit */
                        if (strcmp(func, "exit") == 0 && call->args->len == 1) {
                            String* code = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    call $fern_exit(l %s)\n", string_cstr(code));
                            emit(cg, "    %s =w copy 0\n", string_cstr(result));
                            return result;
                        }
                        if (strcmp(func, "write_stderr") == 0 && call->args->len == 1) {
                            String* text = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_write_stderr(l %s)\n",
                                string_cstr(result), string_cstr(text));
                            return result;
                        }
                        /* System.exec(cmd) -> (Int, String, String) */
                        if (strcmp(func, "exec") == 0 && call->args->len == 1) {
                            String* cmd = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_exec(l %s)\n",
                                string_cstr(result), string_cstr(cmd));
                            return result;
                        }
                        /* Preserve every native argument/result bit, including rejected limits. */
                        if (strcmp(func, "exec_args_bounded") == 0 && call->args->len == 3) {
                            String* args = codegen_expr(cg, call->args->data[0].value);
                            String* timeout = codegen_expr(cg, call->args->data[1].value);
                            String* cap = codegen_expr(cg, call->args->data[2].value);
                            emit(cg, "    %s =l call $fern_exec_args_bounded(l %s, l %s, l %s)\n",
                                string_cstr(result), string_cstr(args), string_cstr(timeout),
                                string_cstr(cap));
                            return result;
                        }
                        /* System.exec_args(args) -> (Int, String, String) */
                        if (strcmp(func, "exec_args") == 0 && call->args->len == 1) {
                            String* args = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_exec_args(l %s)\n",
                                string_cstr(result), string_cstr(args));
                            return result;
                        }
                        /* System.getenv(name) -> String */
                        if (strcmp(func, "getenv") == 0 && call->args->len == 1) {
                            String* name = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_getenv(l %s)\n",
                                string_cstr(result), string_cstr(name));
                            return result;
                        }
                        /* System.setenv(name, value) -> Int */
                        if (strcmp(func, "setenv") == 0 && call->args->len == 2) {
                            String* name = codegen_expr(cg, call->args->data[0].value);
                            String* value = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_setenv(l %s, l %s)\n",
                                string_cstr(result), string_cstr(name), string_cstr(value));
                            return result;
                        }
                        /* System.cwd() -> String */
                        if (strcmp(func, "cwd") == 0 && call->args->len == 0) {
                            emit(cg, "    %s =l call $fern_cwd()\n", string_cstr(result));
                            return result;
                        }
                        /* System.chdir(path) -> Int */
                        if (strcmp(func, "chdir") == 0 && call->args->len == 1) {
                            String* path = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_chdir(l %s)\n",
                                string_cstr(result), string_cstr(path));
                            return result;
                        }
                        /* System.hostname() -> String */
                        if (strcmp(func, "hostname") == 0 && call->args->len == 0) {
                            emit(cg, "    %s =l call $fern_hostname()\n", string_cstr(result));
                            return result;
                        }
                        /* System.user() -> String */
                        if (strcmp(func, "user") == 0 && call->args->len == 0) {
                            emit(cg, "    %s =l call $fern_user()\n", string_cstr(result));
                            return result;
                        }
                        /* System.home() -> String */
                        if (strcmp(func, "home") == 0 && call->args->len == 0) {
                            emit(cg, "    %s =l call $fern_home()\n", string_cstr(result));
                            return result;
                        }
                    }

                    /* ===== Regex module ===== */
                    if (strcmp(module, "Regex") == 0) {
                        /* Regex.is_match(s, pattern) -> Bool */
                        if (strcmp(func, "is_match") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* pattern = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_regex_is_match(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(pattern));
                            return result;
                        }
                        /* Regex.find(s, pattern) -> RegexMatch* */
                        if (strcmp(func, "find") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* pattern = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_regex_find(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(pattern));
                            return result;
                        }
                        /* Regex.find_all(s, pattern) -> List(String) */
                        if (strcmp(func, "find_all") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* pattern = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_regex_find_all(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(pattern));
                            return result;
                        }
                        /* Regex.replace(s, pattern, repl) -> String */
                        if (strcmp(func, "replace") == 0 && call->args->len == 3) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* pattern = codegen_expr(cg, call->args->data[1].value);
                            String* repl = codegen_expr(cg, call->args->data[2].value);
                            emit(cg, "    %s =l call $fern_regex_replace(l %s, l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(pattern), string_cstr(repl));
                            return result;
                        }
                        /* Regex.replace_all(s, pattern, repl) -> String */
                        if (strcmp(func, "replace_all") == 0 && call->args->len == 3) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* pattern = codegen_expr(cg, call->args->data[1].value);
                            String* repl = codegen_expr(cg, call->args->data[2].value);
                            emit(cg, "    %s =l call $fern_regex_replace_all(l %s, l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(pattern), string_cstr(repl));
                            return result;
                        }
                        /* Regex.split(s, pattern) -> List(String) */
                        if (strcmp(func, "split") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* pattern = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_regex_split(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(pattern));
                            return result;
                        }
                        /* Regex.captures(s, pattern) -> captures* */
                        if (strcmp(func, "captures") == 0 && call->args->len == 2) {
                            String* s = codegen_expr(cg, call->args->data[0].value);
                            String* pattern = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_regex_captures(l %s, l %s)\n",
                                string_cstr(result), string_cstr(s), string_cstr(pattern));
                            return result;
                        }
                    }

                    /* Tree values and log records use pointer-sized runtime results. */
                    if ((strcmp(module, "Tui.Tree") == 0 || strcmp(module, "Tui.Log") == 0) &&
                        call->args->len >= 1 && call->args->len <= 2) {
                        String* first = codegen_expr(cg, call->args->data[0].value);
                        const char* prefix = strcmp(module, "Tui.Tree") == 0 ? "tree" : "log";
                        if (call->args->len == 2) {
                            String* second = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_%s_%s(l %s, l %s)\n",
                                string_cstr(result), prefix, func, string_cstr(first),
                                string_cstr(second));
                        } else {
                            emit(cg, "    %s =l call $fern_%s_%s(l %s)\n",
                                string_cstr(result), prefix, func, string_cstr(first));
                        }
                        return result;
                    }

                    /* ===== Tui.Term module ===== */
                    if (strcmp(module, "Tui.Term") == 0) {
                        if (strcmp(func, "move_to") == 0 || strcmp(func, "up") == 0 ||
                            strcmp(func, "down") == 0 || strcmp(func, "left") == 0 ||
                            strcmp(func, "right") == 0) {
                            String* first_value = codegen_expr(cg, call->args->data[0].value);
                            String* first = fresh_temp(cg);
                            emit(cg, "    %s =l copy %s\n", string_cstr(first),
                                string_cstr(first_value));
                            if (call->args->len == 2) {
                                String* second_value = codegen_expr(cg, call->args->data[1].value);
                                String* second = fresh_temp(cg);
                                emit(cg, "    %s =l copy %s\n", string_cstr(second),
                                    string_cstr(second_value));
                                emit(cg, "    call $fern_term_%s(l %s, l %s)\n", func,
                                    string_cstr(first), string_cstr(second));
                            } else {
                                emit(cg, "    call $fern_term_%s(l %s)\n", func, string_cstr(first));
                            }
                            emit(cg, "    %s =w copy 0\n", string_cstr(result));
                            return result;
                        }
                        if (strcmp(func, "clear") == 0 || strcmp(func, "hide_cursor") == 0 ||
                            strcmp(func, "show_cursor") == 0 || strcmp(func, "save_cursor") == 0 ||
                            strcmp(func, "restore_cursor") == 0) {
                            emit(cg, "    call $fern_term_%s()\n", func);
                            emit(cg, "    %s =w copy 0\n", string_cstr(result));
                            return result;
                        }
                        /* Tui.Term.size() -> (cols, rows) as struct pointer */
                        if (strcmp(func, "size") == 0 && call->args->len == 0) {
                            emit(cg, "    %s =l call $fern_term_size()\n", string_cstr(result));
                            return result;
                        }
                        /* Tui.Term.is_tty() -> Bool */
                        if (strcmp(func, "is_tty") == 0 && call->args->len == 0) {
                            emit(cg, "    %s =l call $fern_term_is_tty()\n", string_cstr(result));
                            return result;
                        }
                        /* Tui.Term.color_support() -> Int */
                        if (strcmp(func, "color_support") == 0 && call->args->len == 0) {
                            emit(cg, "    %s =l call $fern_term_color_support()\n", string_cstr(result));
                            return result;
                        }
                    }

                    /* ===== Tui.Style module ===== */
                    if (strcmp(module, "Tui.Style") == 0) {
                        /* Single-argument style functions: Style.red(text), Style.bold(text), etc. */
                        if (call->args->len == 1) {
                            String* text = codegen_expr(cg, call->args->data[0].value);
                            const char* fn_name = NULL;
                            if (strcmp(func, "black") == 0) fn_name = "fern_style_black";
                            else if (strcmp(func, "red") == 0) fn_name = "fern_style_red";
                            else if (strcmp(func, "green") == 0) fn_name = "fern_style_green";
                            else if (strcmp(func, "yellow") == 0) fn_name = "fern_style_yellow";
                            else if (strcmp(func, "blue") == 0) fn_name = "fern_style_blue";
                            else if (strcmp(func, "magenta") == 0) fn_name = "fern_style_magenta";
                            else if (strcmp(func, "cyan") == 0) fn_name = "fern_style_cyan";
                            else if (strcmp(func, "white") == 0) fn_name = "fern_style_white";
                            else if (strcmp(func, "bright_black") == 0) fn_name = "fern_style_bright_black";
                            else if (strcmp(func, "bright_red") == 0) fn_name = "fern_style_bright_red";
                            else if (strcmp(func, "bright_green") == 0) fn_name = "fern_style_bright_green";
                            else if (strcmp(func, "bright_yellow") == 0) fn_name = "fern_style_bright_yellow";
                            else if (strcmp(func, "bright_blue") == 0) fn_name = "fern_style_bright_blue";
                            else if (strcmp(func, "bright_magenta") == 0) fn_name = "fern_style_bright_magenta";
                            else if (strcmp(func, "bright_cyan") == 0) fn_name = "fern_style_bright_cyan";
                            else if (strcmp(func, "bright_white") == 0) fn_name = "fern_style_bright_white";
                            else if (strcmp(func, "on_black") == 0) fn_name = "fern_style_on_black";
                            else if (strcmp(func, "on_red") == 0) fn_name = "fern_style_on_red";
                            else if (strcmp(func, "on_green") == 0) fn_name = "fern_style_on_green";
                            else if (strcmp(func, "on_yellow") == 0) fn_name = "fern_style_on_yellow";
                            else if (strcmp(func, "on_blue") == 0) fn_name = "fern_style_on_blue";
                            else if (strcmp(func, "on_magenta") == 0) fn_name = "fern_style_on_magenta";
                            else if (strcmp(func, "on_cyan") == 0) fn_name = "fern_style_on_cyan";
                            else if (strcmp(func, "on_white") == 0) fn_name = "fern_style_on_white";
                            else if (strcmp(func, "bold") == 0) fn_name = "fern_style_bold";
                            else if (strcmp(func, "dim") == 0) fn_name = "fern_style_dim";
                            else if (strcmp(func, "italic") == 0) fn_name = "fern_style_italic";
                            else if (strcmp(func, "underline") == 0) fn_name = "fern_style_underline";
                            else if (strcmp(func, "blink") == 0) fn_name = "fern_style_blink";
                            else if (strcmp(func, "reverse") == 0) fn_name = "fern_style_reverse";
                            else if (strcmp(func, "strikethrough") == 0) fn_name = "fern_style_strikethrough";
                            else if (strcmp(func, "reset") == 0) fn_name = "fern_style_reset";
                            
                            if (fn_name) {
                                emit(cg, "    %s =l call $%s(l %s)\n",
                                    string_cstr(result), fn_name, string_cstr(text));
                                return result;
                            }
                        }
                        /* Style.color(text, code) and Style.on_color(text, code) */
                        if (call->args->len == 2 && 
                            (strcmp(func, "color") == 0 || strcmp(func, "on_color") == 0)) {
                            String* text = codegen_expr(cg, call->args->data[0].value);
                            String* code = codegen_expr(cg, call->args->data[1].value);
                            const char* fn_name = strcmp(func, "color") == 0 ? 
                                "fern_style_color" : "fern_style_on_color";
                            emit(cg, "    %s =l call $%s(l %s, w %s)\n",
                                string_cstr(result), fn_name, string_cstr(text), string_cstr(code));
                            return result;
                        }
                        /* Style.rgb(text, r, g, b) and Style.on_rgb(text, r, g, b) */
                        if (call->args->len == 4 && 
                            (strcmp(func, "rgb") == 0 || strcmp(func, "on_rgb") == 0)) {
                            String* text = codegen_expr(cg, call->args->data[0].value);
                            String* r = codegen_expr(cg, call->args->data[1].value);
                            String* g = codegen_expr(cg, call->args->data[2].value);
                            String* b = codegen_expr(cg, call->args->data[3].value);
                            const char* fn_name = strcmp(func, "rgb") == 0 ? 
                                "fern_style_rgb" : "fern_style_on_rgb";
                            emit(cg, "    %s =l call $%s(l %s, w %s, w %s, w %s)\n",
                                string_cstr(result), fn_name, string_cstr(text), 
                                string_cstr(r), string_cstr(g), string_cstr(b));
                            return result;
                        }
                        /* Style.hex(text, hex_color) and Style.on_hex(text, hex_color) */
                        if (call->args->len == 2 && 
                            (strcmp(func, "hex") == 0 || strcmp(func, "on_hex") == 0)) {
                            String* text = codegen_expr(cg, call->args->data[0].value);
                            String* hex = codegen_expr(cg, call->args->data[1].value);
                            const char* fn_name = strcmp(func, "hex") == 0 ? 
                                "fern_style_hex" : "fern_style_on_hex";
                            emit(cg, "    %s =l call $%s(l %s, l %s)\n",
                                string_cstr(result), fn_name, string_cstr(text), string_cstr(hex));
                            return result;
                        }
                    }

                    /* ===== Tui.Status module (badges) ===== */
                    if (strcmp(module, "Tui.Status") == 0) {
                        /* Status.warn(msg), Status.ok(msg), etc. */
                        if (call->args->len == 1) {
                            String* msg = codegen_expr(cg, call->args->data[0].value);
                            const char* fn_name = NULL;
                            if (strcmp(func, "warn") == 0) fn_name = "fern_status_warn";
                            else if (strcmp(func, "ok") == 0) fn_name = "fern_status_ok";
                            else if (strcmp(func, "info") == 0) fn_name = "fern_status_info";
                            else if (strcmp(func, "error") == 0) fn_name = "fern_status_error";
                            else if (strcmp(func, "debug") == 0) fn_name = "fern_status_debug";
                            
                            if (fn_name) {
                                emit(cg, "    %s =l call $%s(l %s)\n",
                                    string_cstr(result), fn_name, string_cstr(msg));
                                return result;
                            }
                        }
                    }

                    /* ===== Tui.Live module (same-line updates) ===== */
                    if (strcmp(module, "Tui.Live") == 0) {
                        /* Live.print(text) -> Unit */
                        if (strcmp(func, "print") == 0 && call->args->len == 1) {
                            String* text = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    call $fern_live_print(l %s)\n", string_cstr(text));
                            emit(cg, "    %s =w copy 0\n", string_cstr(result));
                            return result;
                        }
                        /* Live.clear_line() -> Unit */
                        if (strcmp(func, "clear_line") == 0 && call->args->len == 0) {
                            emit(cg, "    call $fern_live_clear_line()\n");
                            emit(cg, "    %s =w copy 0\n", string_cstr(result));
                            return result;
                        }
                        /* Live.update(text) -> Unit */
                        if (strcmp(func, "update") == 0 && call->args->len == 1) {
                            String* text = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    call $fern_live_update(l %s)\n", string_cstr(text));
                            emit(cg, "    %s =w copy 0\n", string_cstr(result));
                            return result;
                        }
                        /* Live.done() -> Unit */
                        if (strcmp(func, "done") == 0 && call->args->len == 0) {
                            emit(cg, "    call $fern_live_done()\n");
                            emit(cg, "    %s =w copy 0\n", string_cstr(result));
                            return result;
                        }
                        /* Live.sleep(ms) -> Unit */
                        if (strcmp(func, "sleep") == 0 && call->args->len == 1) {
                            String* ms = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    call $fern_sleep_ms(l %s)\n", string_cstr(ms));
                            emit(cg, "    %s =w copy 0\n", string_cstr(result));
                            return result;
                        }
                    }

                    /* ===== Tui.Panel module ===== */
                    if (strcmp(module, "Tui.Panel") == 0) {
                        /* Panel.new(content) -> Panel */
                        if (strcmp(func, "new") == 0 && call->args->len == 1) {
                            String* content = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_panel_new(l %s)\n",
                                string_cstr(result), string_cstr(content));
                            return result;
                        }
                        /* Panel.title(panel, title) -> Panel */
                        if (strcmp(func, "title") == 0 && call->args->len == 2) {
                            String* panel = codegen_expr(cg, call->args->data[0].value);
                            String* title = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_panel_title(l %s, l %s)\n",
                                string_cstr(result), string_cstr(panel), string_cstr(title));
                            return result;
                        }
                        /* Panel.subtitle(panel, subtitle) -> Panel */
                        if (strcmp(func, "subtitle") == 0 && call->args->len == 2) {
                            String* panel = codegen_expr(cg, call->args->data[0].value);
                            String* subtitle = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_panel_subtitle(l %s, l %s)\n",
                                string_cstr(result), string_cstr(panel), string_cstr(subtitle));
                            return result;
                        }
                        /* Panel.border(panel, style) -> Panel */
                        if (strcmp(func, "border") == 0 && call->args->len == 2) {
                            String* panel = codegen_expr(cg, call->args->data[0].value);
                            String* style = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_panel_border_str(l %s, l %s)\n",
                                string_cstr(result), string_cstr(panel), string_cstr(style));
                            return result;
                        }
                        /* Panel.width(panel, width) -> Panel */
                        if (strcmp(func, "width") == 0 && call->args->len == 2) {
                            String* panel = codegen_expr(cg, call->args->data[0].value);
                            String* width = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_panel_width(l %s, l %s)\n",
                                string_cstr(result), string_cstr(panel), string_cstr(width));
                            return result;
                        }
                        /* Panel.padding(panel, padding) -> Panel */
                        if (strcmp(func, "padding") == 0 && call->args->len == 2) {
                            String* panel = codegen_expr(cg, call->args->data[0].value);
                            String* padding = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_panel_padding(l %s, l %s)\n",
                                string_cstr(result), string_cstr(panel), string_cstr(padding));
                            return result;
                        }
                        /* Panel.border_color(panel, color) -> Panel */
                        if (strcmp(func, "border_color") == 0 && call->args->len == 2) {
                            String* panel = codegen_expr(cg, call->args->data[0].value);
                            String* color = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_panel_border_color(l %s, l %s)\n",
                                string_cstr(result), string_cstr(panel), string_cstr(color));
                            return result;
                        }
                        /* Panel.render(panel) -> String */
                        if (strcmp(func, "render") == 0 && call->args->len == 1) {
                            String* panel = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_panel_render(l %s)\n",
                                string_cstr(result), string_cstr(panel));
                            return result;
                        }
                    }

                    /* ===== Tui.Table module ===== */
                    if (strcmp(module, "Tui.Table") == 0) {
                        /* Table.new() -> Table */
                        if (strcmp(func, "new") == 0 && call->args->len == 0) {
                            emit(cg, "    %s =l call $fern_table_new()\n", string_cstr(result));
                            return result;
                        }
                        /* Table.add_column(table, header) -> Table */
                        if (strcmp(func, "add_column") == 0 && call->args->len == 2) {
                            String* table = codegen_expr(cg, call->args->data[0].value);
                            String* header = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_table_add_column(l %s, l %s)\n",
                                string_cstr(result), string_cstr(table), string_cstr(header));
                            return result;
                        }
                        /* Table.add_row(table, cells) -> Table */
                        if (strcmp(func, "add_row") == 0 && call->args->len == 2) {
                            String* table = codegen_expr(cg, call->args->data[0].value);
                            String* cells = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_table_add_row(l %s, l %s)\n",
                                string_cstr(result), string_cstr(table), string_cstr(cells));
                            return result;
                        }
                        /* Table.title(table, title) -> Table */
                        if (strcmp(func, "title") == 0 && call->args->len == 2) {
                            String* table = codegen_expr(cg, call->args->data[0].value);
                            String* title = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_table_title(l %s, l %s)\n",
                                string_cstr(result), string_cstr(table), string_cstr(title));
                            return result;
                        }
                        /* Table.border(table, style) -> Table */
                        if (strcmp(func, "border") == 0 && call->args->len == 2) {
                            String* table = codegen_expr(cg, call->args->data[0].value);
                            String* style = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_table_border(l %s, l %s)\n",
                                string_cstr(result), string_cstr(table), string_cstr(style));
                            return result;
                        }
                        /* Table.show_header(table, show) -> Table */
                        if (strcmp(func, "show_header") == 0 && call->args->len == 2) {
                            String* table = codegen_expr(cg, call->args->data[0].value);
                            String* show = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_table_show_header(l %s, l %s)\n",
                                string_cstr(result), string_cstr(table), string_cstr(show));
                            return result;
                        }
                        /* Table.render(table) -> String */
                        if (strcmp(func, "render") == 0 && call->args->len == 1) {
                            String* table = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_table_render(l %s)\n",
                                string_cstr(result), string_cstr(table));
                            return result;
                        }
                    }

                    /* ===== Tui.Progress module ===== */
                    if (strcmp(module, "Tui.Progress") == 0) {
                        /* Progress.new(total) -> Progress */
                        if (strcmp(func, "new") == 0 && call->args->len == 1) {
                            String* total = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_progress_new(l %s)\n",
                                string_cstr(result), string_cstr(total));
                            return result;
                        }
                        /* Progress.description(progress, desc) -> Progress */
                        if (strcmp(func, "description") == 0 && call->args->len == 2) {
                            String* prog = codegen_expr(cg, call->args->data[0].value);
                            String* desc = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_progress_description(l %s, l %s)\n",
                                string_cstr(result), string_cstr(prog), string_cstr(desc));
                            return result;
                        }
                        /* Progress.width(progress, width) -> Progress */
                        if (strcmp(func, "width") == 0 && call->args->len == 2) {
                            String* prog = codegen_expr(cg, call->args->data[0].value);
                            String* width = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_progress_width(l %s, l %s)\n",
                                string_cstr(result), string_cstr(prog), string_cstr(width));
                            return result;
                        }
                        /* Progress.advance(progress) -> Progress */
                        if (strcmp(func, "advance") == 0 && call->args->len == 1) {
                            String* prog = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_progress_advance(l %s)\n",
                                string_cstr(result), string_cstr(prog));
                            return result;
                        }
                        /* Progress.set(progress, value) -> Progress */
                        if (strcmp(func, "set") == 0 && call->args->len == 2) {
                            String* prog = codegen_expr(cg, call->args->data[0].value);
                            String* value = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_progress_set(l %s, l %s)\n",
                                string_cstr(result), string_cstr(prog), string_cstr(value));
                            return result;
                        }
                        /* Progress.render(progress) -> String */
                        if (strcmp(func, "render") == 0 && call->args->len == 1) {
                            String* prog = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_progress_render(l %s)\n",
                                string_cstr(result), string_cstr(prog));
                            return result;
                        }
                    }

                    /* ===== Tui.Spinner module ===== */
                    if (strcmp(module, "Tui.Spinner") == 0) {
                        /* Spinner.new() -> Spinner */
                        if (strcmp(func, "new") == 0 && call->args->len == 0) {
                            emit(cg, "    %s =l call $fern_spinner_new()\n", string_cstr(result));
                            return result;
                        }
                        /* Spinner.message(spinner, message) -> Spinner */
                        if (strcmp(func, "message") == 0 && call->args->len == 2) {
                            String* spin = codegen_expr(cg, call->args->data[0].value);
                            String* msg = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_spinner_message(l %s, l %s)\n",
                                string_cstr(result), string_cstr(spin), string_cstr(msg));
                            return result;
                        }
                        /* Spinner.style(spinner, style) -> Spinner */
                        if (strcmp(func, "style") == 0 && call->args->len == 2) {
                            String* spin = codegen_expr(cg, call->args->data[0].value);
                            String* style = codegen_expr(cg, call->args->data[1].value);
                            emit(cg, "    %s =l call $fern_spinner_style(l %s, l %s)\n",
                                string_cstr(result), string_cstr(spin), string_cstr(style));
                            return result;
                        }
                        /* Spinner.tick(spinner) -> Spinner */
                        if (strcmp(func, "tick") == 0 && call->args->len == 1) {
                            String* spin = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_spinner_tick(l %s)\n",
                                string_cstr(result), string_cstr(spin));
                            return result;
                        }
                        /* Spinner.render(spinner) -> String */
                        if (strcmp(func, "render") == 0 && call->args->len == 1) {
                            String* spin = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_spinner_render(l %s)\n",
                                string_cstr(result), string_cstr(spin));
                            return result;
                        }
                    }

                    /* ===== Tui.Prompt module ===== */
                    if (strcmp(module, "Tui.Prompt") == 0) {
                        /* Tui.Prompt.input(prompt) -> String */
                        if (strcmp(func, "input") == 0 && call->args->len == 1) {
                            String* prompt = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_prompt_input(l %s)\n",
                                string_cstr(result), string_cstr(prompt));
                            return result;
                        }
                        /* Tui.Prompt.confirm(prompt) -> Bool */
                        if (strcmp(func, "confirm") == 0 && call->args->len == 1) {
                            String* prompt = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =w call $fern_prompt_confirm(l %s)\n",
                                string_cstr(result), string_cstr(prompt));
                            return result;
                        }
                        /* Tui.Prompt.select(prompt, choices) -> Int */
                        if (strcmp(func, "select") == 0 && call->args->len == 2) {
                            String* prompt = codegen_expr(cg, call->args->data[0].value);
                            String* choices = codegen_expr(cg, call->args->data[1].value);
                            String* native = fresh_temp(cg);
                            emit(cg, "    %s =w call $fern_prompt_select(l %s, l %s)\n",
                                string_cstr(native), string_cstr(prompt), string_cstr(choices));
                            emit(cg, "    %s =l extsw %s\n", string_cstr(result), string_cstr(native));
                            return result;
                        }
                        /* Tui.Prompt.password(prompt) -> String */
                        if (strcmp(func, "password") == 0 && call->args->len == 1) {
                            String* prompt = codegen_expr(cg, call->args->data[0].value);
                            emit(cg, "    %s =l call $fern_prompt_password(l %s)\n",
                                string_cstr(result), string_cstr(prompt));
                            return result;
                        }
                        /* Tui.Prompt.int(prompt, min, max) -> Int */
                        if (strcmp(func, "int") == 0 && call->args->len == 3) {
                            String* prompt = codegen_expr(cg, call->args->data[0].value);
                            String* min = codegen_expr(cg, call->args->data[1].value);
                            String* max = codegen_expr(cg, call->args->data[2].value);
                            emit(cg, "    %s =l call $fern_prompt_int(l %s, l %s, l %s)\n",
                                string_cstr(result), string_cstr(prompt), 
                                string_cstr(min), string_cstr(max));
                            return result;
                        }
                    }
                }
            }
            
            /* Check for special Result constructors Ok and Err */
            if (call->func->type == EXPR_IDENT) {
                const char* fn_name = string_cstr(call->func->data.ident.name);
                
                /* Handle Ok(value) - creates a successful Result */
                if (strcmp(fn_name, "Ok") == 0 && call->args->len == 1) {
                    String* val = codegen_result_payload(cg, call->args->data[0].value);
                    /* Native Result payloads are always full-width values. */
                    emit(cg, "    %s =l call $fern_result_ok(l %s)\n",
                        string_cstr(result), string_cstr(val));
                    return result;
                }
                
                /* Handle Err(error) - creates an error Result */
                if (strcmp(fn_name, "Err") == 0 && call->args->len == 1) {
                    String* err = codegen_result_payload(cg, call->args->data[0].value);
                    /* Native Result payloads are always full-width values. */
                    emit(cg, "    %s =l call $fern_result_err(l %s)\n",
                        string_cstr(result), string_cstr(err));
                    return result;
                }

                /* Handle print(value) - prints to stdout without newline */
                if (strcmp(fn_name, "print") == 0 && call->args->len == 1) {
                    Expr* arg = call->args->data[0].value;
                    String* val = codegen_expr(cg, arg);
                    PrintType pt = get_print_type(cg, arg);
                    char val_type = qbe_type_for_expr(cg, arg);
                    switch (pt) {
                        case PRINT_STRING:
                            emit(cg, "    call $fern_print_str(l %s)\n", string_cstr(val));
                            break;
                        case PRINT_BOOL:
                            emit(cg, "    call $fern_print_bool(%c %s)\n", val_type, string_cstr(val));
                            break;
                        case PRINT_INT:
                        default:
                            emit(cg, "    call $fern_print_int(%c %s)\n", val_type, string_cstr(val));
                            break;
                    }
                    emit(cg, "    %s =w copy 0\n", string_cstr(result));
                    return result;
                }

                /* Handle println(value) - prints to stdout with newline */
                if (strcmp(fn_name, "println") == 0 && call->args->len == 1) {
                    Expr* arg = call->args->data[0].value;
                    String* val = codegen_expr(cg, arg);
                    PrintType pt = get_print_type(cg, arg);
                    char val_type = qbe_type_for_expr(cg, arg);
                    switch (pt) {
                        case PRINT_STRING:
                            emit(cg, "    call $fern_println_str(l %s)\n", string_cstr(val));
                            break;
                        case PRINT_BOOL:
                            emit(cg, "    call $fern_println_bool(%c %s)\n", val_type, string_cstr(val));
                            break;
                        case PRINT_INT:
                        default:
                            emit(cg, "    call $fern_println_int(%c %s)\n", val_type, string_cstr(val));
                            break;
                    }
                    emit(cg, "    %s =w copy 0\n", string_cstr(result));
                    return result;
                }

                /* Handle str_len(s) -> Int */
                if (strcmp(fn_name, "str_len") == 0 && call->args->len == 1) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_str_len(l %s)\n",
                        string_cstr(result), string_cstr(s));
                    return result;
                }

                /* Handle str_concat(a, b) -> String */
                if (strcmp(fn_name, "str_concat") == 0 && call->args->len == 2) {
                    String* a = codegen_expr(cg, call->args->data[0].value);
                    String* b = codegen_expr(cg, call->args->data[1].value);
                    emit(cg, "    %s =l call $fern_str_concat(l %s, l %s)\n",
                        string_cstr(result), string_cstr(a), string_cstr(b));
                    return result;
                }

                /* Handle str_eq(a, b) -> Bool */
                if (strcmp(fn_name, "str_eq") == 0 && call->args->len == 2) {
                    String* a = codegen_expr(cg, call->args->data[0].value);
                    String* b = codegen_expr(cg, call->args->data[1].value);
                    emit(cg, "    %s =l call $fern_str_eq(l %s, l %s)\n",
                        string_cstr(result), string_cstr(a), string_cstr(b));
                    return result;
                }

                /* Handle list_len(list) -> Int */
                if (strcmp(fn_name, "list_len") == 0 && call->args->len == 1) {
                    String* list = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_list_len(l %s)\n",
                        string_cstr(result), string_cstr(list));
                    return result;
                }

                /* Handle list_get(list, index) -> elem (returns l for pointer elements) */
                if (strcmp(fn_name, "list_get") == 0 && call->args->len == 2) {
                    String* list = codegen_expr(cg, call->args->data[0].value);
                    String* index = codegen_expr(cg, call->args->data[1].value);
                    emit(cg, "    %s =l call $fern_list_get(l %s, l %s)\n",
                        string_cstr(result), string_cstr(list), string_cstr(index));
                    register_wide_var(cg, result);
                    return result;
                }

                /* ===== Additional String Functions ===== */

                /* Handle str_starts_with(s, prefix) -> Bool */
                if (strcmp(fn_name, "str_starts_with") == 0 && call->args->len == 2) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    String* prefix = codegen_expr(cg, call->args->data[1].value);
                    emit(cg, "    %s =l call $fern_str_starts_with(l %s, l %s)\n",
                        string_cstr(result), string_cstr(s), string_cstr(prefix));
                    return result;
                }

                /* Handle str_ends_with(s, suffix) -> Bool */
                if (strcmp(fn_name, "str_ends_with") == 0 && call->args->len == 2) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    String* suffix = codegen_expr(cg, call->args->data[1].value);
                    emit(cg, "    %s =l call $fern_str_ends_with(l %s, l %s)\n",
                        string_cstr(result), string_cstr(s), string_cstr(suffix));
                    return result;
                }

                /* Handle str_contains(s, substr) -> Bool */
                if (strcmp(fn_name, "str_contains") == 0 && call->args->len == 2) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    String* substr = codegen_expr(cg, call->args->data[1].value);
                    emit(cg, "    %s =l call $fern_str_contains(l %s, l %s)\n",
                        string_cstr(result), string_cstr(s), string_cstr(substr));
                    return result;
                }

                /* Handle str_slice(s, start, end) -> String */
                if (strcmp(fn_name, "str_slice") == 0 && call->args->len == 3) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    String* start = codegen_expr(cg, call->args->data[1].value);
                    String* end = codegen_expr(cg, call->args->data[2].value);
                    emit(cg, "    %s =l call $fern_str_slice(l %s, l %s, l %s)\n",
                        string_cstr(result), string_cstr(s), string_cstr(start), string_cstr(end));
                    return result;
                }

                /* Handle str_trim(s) -> String */
                if (strcmp(fn_name, "str_trim") == 0 && call->args->len == 1) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_str_trim(l %s)\n",
                        string_cstr(result), string_cstr(s));
                    return result;
                }

                /* Handle str_trim_start(s) -> String */
                if (strcmp(fn_name, "str_trim_start") == 0 && call->args->len == 1) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_str_trim_start(l %s)\n",
                        string_cstr(result), string_cstr(s));
                    return result;
                }

                /* Handle str_trim_end(s) -> String */
                if (strcmp(fn_name, "str_trim_end") == 0 && call->args->len == 1) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_str_trim_end(l %s)\n",
                        string_cstr(result), string_cstr(s));
                    return result;
                }

                /* Handle str_to_upper(s) -> String */
                if (strcmp(fn_name, "str_to_upper") == 0 && call->args->len == 1) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_str_to_upper(l %s)\n",
                        string_cstr(result), string_cstr(s));
                    return result;
                }

                /* Handle str_to_lower(s) -> String */
                if (strcmp(fn_name, "str_to_lower") == 0 && call->args->len == 1) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_str_to_lower(l %s)\n",
                        string_cstr(result), string_cstr(s));
                    return result;
                }

                /* Handle str_replace(s, old, new) -> String */
                if (strcmp(fn_name, "str_replace") == 0 && call->args->len == 3) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    String* old_str = codegen_expr(cg, call->args->data[1].value);
                    String* new_str = codegen_expr(cg, call->args->data[2].value);
                    emit(cg, "    %s =l call $fern_str_replace(l %s, l %s, l %s)\n",
                        string_cstr(result), string_cstr(s), string_cstr(old_str), string_cstr(new_str));
                    return result;
                }

                /* Handle str_repeat(s, n) -> String */
                if (strcmp(fn_name, "str_repeat") == 0 && call->args->len == 2) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    String* n = codegen_expr(cg, call->args->data[1].value);
                    emit(cg, "    %s =l call $fern_str_repeat(l %s, l %s)\n",
                        string_cstr(result), string_cstr(s), string_cstr(n));
                    return result;
                }

                /* Handle str_is_empty(s) -> Bool */
                if (strcmp(fn_name, "str_is_empty") == 0 && call->args->len == 1) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_str_is_empty(l %s)\n",
                        string_cstr(result), string_cstr(s));
                    return result;
                }

                /* Handle str_is_decimal(s) -> Bool */
                if (strcmp(fn_name, "str_is_decimal") == 0 && call->args->len == 1) {
                    String* s = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_str_is_decimal(l %s)\n",
                        string_cstr(result), string_cstr(s));
                    return result;
                }

                /* ===== Additional List Functions ===== */

                /* Handle list_push(list, elem) -> List */
                if (strcmp(fn_name, "list_push") == 0 && call->args->len == 2) {
                    String* list = codegen_expr(cg, call->args->data[0].value);
                    String* elem = codegen_expr(cg, call->args->data[1].value);
                    char elem_type = qbe_type_for_expr(cg, call->args->data[1].value);
                    emit(cg, "    %s =l call $fern_list_push(l %s, %c %s)\n",
                        string_cstr(result), string_cstr(list), elem_type, string_cstr(elem));
                    return result;
                }

                /* Handle list_reverse(list) -> List */
                if (strcmp(fn_name, "list_reverse") == 0 && call->args->len == 1) {
                    String* list = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_list_reverse(l %s)\n",
                        string_cstr(result), string_cstr(list));
                    return result;
                }

                /* Handle list_concat(a, b) -> List */
                if (strcmp(fn_name, "list_concat") == 0 && call->args->len == 2) {
                    String* a = codegen_expr(cg, call->args->data[0].value);
                    String* b = codegen_expr(cg, call->args->data[1].value);
                    emit(cg, "    %s =l call $fern_list_concat(l %s, l %s)\n",
                        string_cstr(result), string_cstr(a), string_cstr(b));
                    return result;
                }

                /* Handle list_head(list) -> elem (returns pointer type) */
                if (strcmp(fn_name, "list_head") == 0 && call->args->len == 1) {
                    String* list = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_list_head(l %s)\n",
                        string_cstr(result), string_cstr(list));
                    register_wide_var(cg, result);
                    return result;
                }

                /* Handle list_tail(list) -> List */
                if (strcmp(fn_name, "list_tail") == 0 && call->args->len == 1) {
                    String* list = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_list_tail(l %s)\n",
                        string_cstr(result), string_cstr(list));
                    return result;
                }

                /* Handle list_is_empty(list) -> Bool */
                if (strcmp(fn_name, "list_is_empty") == 0 && call->args->len == 1) {
                    String* list = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_list_is_empty(l %s)\n",
                        string_cstr(result), string_cstr(list));
                    return result;
                }

                /* ===== File I/O Functions ===== */

                /* Handle read_file(path) -> Result(String, Int) */
                if (strcmp(fn_name, "read_file") == 0 && call->args->len == 1) {
                    String* path = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_read_file(l %s)\n",
                        string_cstr(result), string_cstr(path));
                    return result;
                }

                /* Handle write_file(path, contents) -> Result(Int, Int) */
                if (strcmp(fn_name, "write_file") == 0 && call->args->len == 2) {
                    String* path = codegen_expr(cg, call->args->data[0].value);
                    String* contents = codegen_expr(cg, call->args->data[1].value);
                    emit(cg, "    %s =l call $fern_write_file(l %s, l %s)\n",
                        string_cstr(result), string_cstr(path), string_cstr(contents));
                    return result;
                }

                /* Handle append_file(path, contents) -> Result(Int, Int) */
                if (strcmp(fn_name, "append_file") == 0 && call->args->len == 2) {
                    String* path = codegen_expr(cg, call->args->data[0].value);
                    String* contents = codegen_expr(cg, call->args->data[1].value);
                    emit(cg, "    %s =l call $fern_append_file(l %s, l %s)\n",
                        string_cstr(result), string_cstr(path), string_cstr(contents));
                    return result;
                }

                /* Handle file_exists(path) -> Bool */
                if (strcmp(fn_name, "file_exists") == 0 && call->args->len == 1) {
                    String* path = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_file_exists(l %s)\n",
                        string_cstr(result), string_cstr(path));
                    return result;
                }

                /* Handle delete_file(path) -> Result(Int, Int) */
                if (strcmp(fn_name, "delete_file") == 0 && call->args->len == 1) {
                    String* path = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_delete_file(l %s)\n",
                        string_cstr(result), string_cstr(path));
                    return result;
                }

                /* Handle file_size(path) -> Result(Int, Int) */
                if (strcmp(fn_name, "file_size") == 0 && call->args->len == 1) {
                    String* path = codegen_expr(cg, call->args->data[0].value);
                    emit(cg, "    %s =l call $fern_file_size(l %s)\n",
                        string_cstr(result), string_cstr(path));
                    return result;
                }
            }

            /* Generate code for arguments and track their types */
            String** arg_temps = NULL;
            char* arg_types = NULL;
            if (call->args->len > 0) {
                arg_temps = arena_alloc(cg->arena, sizeof(String*) * call->args->len);
                arg_types = arena_alloc(cg->arena, sizeof(char) * call->args->len);
                for (size_t i = 0; i < call->args->len; i++) {
                    arg_temps[i] = codegen_expr(cg, call->args->data[i].value);
                    arg_types[i] = qbe_type_for_expr(cg, call->args->data[i].value);
                }
            }
            
            /* Generate call */
            /* For now, assume func is a simple identifier */
            if (call->func->type == EXPR_IDENT) {
                const char* func_name = string_cstr(call->func->data.ident.name);
                /* Check if function returns a tuple (pointer type) */
                char ret_type = expr->checked_type ? qbe_type_for_expr(cg, expr) :
                    (is_tuple_return_func(cg, func_name) ? 'l' : 'w');
                if (ret_type == 'l') {
                    register_wide_var(cg, result);
                }
                emit(cg, "    %s =%c call %c%s(",
                    string_cstr(result), ret_type,
                    is_function_var(cg, call->func->data.ident.name) ? '%' : '$', func_name);
                for (size_t i = 0; i < call->args->len; i++) {
                    if (i > 0) emit(cg, ", ");
                    emit(cg, "%c %s", arg_types[i], string_cstr(arg_temps[i]));
                }
                emit(cg, ")\n");
            } else {
                char ret_type = qbe_type_for_expr(cg, expr);
                String* callee = codegen_expr(cg, call->func);
                if (ret_type == 'l') {
                    register_wide_var(cg, result);
                }
                emit(cg, "    %s =%c call %s(",
                    string_cstr(result), ret_type, string_cstr(callee));
                for (size_t i = 0; i < call->args->len; i++) {
                    if (i > 0) emit(cg, ", ");
                    emit(cg, "%c %s", arg_types[i], string_cstr(arg_temps[i]));
                }
                emit(cg, ")\n");
            }
            
            return result;
        }
        
        case EXPR_TUPLE: {
            TupleExpr* tuple = &expr->data.tuple;
            size_t num_elements = tuple->elements->len;
            
            /* Allocate tuple on heap: 8 bytes per element (64-bit aligned) */
            String* tuple_ptr = fresh_temp(cg);
            size_t tuple_size = num_elements * 8;
            emit(cg, "    %s =l call $fern_alloc(l %zu)\n", string_cstr(tuple_ptr), tuple_size);
            register_wide_var(cg, tuple_ptr);
            
            /* Store each element at its offset */
            for (size_t i = 0; i < num_elements; i++) {
                Expr* elem_expr = tuple->elements->data[i];
                String* elem = codegen_expr(cg, elem_expr);
                char elem_type = qbe_type_for_expr(cg, elem_expr);
                
                String* addr = fresh_temp(cg);
                size_t offset = i * 8;
                emit(cg, "    %s =l add %s, %zu\n", string_cstr(addr), string_cstr(tuple_ptr), offset);
                
                /* Store based on element type */
                if (elem_type == 'l') {
                    emit(cg, "    storel %s, %s\n", string_cstr(elem), string_cstr(addr));
                } else {
                    /* Store word as 64-bit for uniform access */
                    String* extended = fresh_temp(cg);
                    emit(cg, "    %s =l extsw %s\n", string_cstr(extended), string_cstr(elem));
                    emit(cg, "    storel %s, %s\n", string_cstr(extended), string_cstr(addr));
                }
            }
            
            return tuple_ptr;
        }
        
        case EXPR_LIST: {
            ListExpr* list = &expr->data.list;
            String* list_ptr = fresh_temp(cg);
            
            /* Create a new list via runtime */
            emit(cg, "    %s =l call $fern_list_new()\n", string_cstr(list_ptr));
            
            /* Push each element to the list (mutating for construction) */
            for (size_t i = 0; i < list->elements->len; i++) {
                Expr* elem_expr = list->elements->data[i];
                String* elem = codegen_expr(cg, elem_expr);
                char elem_type = qbe_type_for_expr(cg, elem_expr);
                emit(cg, "    call $fern_list_push_mut(l %s, %c %s)\n",
                    string_cstr(list_ptr), elem_type, string_cstr(elem));
            }
            
            return list_ptr;
        }
        
        case EXPR_LAMBDA: {
            LambdaExpr* lambda = &expr->data.lambda;
            
            /* Generate a unique function name for this lambda */
            int lambda_id = cg->string_counter++;
            char fn_name_buf[64];
            snprintf(fn_name_buf, sizeof(fn_name_buf), "$lambda%d", lambda_id);
            
            /* Save current output and start new function */
            String* saved_output = cg->output;
            cg->output = string_new(cg->arena, "");
            
            /* Function header */
            emit(cg, "function l %s(", fn_name_buf);
            
            /* Parameters - use 'l' (64-bit) for all since runtime uses int64_t
             * Note: We do NOT call register_wide_var here because that would mark
             * integer params as pointer types, breaking arithmetic operations.
             * The 'l' type is used for ABI compatibility with int64_t. */
            if (lambda->params) {
                for (size_t i = 0; i < lambda->params->len; i++) {
                    if (i > 0) emit(cg, ", ");
                    emit(cg, "l %%%s", string_cstr(lambda->params->data[i]));
                }
            }
            
            emit(cg, ") {\n");
            emit(cg, "@start\n");
            
            /* Function body */
            String* result = codegen_expr(cg, lambda->body);
            emit(cg, "    ret %s\n", string_cstr(result));
            
            emit(cg, "}\n\n");
            
            /* Prepend the lambda function to saved output */
            String* lambda_fn = cg->output;
            cg->output = string_concat(cg->arena, lambda_fn, saved_output);
            
            /* Return a pointer to the function */
            String* tmp = fresh_temp(cg);
            emit(cg, "    %s =l copy %s\n", string_cstr(tmp), fn_name_buf);
            return tmp;
        }
        
        case EXPR_TRY: {
            /* The ? operator: unwrap Result or return early with error
             * 
             * Generated code pattern:
             *   result = <operand>           # Get the Result value
             *   tag = load result            # Load the tag (offset 0)
             *   jnz tag, @err, @ok           # Branch on tag (0=Ok, 1=Err)
             * @err
             *   ret result                   # Return the error as-is
             * @ok
             *   value = load result+8        # Load the value (offset 8)
             *   ... continue with value ...
             */
            TryExpr* try_expr = &expr->data.try_expr;
            
            /* Generate code for the operand (returns a Result pointer or struct) */
            String* result_val = codegen_expr(cg, try_expr->operand);
            
            /* For now, we'll call runtime functions to check and unwrap
             * This assumes Result is passed as a pointer */
            String* is_ok = fresh_temp(cg);
            String* ok_label = fresh_label(cg);
            String* err_label = fresh_label(cg);
            String* unwrapped = fresh_temp(cg);
            
            /* Call fern_result_is_ok to check the tag */
            emit(cg, "    %s =l call $fern_result_is_ok(l %s)\n",
                string_cstr(is_ok), string_cstr(result_val));
            
            /* Branch: if Ok continue, if Err return early */
            emit(cg, "    jnz %s, %s, %s\n", 
                string_cstr(is_ok), string_cstr(ok_label), string_cstr(err_label));
            
            /* Error path: return the error */
            emit(cg, "%s\n", string_cstr(err_label));
            emit(cg, "    ret %s\n", string_cstr(result_val));
            
            /* Ok path: unwrap the value
             * Use 'l' since unwrapped value may be a pointer (String, List, etc.) */
            emit(cg, "%s\n", string_cstr(ok_label));
            emit(cg, "    %s =l call $fern_result_unwrap(l %s)\n",
                string_cstr(unwrapped), string_cstr(result_val));
            register_wide_var(cg, unwrapped);
            
            return unwrapped;
        }
        
        case EXPR_INDEX: {
            /* Index expression: list[index]
             * Calls fern_list_get(list, index) - returns l for pointer elements */
            IndexExpr* idx = &expr->data.index_expr;
            
            String* obj = codegen_expr(cg, idx->object);
            String* index = codegen_expr(cg, idx->index);
            String* result = fresh_temp(cg);
            
            /* Call runtime function to get element */
            emit(cg, "    %s =l call $fern_list_get(l %s, l %s)\n",
                string_cstr(result), string_cstr(obj), string_cstr(index));
            register_wide_var(cg, result);
            
            return result;
        }
        
        case EXPR_FOR: {
            /* For loop: for var_name in iterable: body
             * Supports both List and Range iterables.
             */
            ForExpr* for_loop = &expr->data.for_loop;
            
            /* Check if iterable is a Range expression */
            if (for_loop->iterable->type == EXPR_RANGE) {
                /* Range iteration: for i in start..end: body
                 * 
                 * Generated code pattern:
                 *   current = start
                 *   end_val = end
                 * @loop_start
                 *   cond = current < end_val (or <= for inclusive)
                 *   jnz cond, @loop_body, @loop_end
                 * @loop_body
                 *   var_name = current
                 *   <body>
                 *   current = current + 1
                 *   jmp @loop_start
                 * @loop_end
                 *   result = 0  (unit value)
                 */
                RangeExpr* range = &for_loop->iterable->data.range;
                
                /* Evaluate start and end values */
                String* start_val = codegen_expr(cg, range->start);
                String* end_val = codegen_expr(cg, range->end);
                
                /* Initialize current to start */
                String* current = fresh_temp(cg);
                emit(cg, "    %s =l copy %s\n", string_cstr(current), string_cstr(start_val));
                
                /* Generate labels */
                String* loop_start = fresh_label(cg);
                String* loop_body = fresh_label(cg);
                String* loop_end = fresh_label(cg);
                
                /* Loop start: check condition */
                emit(cg, "%s\n", string_cstr(loop_start));
                String* cond = fresh_temp(cg);
                if (range->inclusive) {
                    /* current <= end for ..= */
                    emit(cg, "    %s =w cslel %s, %s\n",
                        string_cstr(cond), string_cstr(current), string_cstr(end_val));
                } else {
                    /* current < end for .. */
                    emit(cg, "    %s =w csltl %s, %s\n",
                        string_cstr(cond), string_cstr(current), string_cstr(end_val));
                }
                emit(cg, "    jnz %s, %s, %s\n", 
                    string_cstr(cond), string_cstr(loop_body), string_cstr(loop_end));
                
                /* Loop body */
                emit(cg, "%s\n", string_cstr(loop_body));
                
                /* Bind loop variable to current value */
                emit(cg, "    %%%s =l copy %s\n",
                    string_cstr(for_loop->var_name), string_cstr(current));
                
                /* Execute body */
                codegen_expr(cg, for_loop->body);
                
                /* At the inclusive endpoint, stop before increment can wrap INT64_MAX. */
                if (range->inclusive) {
                    String* finished = fresh_temp(cg);
                    String* increment = fresh_label(cg);
                    emit(cg, "    %s =w ceql %s, %s\n", string_cstr(finished),
                        string_cstr(current), string_cstr(end_val));
                    emit(cg, "    jnz %s, %s, %s\n", string_cstr(finished),
                        string_cstr(loop_end), string_cstr(increment));
                    emit(cg, "%s\n", string_cstr(increment));
                }
                /* Increment current */
                String* new_current = fresh_temp(cg);
                emit(cg, "    %s =l add %s, 1\n", string_cstr(new_current), string_cstr(current));
                emit(cg, "    %s =l copy %s\n", string_cstr(current), string_cstr(new_current));
                
                /* Jump back to loop start */
                emit(cg, "    jmp %s\n", string_cstr(loop_start));
                
                /* Loop end: return unit value */
                emit(cg, "%s\n", string_cstr(loop_end));
                String* result = fresh_temp(cg);
                emit(cg, "    %s =w copy 0\n", string_cstr(result));
                
                return result;
            }
            
            /* List iteration: for item in list: body
             * 
             * Generated code pattern:
             *   list = <iterable>
             *   len = fern_list_len(list)
             *   idx = 0
             * @loop_start
             *   cond = idx < len
             *   jnz cond, @loop_body, @loop_end
             * @loop_body
             *   var_name = fern_list_get(list, idx)
             *   <body>
             *   idx = idx + 1
             *   jmp @loop_start
             * @loop_end
             *   result = 0  (unit value)
             */
            
            /* Generate code for the iterable (list) */
            String* list = codegen_expr(cg, for_loop->iterable);
            
            /* Get list length */
            String* len = fresh_temp(cg);
            emit(cg, "    %s =l call $fern_list_len(l %s)\n",
                string_cstr(len), string_cstr(list));
            
            /* Initialize index to 0 */
            String* idx = fresh_temp(cg);
            emit(cg, "    %s =l copy 0\n", string_cstr(idx));
            
            /* Generate labels */
            String* loop_start = fresh_label(cg);
            String* loop_body = fresh_label(cg);
            String* loop_end = fresh_label(cg);
            
            /* Loop start: check condition */
            emit(cg, "%s\n", string_cstr(loop_start));
            String* cond = fresh_temp(cg);
            emit(cg, "    %s =w csltl %s, %s\n",
                string_cstr(cond), string_cstr(idx), string_cstr(len));
            emit(cg, "    jnz %s, %s, %s\n", 
                string_cstr(cond), string_cstr(loop_body), string_cstr(loop_end));
            
            /* Loop body */
            emit(cg, "%s\n", string_cstr(loop_body));
            
            /* Get element at current index and bind to var_name
             * Use 'l' type since list elements may be pointers (strings, etc.) */
            String* elem = fresh_temp(cg);
            emit(cg, "    %s =l call $fern_list_get(l %s, l %s)\n",
                string_cstr(elem), string_cstr(list), string_cstr(idx));
            register_wide_var(cg, elem);
            emit(cg, "    %%%s =l copy %s\n", 
                string_cstr(for_loop->var_name), string_cstr(elem));
            register_wide_var(cg, for_loop->var_name);
            
            /* Execute body */
            codegen_expr(cg, for_loop->body);
            
            /* Increment index */
            String* new_idx = fresh_temp(cg);
            emit(cg, "    %s =l add %s, 1\n", string_cstr(new_idx), string_cstr(idx));
            emit(cg, "    %s =l copy %s\n", string_cstr(idx), string_cstr(new_idx));
            
            /* Jump back to loop start */
            emit(cg, "    jmp %s\n", string_cstr(loop_start));
            
            /* Loop end: return unit value */
            emit(cg, "%s\n", string_cstr(loop_end));
            String* result = fresh_temp(cg);
            emit(cg, "    %s =w copy 0\n", string_cstr(result));
            
            return result;
        }
        
        case EXPR_WITH: {
            /* With expression: with x <- f(), y <- g(x) do body [else arms]
             *
             * Generated code pattern for each binding:
             *   result = f()                  # Get Result value
             *   is_ok = fern_result_is_ok(result)
             *   jnz is_ok, @ok_label, @err_label
             * @err_label
             *   # Error path: return error or match else arms
             *   ret result (or handle else arms)
             * @ok_label
             *   x = fern_result_unwrap(result)
             *   ... continue to next binding or body ...
             */
            WithExpr* with = &expr->data.with_expr;
            String* result = fresh_temp(cg);
            String* err_label = fresh_label(cg);
            String* end_label = fresh_label(cg);
            String* failed_result = fresh_temp(cg);
            char result_type = qbe_type_for_expr(cg, expr);
            
            /* Process each binding */
            for (size_t i = 0; i < with->bindings->len; i++) {
                WithBinding* binding = &with->bindings->data[i];
                String* ok_label = fresh_label(cg);
                String* binding_err_label = fresh_label(cg);
                
                /* Evaluate the binding's value (returns a Result) */
                String* res_val = codegen_expr(cg, binding->value);
                
                /* Check if Ok */
                String* is_ok = fresh_temp(cg);
                emit(cg, "    %s =l call $fern_result_is_ok(l %s)\n",
                    string_cstr(is_ok), string_cstr(res_val));
                
                /* Branch: if Err, jump to error handling */
                emit(cg, "    jnz %s, %s, %s\n",
                    string_cstr(is_ok), string_cstr(ok_label), string_cstr(binding_err_label));

                /* Err path: capture failed result and jump to shared error handler */
                emit(cg, "%s\n", string_cstr(binding_err_label));
                emit(cg, "    %s =l copy %s\n", string_cstr(failed_result), string_cstr(res_val));
                emit(cg, "    jmp %s\n", string_cstr(err_label));
                
                /* Ok path: unwrap and bind
                 * Use 'l' since unwrapped value may be a pointer (String, List, etc.) */
                emit(cg, "%s\n", string_cstr(ok_label));
                String* unwrapped = fresh_temp(cg);
                emit(cg, "    %s =l call $fern_result_unwrap(l %s)\n",
                    string_cstr(unwrapped), string_cstr(res_val));
                register_wide_var(cg, unwrapped);
                emit(cg, "    %%%s =l copy %s\n",
                    string_cstr(binding->name), string_cstr(unwrapped));
                register_wide_var(cg, binding->name);
            }
            
            /* All bindings succeeded: evaluate do body */
            String* body_val = codegen_expr(cg, with->body);
            emit(cg, "    %s =%c copy %s\n", string_cstr(result), result_type, string_cstr(body_val));
            if (result_type == 'l') {
                register_wide_var(cg, result);
            }
            emit(cg, "    jmp %s\n", string_cstr(end_label));
            
            /* Error path */
            emit(cg, "%s\n", string_cstr(err_label));
            if (with->else_arms && with->else_arms->len > 0) {
                /* Match else arms against the failed Result value. */
                for (size_t i = 0; i < with->else_arms->len; i++) {
                    MatchArm* arm = &with->else_arms->data[i];
                    int bound_scope = cg->bound_pattern_count;
                    int wide_scope = cg->wide_var_count;
                    String* next_arm_label = fresh_label(cg);
                    String* arm_body_label = fresh_label(cg);

                    switch (arm->pattern->type) {
                        case PATTERN_WILDCARD:
                            emit(cg, "    jmp %s\n", string_cstr(arm_body_label));
                            break;

                        case PATTERN_IDENT:
                            emit(cg, "    %%%s =l copy %s\n",
                                string_cstr(arm->pattern->data.ident), string_cstr(failed_result));
                            register_wide_var(cg, arm->pattern->data.ident);
                            emit(cg, "    jmp %s\n", string_cstr(arm_body_label));
                            break;

                        case PATTERN_LIT: {
                            Expr* lit = arm->pattern->data.literal;
                            String* lit_temp = codegen_expr(cg, lit);
                            String* cmp = fresh_temp(cg);
                            emit(cg, "    %s =w ceql %s, %s\n",
                                string_cstr(cmp), string_cstr(failed_result), string_cstr(lit_temp));
                            emit(cg, "    jnz %s, %s, %s\n",
                                string_cstr(cmp), string_cstr(arm_body_label), string_cstr(next_arm_label));
                            break;
                        }

                        case PATTERN_CONSTRUCTOR: {
                            ConstructorPattern* ctor = &arm->pattern->data.constructor;
                            const char* ctor_name = string_cstr(ctor->name);

                            if (strcmp(ctor_name, "Ok") == 0) {
                                String* tag = fresh_temp(cg);
                                String* cmp = fresh_temp(cg);
                                emit(cg, "    %s =w loadw %s\n", string_cstr(tag), string_cstr(failed_result));
                                emit(cg, "    %s =w ceqw %s, 0\n", string_cstr(cmp), string_cstr(tag));
                                emit(cg, "    jnz %s, %s, %s\n",
                                    string_cstr(cmp), string_cstr(arm_body_label), string_cstr(next_arm_label));
                            } else if (strcmp(ctor_name, "Err") == 0) {
                                String* tag = fresh_temp(cg);
                                String* cmp = fresh_temp(cg);
                                emit(cg, "    %s =w loadw %s\n", string_cstr(tag), string_cstr(failed_result));
                                emit(cg, "    %s =w ceqw %s, 1\n", string_cstr(cmp), string_cstr(tag));
                                emit(cg, "    jnz %s, %s, %s\n",
                                    string_cstr(cmp), string_cstr(arm_body_label), string_cstr(next_arm_label));
                            } else {
                                emit(cg, "    # TODO: constructor %s\n", ctor_name);
                                emit(cg, "    jmp %s\n", string_cstr(arm_body_label));
                            }
                            break;
                        }

                        default:
                            emit(cg, "    # TODO: pattern type %d\n", arm->pattern->type);
                            emit(cg, "    jmp %s\n", string_cstr(arm_body_label));
                    }

                    emit(cg, "%s\n", string_cstr(arm_body_label));

                    if (arm->pattern->type == PATTERN_CONSTRUCTOR) {
                        ConstructorPattern* ctor = &arm->pattern->data.constructor;
                        const char* ctor_name = string_cstr(ctor->name);

                        if (strcmp(ctor_name, "Ok") == 0 && ctor->args && ctor->args->len > 0) {
                            Pattern* inner = ctor->args->data[0];
                            if (inner->type == PATTERN_IDENT) {
                                String* val_ptr = fresh_temp(cg);
                                String* val = fresh_temp(cg);
                                emit(cg, "    %s =l add %s, 8\n", string_cstr(val_ptr), string_cstr(failed_result));
                                emit(cg, "    %s =l loadl %s\n", string_cstr(val), string_cstr(val_ptr));
                                emit_pattern_binding(cg, inner, val, 'l');
                            }
                        } else if (strcmp(ctor_name, "Err") == 0 && ctor->args && ctor->args->len > 0) {
                            Pattern* inner = ctor->args->data[0];
                            if (inner->type == PATTERN_IDENT) {
                                String* val_ptr = fresh_temp(cg);
                                String* val = fresh_temp(cg);
                                emit(cg, "    %s =l add %s, 8\n", string_cstr(val_ptr), string_cstr(failed_result));
                                emit(cg, "    %s =l loadl %s\n", string_cstr(val), string_cstr(val_ptr));
                                emit_pattern_binding(cg, inner, val, 'l');
                            }
                        }
                    }

                    String* arm_val = codegen_expr(cg, arm->body);
                    char arm_type = qbe_type_for_expr(cg, arm->body);
                    cg->bound_pattern_count = bound_scope;
                    cg->wide_var_count = wide_scope;
                    emit(cg, "    %s =%c copy %s\n", string_cstr(result), arm_type, string_cstr(arm_val));
                    if (arm_type == 'l') {
                        register_wide_var(cg, result);
                    }
                    emit(cg, "    jmp %s\n", string_cstr(end_label));
                    emit(cg, "%s\n", string_cstr(next_arm_label));
                }

                emit(cg, "    %s =%c copy 0\n", string_cstr(result), result_type);
            } else {
                /* No else clause: propagate the failed Result like the ? operator. */
                emit(cg, "    ret %s\n", string_cstr(failed_result));
            }
            
            emit(cg, "%s\n", string_cstr(end_label));
            return result;
        }
        
        case EXPR_RECORD_UPDATE: {
            RecordUpdateExpr* update = &expr->data.record_update;
            String* base = codegen_expr(cg, update->base);
            char base_type = qbe_type_for_expr(cg, update->base);

            /* Evaluate field expressions to preserve effects, then return updated base. */
            for (size_t i = 0; i < update->fields->len; i++) {
                (void)codegen_expr(cg, update->fields->data[i].value);
            }

            String* result = fresh_temp(cg);
            emit(cg, "    %s =%c copy %s\n", string_cstr(result), base_type, string_cstr(base));
            if (base_type == 'l') {
                register_wide_var(cg, result);
            }
            return result;
        }

        case EXPR_SPAWN: {
            SpawnExpr* spawn = &expr->data.spawn_expr;
            const char* actor_name = "anonymous";
            if (spawn->func->type == EXPR_IDENT) {
                actor_name = string_cstr(spawn->func->data.ident.name);
            } else {
                /* Preserve side effects for non-identifier spawn targets. */
                (void)codegen_expr(cg, spawn->func);
            }

            String* label = fresh_string_label(cg);
            String* result = fresh_temp(cg);
            emit_data(cg, "data %s = { b \"%s\", b 0 }\n",
                string_cstr(label), actor_name);
            emit(cg, "    %s =l call $fern_actor_spawn(l %s)\n",
                string_cstr(result), string_cstr(label));
            return result;
        }

        case EXPR_SEND: {
            SendExpr* send = &expr->data.send_expr;
            String* pid = codegen_expr(cg, send->pid);
            String* msg = codegen_expr(cg, send->message);
            String* result = fresh_temp(cg);

            /* The runtime already returns a Result pointer, including send errors. */
            emit(cg, "    %s =l call $fern_actor_send(l %s, l %s)\n",
                string_cstr(result), string_cstr(pid), string_cstr(msg));
            register_wide_var(cg, result);
            return result;
        }

        case EXPR_RECEIVE: {
            ReceiveExpr* recv = &expr->data.receive_expr;

            if (recv->arms && recv->arms->len > 0) {
                return codegen_expr(cg, recv->arms->data[0].body);
            }
            if (recv->after_body) {
                return codegen_expr(cg, recv->after_body);
            }

            String* result = fresh_temp(cg);
            emit(cg, "    %s =w copy 0\n", string_cstr(result));
            return result;
        }

        case EXPR_DOT: {
            /* Field access: expr.field
             * For tuple-like structs (exec result), access numbered fields
             * Field "0" is at offset 0, "1" at offset 8, "2" at offset 16 (64-bit aligned)
             */
            DotExpr* dot = &expr->data.dot;
            Type* semantic = checked_expr_type(expr);
            if (semantic && semantic->kind == TYPE_FN && dot->object->type == EXPR_IDENT &&
                strcmp(string_cstr(dot->object->data.ident.name), "System") == 0 &&
                (strcmp(string_cstr(dot->field), "exec_args_bounded") == 0 ||
                 strcmp(string_cstr(dot->field), "write_stderr") == 0)) {
                String* pointer = fresh_temp(cg);
                emit(cg, "    %s =l copy $fern_%s\n", string_cstr(pointer), string_cstr(dot->field));
                return pointer;
            }
            String* obj = codegen_expr(cg, dot->object);
            String* result = fresh_temp(cg);
            const char* field = string_cstr(dot->field);
            
            /* Check if field is a numeric index (tuple-style access) */
            if (field[0] >= '0' && field[0] <= '9') {
                int idx = atoi(field);
                int offset = idx * 8;  /* 64-bit aligned fields */
                
                /* Load pointer at offset from struct */
                String* addr = fresh_temp(cg);
                emit(cg, "    %s =l add %s, %d\n", string_cstr(addr), string_cstr(obj), offset);
                
                /* All tuple elements are stored as 64-bit values, load as long
                 * The let binding will handle type conversion based on annotation
                 * NOTE: Don't register as wide_var - the result is just 64-bit data,
                 * it may be an Int or a pointer depending on the actual type */
                emit(cg, "    %s =l loadl %s\n", string_cstr(result), string_cstr(addr));
            } else {
                /* Named field access delegates to a runtime helper symbol. */
                String* field_label = fresh_string_label(cg);
                emit_data(cg, "data %s = { b \"%s\", b 0 }\n",
                    string_cstr(field_label), field);
                emit(cg, "    %s =l call $fern_record_get_field(l %s, l %s)\n",
                    string_cstr(result), string_cstr(obj), string_cstr(field_label));
                register_wide_var(cg, result);
            }
            
            return result;
        }
            
        default:
            {
                String* result = fresh_temp(cg);
                emit(cg, "    # unsupported expr type %d\n", expr->type);
                emit(cg, "    %s =w copy 0\n", string_cstr(result));
                return result;
            }
    }
}

/* ========== Statement Code Generation ========== */

/**
 * Generate code for a function definition.
 * @param cg The codegen context.
 * @param fn The function definition.
 */
static void codegen_fn_def(Codegen* cg, FunctionDef* fn) {
    // FERN_STYLE: allow(function-length) QBE function prologue/epilogue handling is inherently complex
    assert(cg != NULL);
    assert(fn != NULL);
    
    /* Clear defer stack, wide vars, and return flag at function start */
    clear_defers(cg);
    cg->wide_var_count = 0;
    cg->bound_pattern_count = 0;
    cg->owned_ptr_var_count = 0;
    cg->function_var_count = 0;
    cg->returned = false;
    
    /* Check if this is main() with no return type (Unit return) */
    const char* fn_name = string_cstr(fn->name);
    bool is_main = (strcmp(fn_name, "main") == 0);
    bool is_main_unit = is_main && (fn->return_type == NULL);
    
    /* Determine return type: tuples, strings, lists return 'l' (pointer), others return 'w' */
    char ret_type = 'w';
    if (fn->return_type != NULL) {
        if (fn->return_type->kind == TYPEEXPR_TUPLE ||
            fn->return_type->kind == TYPEEXPR_FUNCTION) {
            ret_type = 'l';
        } else if (fn->return_type->kind == TYPEEXPR_NAMED) {
            const char* type_name = string_cstr(fn->return_type->data.named.name);
            /* String and List are pointer types */
            if (strcmp(type_name, "String") == 0 || strcmp(type_name, "List") == 0 ||
                strcmp(type_name, "Result") == 0 || strcmp(type_name, "Option") == 0 ||
                strcmp(type_name, "Int") == 0) {
                ret_type = 'l';
            } else if (strcmp(type_name, "Float") == 0) {
                ret_type = 'd';
            }
        }
    }
    
    if (!fn->return_type && !is_main_unit) {
        ret_type = qbe_type_for_expr(cg, fn->body);
    }

    /* Function header - rename main to fern_main so C runtime can provide entry point */
    const char* emit_name = is_main ? "fern_main" : fn_name;
    emit(cg, "export function %c $%s(", ret_type, emit_name);
    
    /* Parameters */
    if (fn->params) {
        for (size_t i = 0; i < fn->params->len; i++) {
            if (i > 0) emit(cg, ", ");
            /* Determine parameter type: String, List, and tuples are pointers (l), others are words (w) */
            char param_type = 'w';
            Parameter* param = &fn->params->data[i];
            if (param->type_ann != NULL) {
                if (param->type_ann->kind == TYPEEXPR_FUNCTION) {
                    param_type = 'l';
                    register_function_var(cg, param->name);
                } else if (param->type_ann->kind == TYPEEXPR_TUPLE) {
                    /* Tuples are heap-allocated pointers */
                    param_type = 'l';
                    register_wide_var(cg, param->name);
                    register_owned_ptr_var(cg, param->name);
                } else if (param->type_ann->kind == TYPEEXPR_NAMED) {
                    const char* type_name = string_cstr(param->type_ann->data.named.name);
                    if (strcmp(type_name, "Float") == 0) param_type = 'd';
                    if (strcmp(type_name, "Int") == 0) param_type = 'l';
                    /* String, List are heap pointers; Option is packed 64-bit; Result is heap pointer */
                    if (strcmp(type_name, "String") == 0 || strcmp(type_name, "List") == 0 ||
                        strcmp(type_name, "Option") == 0 || strcmp(type_name, "Result") == 0) {
                        param_type = 'l';
                        /* Register as wide var so uses within function body use correct type */
                        register_wide_var(cg, param->name);
                        register_owned_ptr_var(cg, param->name);
                    }
                }
            }
            emit(cg, "%c %%%s", param_type, string_cstr(param->name));
        }
    }
    
    emit(cg, ") {\n");
    emit(cg, "@start\n");
    
    /* Function body */
    String* result = codegen_expr(cg, fn->body);
    const char* preserve_name = preserved_owned_ptr_name(cg, fn->body);
    
    /* Emit deferred expressions before final return */
    emit_defers(cg);
    emit_owned_ptr_drops(cg, preserve_name);
    
    /* For main() with Unit return, always return 0 (success exit code) */
    if (is_main_unit) {
        emit(cg, "    ret 0\n");
    } else {
        emit(cg, "    ret %s\n", string_cstr(result));
    }
    
    emit(cg, "}\n\n");
}

/**
 * Generate QBE IR code for a statement.
 * @param cg The codegen context.
 * @param stmt The statement to compile.
 */
void codegen_stmt(Codegen* cg, Stmt* stmt) {
    // FERN_STYLE: allow(function-length) switch over all statement types must be in one function
    assert(cg != NULL);
    assert(stmt != NULL);
    
    switch (stmt->type) {
        case STMT_LET: {
            LetStmt* let = &stmt->data.let;
            String* val = codegen_expr(cg, let->value);
            
            /* Determine QBE type - prefer type annotation if available, otherwise infer from value */
            char type_spec = 'w';
            if (let->type_ann != NULL) {
                /* Use type annotation to determine QBE type */
                if (let->type_ann->kind == TYPEEXPR_NAMED) {
                    const char* type_name = string_cstr(let->type_ann->data.named.name);
                    if (strcmp(type_name, "Int") == 0 || strcmp(type_name, "String") == 0 ||
                        strcmp(type_name, "List") == 0) {
                        type_spec = 'l';
                    }
                } else if (let->type_ann->kind == TYPEEXPR_TUPLE ||
                    let->type_ann->kind == TYPEEXPR_FUNCTION) {
                    type_spec = 'l';
                }
            } else {
                /* Fall back to inferring from value expression */
                type_spec = qbe_type_for_expr(cg, let->value);
            }
            
            /* For simple identifier patterns, just copy to a named local */
            if (let->pattern->type == PATTERN_IDENT) {
                /* Only register as wide_var if we KNOW it's a pointer type
                 * (String, List, Option, Result, Tuple). Check:
                 * 1. Type annotation indicates pointer type
                 * 2. Value expression is a literal of pointer type (tuple, list, string)
                 * 3. Value is a function call known to return pointer type
                 * DON'T register for tuple field access without annotation - could be Int */
                bool is_pointer_type = false;
                if (let->type_ann != NULL) {
                    if (let->type_ann->kind == TYPEEXPR_TUPLE) {
                        is_pointer_type = true;
                    } else if (let->type_ann->kind == TYPEEXPR_NAMED) {
                        const char* type_name = string_cstr(let->type_ann->data.named.name);
                        if (strcmp(type_name, "String") == 0 ||
                            strcmp(type_name, "List") == 0 ||
                            strcmp(type_name, "Option") == 0 ||
                            strcmp(type_name, "Result") == 0) {
                            is_pointer_type = true;
                        }
                    }
                } else {
                    /* No type annotation - check value expression type */
                    ExprType vt = let->value->type;
                    if (vt == EXPR_TUPLE || vt == EXPR_LIST || 
                        vt == EXPR_STRING_LIT || vt == EXPR_INTERP_STRING) {
                        is_pointer_type = true;
                    }
                    /* Also check for function calls that return pointer types */
                    if (vt == EXPR_CALL) {
                        /* Use qbe_type_for_expr which knows about pointer-returning functions */
                        if (qbe_type_for_expr(cg, let->value) == 'l') {
                            is_pointer_type = true;
                        }
                    }
                    /* Identifiers: check if the source is a wide var */
                    if (vt == EXPR_IDENT) {
                        if (is_wide_var(cg, let->value->data.ident.name)) {
                            is_pointer_type = true;
                        }
                    }
                    /* Binary expressions: check if string concatenation or pipe returning pointer */
                    if (vt == EXPR_BINARY) {
                        if (qbe_type_for_expr(cg, let->value) == 'l') {
                            is_pointer_type = true;
                        }
                    }
                    /* Match expressions: check if result is a string/pointer type */
                    if (vt == EXPR_MATCH || vt == EXPR_IF || vt == EXPR_BLOCK) {
                        if (qbe_type_for_expr(cg, let->value) == 'l') {
                            is_pointer_type = true;
                        }
                    }
                }
                Type* value_type = checked_expr_type(let->value);
                if (value_type && (value_type->kind == TYPE_INT || value_type->kind == TYPE_BOOL ||
                    value_type->kind == TYPE_FLOAT || value_type->kind == TYPE_UNIT || value_type->kind == TYPE_FN)) {
                    is_pointer_type = false;
                }
                if (value_type && value_type->kind == TYPE_FN) {
                    register_function_var(cg, let->pattern->data.ident);
                }
                if (is_pointer_type) {
                    register_wide_var(cg, let->pattern->data.ident);
                    register_owned_ptr_var(cg, let->pattern->data.ident);
                    type_spec = 'l';
                }
                if (is_pointer_type &&
                    let->value->type == EXPR_IDENT &&
                    is_wide_var(cg, let->value->data.ident.name)) {
                    emit(cg, "    %%%s =l call $fern_dup(l %%%s)\n",
                        string_cstr(let->pattern->data.ident),
                        string_cstr(let->value->data.ident.name));
                } else {
                    emit(cg, "    %%%s =%c copy %s\n", 
                        string_cstr(let->pattern->data.ident), type_spec, string_cstr(val));
                }
            } else {
                emit(cg, "    # TODO: pattern binding\n");
            }
            break;
        }
        
        case STMT_EXPR: {
            codegen_expr(cg, stmt->data.expr.expr);
            break;
        }
        
        case STMT_RETURN: {
            ReturnStmt* ret = &stmt->data.return_stmt;
            const char* preserve_name = NULL;
            if (ret->value) preserve_name = preserved_owned_ptr_name(cg, ret->value);
            /* Emit deferred expressions before returning */
            emit_defers(cg);
            if (ret->value) {
                String* val = codegen_expr(cg, ret->value);
                emit_owned_ptr_drops(cg, preserve_name);
                emit(cg, "    ret %s\n", string_cstr(val));
            } else {
                emit_owned_ptr_drops(cg, NULL);
                emit(cg, "    ret 0\n");
            }
            /* Mark that we've returned - no more code should be emitted
             * until we hit a new label (branch target) */
            cg->returned = true;
            break;
        }
        
        case STMT_DEFER: {
            /* Push the deferred expression onto the stack - it will be
             * emitted before any return (LIFO order) */
            DeferStmt* defer = &stmt->data.defer_stmt;
            push_defer(cg, defer->expr);
            break;
        }
        
        case STMT_FN: {
            codegen_fn_def(cg, &stmt->data.fn);
            break;
        }
        
        default:
            emit(cg, "# TODO: codegen for stmt type %d\n", stmt->type);
    }
}

/* ========== Program Code Generation ========== */

/**
 * Generate code for a complete program.
 * @param cg The codegen context.
 * @param stmts The program statements.
 */
void codegen_program(Codegen* cg, StmtVec* stmts) {
    assert(cg != NULL);
    assert(cg->arena != NULL);
    
    if (!stmts) return;
    cg->program_stmts = stmts;
    
    /* First pass: register all functions that return pointers so call sites know the return type */
    for (size_t i = 0; i < stmts->len; i++) {
        Stmt* stmt = stmts->data[i];
        if (stmt->type == STMT_FN) {
            FunctionDef* fn = &stmt->data.fn;
            if (fn->return_type != NULL) {
                bool returns_pointer = false;
                if (fn->return_type->kind == TYPEEXPR_TUPLE) {
                    returns_pointer = true;
                } else if (fn->return_type->kind == TYPEEXPR_NAMED) {
                    const char* type_name = string_cstr(fn->return_type->data.named.name);
                    if (strcmp(type_name, "String") == 0 || strcmp(type_name, "List") == 0) {
                        returns_pointer = true;
                    }
                }
                if (returns_pointer) {
                    register_tuple_return_func(cg, fn->name);
                }
            }
        }
    }
    
    /* Second pass: generate code */
    for (size_t i = 0; i < stmts->len; i++) {
        codegen_stmt(cg, stmts->data[i]);
    }
}

/* ========== Output ========== */

/**
 * Get the generated output as a string.
 * @param cg The codegen context.
 * @return The generated QBE IR.
 */
String* codegen_output(Codegen* cg) {
    assert(cg != NULL);
    assert(cg->output != NULL);
    /* Combine functions and data section */
    return string_concat(cg->arena, cg->output, cg->data_section);
}

/**
 * Write the generated code to a file.
 * @param cg The codegen context.
 * @param filename The output filename.
 * @return True on success.
 */
bool codegen_write(Codegen* cg, const char* filename) {
    assert(cg != NULL);
    assert(filename != NULL);
    
    FILE* f = fopen(filename, "w");
    if (!f) return false;
    
    /* Write combined output (functions + data section) */
    String* full_output = codegen_output(cg);
    fprintf(f, "%s", string_cstr(full_output));
    fclose(f);
    return true;
}

/**
 * Emit the generated code to a file stream.
 * @param cg The codegen context.
 * @param out The output file stream.
 */
void codegen_emit(Codegen* cg, FILE* out) {
    assert(cg != NULL);
    assert(out != NULL);
    
    /* Write combined output (functions + data section) */
    String* full_output = codegen_output(cg);
    fprintf(out, "%s", string_cstr(full_output));
}
