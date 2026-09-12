/**
 * Fern Runtime Library Implementation
 *
 * Core functions for compiled Fern programs.
 */

/* Enable POSIX/BSD functions in strict C11 mode */
#define _POSIX_C_SOURCE 200809L
#define _DEFAULT_SOURCE
#define _BSD_SOURCE

#include "fern_runtime.h"
#include "fern_gc.h"
#include "civetweb.h"
#include "linenoise.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>
#include <stdbool.h>
#include <limits.h>
#include <errno.h>
#include <fcntl.h>
#include <spawn.h>

extern char** environ;
#include <sys/stat.h>
#include <sys/wait.h>
#include <dirent.h>
#include <unistd.h>
#include <regex.h>
#include <sqlite3.h>
#include <time.h>

/* ========== I/O Functions ========== */

/**
 * Print an integer to stdout (no newline).
 * @param n The integer to print.
 */
void fern_print_int(int64_t n) {
    printf("%lld", (long long)n);
}

/**
 * Print an integer to stdout with newline.
 * @param n The integer to print.
 */
void fern_println_int(int64_t n) {
    printf("%lld\n", (long long)n);
}

/**
 * Keep variadic platform ABI details behind a fixed double argument.
 * @param n Any IEEE double, including signed zero, infinities and NaNs.
 * @return No value; write the existing round-trip format to stdout.
 */
void fern_print_float(double n) {
    printf("%.17g", n);
}

/**
 * Print one Float line through a fixed-signature native boundary.
 * @param n Any IEEE double; no finite-only precondition applies.
 * @return No value; append exactly one newline after the formatted number.
 */
void fern_println_float(double n) {
    printf("%.17g\n", n);
}

/**
 * Format a Float into bounded GC storage without exposing a variadic compiler ABI.
 * @param n Any IEEE double; use the same host format as existing Fern interpolation.
 * @return A GC-managed NUL-terminated string; allocation must succeed.
 */
char* fern_float_to_str(double n) {
    char* result = FERN_ALLOC(32);
    assert(result != NULL);
    int written = snprintf(result, 32, "%.17g", n);
    assert(written >= 0 && written < 32);
    (void)written;
    return result;
}

/**
 * Print a string to stdout (no newline).
 * @param s The null-terminated string to print.
 */
void fern_print_str(const char* s) {
    assert(s != NULL);
    printf("%s", s);
}

/**
 * Print a string to stdout with newline.
 * @param s The null-terminated string to print.
 */
void fern_println_str(const char* s) {
    assert(s != NULL);
    printf("%s\n", s);
}

/**
 * Print a boolean to stdout (no newline).
 * @param b The boolean to print (0 = false, non-zero = true).
 */
void fern_print_bool(int64_t b) {
    printf("%s", b ? "true" : "false");
}

/**
 * Print a boolean to stdout with newline.
 * @param b The boolean to print.
 */
void fern_println_bool(int64_t b) {
    printf("%s\n", b ? "true" : "false");
}

/* ========== String Functions ========== */

/**
 * Convert an integer to a string.
 * @param n The integer to convert.
 * @return Newly allocated string (caller must free).
 */
char* fern_int_to_str(int64_t n) {
    char buf[32];
    snprintf(buf, sizeof(buf), "%lld", (long long)n);
    char* result = FERN_ALLOC(strlen(buf) + 1);
    assert(result != NULL);
    strcpy(result, buf);
    return result;
}

/**
 * Convert a boolean to a string.
 * @param b The boolean (0 = false, non-zero = true).
 * @return Newly allocated string (caller must free).
 */
char* fern_bool_to_str(int64_t b) {
    const char* str = b ? "true" : "false";
    char* result = FERN_ALLOC(strlen(str) + 1);
    assert(result != NULL);
    strcpy(result, str);
    return result;
}

/**
 * Get the length of a string.
 * @param s The null-terminated string.
 * @return The length in bytes.
 */
int64_t fern_str_len(const char* s) {
    assert(s != NULL);
    return (int64_t)strlen(s);
}

/**
 * Concatenate two strings.
 * @param a First string.
 * @param b Second string.
 * @return Newly allocated concatenated string (caller must free).
 */
char* fern_str_concat(const char* a, const char* b) {
    assert(a != NULL);
    assert(b != NULL);
    size_t len_a = strlen(a);
    size_t len_b = strlen(b);
    char* result = FERN_ALLOC(len_a + len_b + 1);
    assert(result != NULL);
    memcpy(result, a, len_a);
    memcpy(result + len_a, b, len_b + 1);
    return result;
}

/**
 * Compare two strings for equality.
 * @param a First string.
 * @param b Second string.
 * @return 1 if equal, 0 otherwise.
 */
int64_t fern_str_eq(const char* a, const char* b) {
    assert(a != NULL);
    assert(b != NULL);
    return strcmp(a, b) == 0 ? 1 : 0;
}

/**
 * Check if string starts with prefix.
 * @param s The string.
 * @param prefix The prefix to check.
 * @return 1 if s starts with prefix, 0 otherwise.
 */
int64_t fern_str_starts_with(const char* s, const char* prefix) {
    assert(s != NULL);
    assert(prefix != NULL);
    size_t prefix_len = strlen(prefix);
    return strncmp(s, prefix, prefix_len) == 0 ? 1 : 0;
}

/**
 * Check if string ends with suffix.
 * @param s The string.
 * @param suffix The suffix to check.
 * @return 1 if s ends with suffix, 0 otherwise.
 */
int64_t fern_str_ends_with(const char* s, const char* suffix) {
    assert(s != NULL);
    assert(suffix != NULL);
    size_t s_len = strlen(s);
    size_t suffix_len = strlen(suffix);
    if (suffix_len > s_len) return 0;
    return strcmp(s + s_len - suffix_len, suffix) == 0 ? 1 : 0;
}

/**
 * Check if string contains substring.
 * @param s The string.
 * @param substr The substring to find.
 * @return 1 if s contains substr, 0 otherwise.
 */
int64_t fern_str_contains(const char* s, const char* substr) {
    assert(s != NULL);
    assert(substr != NULL);
    return strstr(s, substr) != NULL ? 1 : 0;
}

/**
 * Find index of substring.
 * @param s The string.
 * @param substr The substring to find.
 * @return Option: Some(index) if found, None otherwise.
 */
int64_t fern_str_index_of(const char* s, const char* substr) {
    assert(s != NULL);
    assert(substr != NULL);
    const char* found = strstr(s, substr);
    if (found == NULL) {
        return fern_option_none();
    }
    return fern_option_some((int64_t)(found - s));
}

/**
 * Validate clamped byte slice endpoints without allocation or runtime termination.
 * @param s Non-null UTF-8 string.
 * @param start Start byte index, clamped to the string.
 * @param end Exclusive end byte index, clamped after start.
 * @return 1 when both clamped endpoints are scalar boundaries, otherwise 0.
 */
int64_t fern_str_slice_is_valid(const char* s, int64_t start, int64_t end) {
    assert(s != NULL);
    size_t len = strlen(s);
    assert(len <= INT64_MAX);
    if (start < 0) start = 0;
    if (end < start) end = start;
    size_t from = (uint64_t)start > len ? len : (size_t)start;
    size_t to = (uint64_t)end > len ? len : (size_t)end;
    return ((unsigned char)s[from] & 0xc0) != 0x80 &&
           ((unsigned char)s[to] & 0xc0) != 0x80;
}

/**
 * Get substring from start to end (exclusive).
 * @param s The string.
 * @param start Start index.
 * @param end End index (exclusive).
 * @return New string containing the substring.
 */
char* fern_str_slice(const char* s, int64_t start, int64_t end) {
    assert(s != NULL);
    if (!fern_str_slice_is_valid(s, start, end)) {
        fputs("fern: runtime error: String.slice indices must be UTF-8 character boundaries\n", stderr);
        exit(1);
    }
    size_t len = strlen(s);
    
    /* Clamp indices */
    if (start < 0) start = 0;
    if (end < start) end = start;
    if ((size_t)start > len) start = (int64_t)len;
    if ((size_t)end > len) end = (int64_t)len;
    
    size_t slice_len = (size_t)(end - start);
    char* result = FERN_ALLOC(slice_len + 1);
    assert(result != NULL);
    memcpy(result, s + start, slice_len);
    result[slice_len] = '\0';
    return result;
}

/* Helper: check if character is whitespace */
static int is_whitespace(char c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r';
}

/**
 * Trim whitespace from both ends.
 * @param s The string.
 * @return New string with whitespace trimmed.
 */
char* fern_str_trim(const char* s) {
    assert(s != NULL);
    size_t len = strlen(s);
    
    /* Find start (skip leading whitespace) */
    size_t start = 0;
    while (start < len && is_whitespace(s[start])) start++;
    
    /* Find end (skip trailing whitespace) */
    size_t end = len;
    while (end > start && is_whitespace(s[end - 1])) end--;
    
    size_t new_len = end - start;
    char* result = FERN_ALLOC(new_len + 1);
    assert(result != NULL);
    memcpy(result, s + start, new_len);
    result[new_len] = '\0';
    return result;
}

/**
 * Trim whitespace from start.
 * @param s The string.
 * @return New string with leading whitespace trimmed.
 */
char* fern_str_trim_start(const char* s) {
    assert(s != NULL);
    size_t len = strlen(s);
    
    size_t start = 0;
    while (start < len && is_whitespace(s[start])) start++;
    
    size_t new_len = len - start;
    char* result = FERN_ALLOC(new_len + 1);
    assert(result != NULL);
    memcpy(result, s + start, new_len + 1);
    return result;
}

/**
 * Trim whitespace from end.
 * @param s The string.
 * @return New string with trailing whitespace trimmed.
 */
char* fern_str_trim_end(const char* s) {
    assert(s != NULL);
    size_t len = strlen(s);
    
    while (len > 0 && is_whitespace(s[len - 1])) len--;
    
    char* result = FERN_ALLOC(len + 1);
    assert(result != NULL);
    memcpy(result, s, len);
    result[len] = '\0';
    return result;
}

/**
 * Convert string to uppercase.
 * @param s The string.
 * @return New string in uppercase.
 */
char* fern_str_to_upper(const char* s) {
    assert(s != NULL);
    size_t len = strlen(s);
    char* result = FERN_ALLOC(len + 1);
    assert(result != NULL);
    
    for (size_t i = 0; i <= len; i++) {
        char c = s[i];
        if (c >= 'a' && c <= 'z') {
            result[i] = c - 'a' + 'A';
        } else {
            result[i] = c;
        }
    }
    return result;
}

/**
 * Convert string to lowercase.
 * @param s The string.
 * @return New string in lowercase.
 */
char* fern_str_to_lower(const char* s) {
    assert(s != NULL);
    size_t len = strlen(s);
    char* result = FERN_ALLOC(len + 1);
    assert(result != NULL);
    
    for (size_t i = 0; i <= len; i++) {
        char c = s[i];
        if (c >= 'A' && c <= 'Z') {
            result[i] = c - 'A' + 'a';
        } else {
            result[i] = c;
        }
    }
    return result;
}

/**
 * Replace all occurrences of old with new.
 * @param s The string.
 * @param old_str Substring to replace.
 * @param new_str Replacement string.
 * @return New string with replacements.
 */
char* fern_str_replace(const char* s, const char* old_str, const char* new_str) {
    assert(s != NULL);
    assert(old_str != NULL);
    assert(new_str != NULL);
    
    size_t old_len = strlen(old_str);
    size_t new_len = strlen(new_str);
    
    /* Empty old_str: return copy of s */
    if (old_len == 0) {
        char* result = FERN_ALLOC(strlen(s) + 1);
        assert(result != NULL);
        strcpy(result, s);
        return result;
    }
    
    /* Count occurrences */
    size_t count = 0;
    const char* p = s;
    while ((p = strstr(p, old_str)) != NULL) {
        count++;
        p += old_len;
    }
    
    /* Calculate new length */
    size_t s_len = strlen(s);
    size_t result_len = s_len + count * (new_len - old_len);
    char* result = FERN_ALLOC(result_len + 1);
    assert(result != NULL);
    
    /* Build result */
    char* dst = result;
    p = s;
    const char* found;
    while ((found = strstr(p, old_str)) != NULL) {
        size_t prefix_len = (size_t)(found - p);
        memcpy(dst, p, prefix_len);
        dst += prefix_len;
        memcpy(dst, new_str, new_len);
        dst += new_len;
        p = found + old_len;
    }
    strcpy(dst, p);
    
    return result;
}

/**
 * Decode the width of one valid Unicode scalar without reading beyond remaining.
 * @param text Non-null bytes at a prospective scalar boundary.
 * @param remaining Positive number of bytes available.
 * @return Scalar width from 1 to 4, or 0 for malformed UTF-8.
 */
static size_t fern_utf8_scalar_width(const unsigned char* text, size_t remaining) {
    assert(text != NULL);
    assert(remaining > 0);
    unsigned char first = text[0];
    if (first < 0x80) return 1;
    size_t width = first >= 0xc2 && first <= 0xdf ? 2 :
                   first >= 0xe0 && first <= 0xef ? 3 :
                   first >= 0xf0 && first <= 0xf4 ? 4 : 0;
    if (width == 0 || width > remaining) return 0;
    for (size_t i = 1; i < width; i++) {
        if ((text[i] & 0xc0) != 0x80) return 0;
    }
    if ((first == 0xe0 && text[1] < 0xa0) ||
        (first == 0xed && text[1] >= 0xa0) ||
        (first == 0xf0 && text[1] < 0x90) ||
        (first == 0xf4 && text[1] >= 0x90)) return 0;
    return width;
}

/**
 * Validate input before empty-delimiter splitting without allocation or termination.
 * @param s Non-null source bytes.
 * @param delim Non-null delimiter; nonempty delimiters retain byte splitting.
 * @return 1 for permitted input, or 0 for malformed scalar input with empty delimiter.
 */
int64_t fern_str_split_is_valid(const char* s, const char* delim) {
    assert(s != NULL);
    assert(delim != NULL);
    if (*delim != '\0') return 1;
    size_t length = strlen(s);
    for (size_t offset = 0; offset < length;) {
        size_t width = fern_utf8_scalar_width((const unsigned char*)s + offset, length - offset);
        if (width == 0) return 0;
        offset += width;
    }
    return 1;
}

/**
 * Append a copied byte span to the split result, growing capacity when needed.
 * @param list Initialized string list with positive capacity.
 * @param text Non-null source bytes.
 * @param length Byte count to copy.
 */
static void fern_split_append(FernStringList* list, const char* text, size_t length) {
    assert(list != NULL && list->cap > 0);
    assert(text != NULL);
    if (list->len >= list->cap) {
        if ((uint64_t)list->cap > SIZE_MAX / sizeof(char*) / 2 || list->cap > INT64_MAX / 2) {
            fputs("fern: runtime error: string list size limit exceeded\n", stderr);
            exit(1);
        }
        list->cap *= 2;
        list->data = FERN_REALLOC(list->data, (size_t)list->cap * sizeof(char*));
        assert(list->data != NULL);
    }
    char* part = FERN_ALLOC(length + 1);
    assert(part != NULL);
    memcpy(part, text, length);
    part[length] = '\0';
    list->data[list->len++] = part;
}

/**
 * Split string by delimiter, using Unicode scalars for an empty delimiter.
 * @param s Non-null UTF-8 string.
 * @param delim Non-null delimiter.
 * @return New string list; empty input and delimiter produce an empty list.
 */
FernStringList* fern_str_split(const char* s, const char* delim) {
    assert(s != NULL);
    assert(delim != NULL);
    FernStringList* list = fern_rc_alloc(sizeof(FernStringList), FERN_RC_TYPE_STRING_LIST);
    assert(list != NULL);
    list->cap = 8;
    list->len = 0;
    list->data = FERN_ALLOC((size_t)list->cap * sizeof(char*));
    assert(list->data != NULL);
    size_t delim_len = strlen(delim);
    size_t length = strlen(s);
    if (delim_len == 0) {
        for (size_t offset = 0; offset < length;) {
            size_t width = fern_utf8_scalar_width((const unsigned char*)s + offset, length - offset);
            if (width == 0) {
                fputs("fern: runtime error: String.split requires valid UTF-8 input\n", stderr);
                exit(1);
            }
            fern_split_append(list, s + offset, width);
            offset += width;
        }
        return list;
    }
    const char* part = s;
    for (size_t count = 0; count < length; count++) {
        const char* found = strstr(part, delim);
        if (found == NULL) break;
        fern_split_append(list, part, (size_t)(found - part));
        part = found + delim_len;
    }
    fern_split_append(list, part, strlen(part));
    return list;
}

/**
 * Split string into lines.
 * Handles \n, \r\n, and \r line endings.
 * @param s The string.
 * @return List of lines (as FernStringList).
 */
FernStringList* fern_str_lines(const char* s) {
    assert(s != NULL);
    
    FernStringList* list = fern_rc_alloc(sizeof(FernStringList), FERN_RC_TYPE_STRING_LIST);
    assert(list != NULL);
    list->cap = 8;
    list->len = 0;
    list->data = FERN_ALLOC((size_t)list->cap * sizeof(char*));
    assert(list->data != NULL);
    
    const char* p = s;
    const char* line_start = s;
    
    while (*p != '\0') {
        if (*p == '\n' || *p == '\r') {
            /* End of line found - add the line */
            if (list->len >= list->cap) {
                list->cap *= 2;
                list->data = FERN_REALLOC(list->data, (size_t)list->cap * sizeof(char*));
                assert(list->data != NULL);
            }
            size_t line_len = (size_t)(p - line_start);
            char* line = FERN_ALLOC(line_len + 1);
            assert(line != NULL);
            memcpy(line, line_start, line_len);
            line[line_len] = '\0';
            list->data[list->len++] = line;
            
            /* Skip line ending - handle \r\n as single newline */
            if (*p == '\r' && *(p + 1) == '\n') {
                p += 2;
            } else {
                p++;
            }
            line_start = p;
        } else {
            p++;
        }
    }
    
    /* Add remaining content as last line (even if empty after final newline) */
    if (p > line_start || list->len == 0) {
        if (list->len >= list->cap) {
            list->cap *= 2;
            list->data = FERN_REALLOC(list->data, (size_t)list->cap * sizeof(char*));
            assert(list->data != NULL);
        }
        size_t line_len = (size_t)(p - line_start);
        char* line = FERN_ALLOC(line_len + 1);
        assert(line != NULL);
        memcpy(line, line_start, line_len);
        line[line_len] = '\0';
        list->data[list->len++] = line;
    }
    
    return list;
}

/**
 * Join list of strings with separator.
 * @param list The string list.
 * @param sep The separator.
 * @return New joined string.
 */
char* fern_str_join(FernStringList* list, const char* sep) {
    assert(list != NULL);
    assert(sep != NULL);
    
    if (list->len == 0) {
        char* result = FERN_ALLOC(1);
        assert(result != NULL);
        result[0] = '\0';
        return result;
    }
    
    /* Calculate total length */
    size_t sep_len = strlen(sep);
    size_t total = 0;
    for (int64_t i = 0; i < list->len; i++) {
        total += strlen(list->data[i]);
        if (i < list->len - 1) total += sep_len;
    }
    
    char* result = FERN_ALLOC(total + 1);
    assert(result != NULL);
    
    char* dst = result;
    for (int64_t i = 0; i < list->len; i++) {
        size_t part_len = strlen(list->data[i]);
        memcpy(dst, list->data[i], part_len);
        dst += part_len;
        if (i < list->len - 1) {
            memcpy(dst, sep, sep_len);
            dst += sep_len;
        }
    }
    *dst = '\0';
    
    return result;
}

/**
 * Repeat string n times.
 * @param s The string.
 * @param n Number of repetitions.
 * @return New string, with at most 16 MiB of content; nonpositive n yields empty.
 * Oversized requests terminate with a diagnostic before allocation. Rust-generated
 * callers validate first so their function-owned deferred cleanup still executes.
 */
char* fern_str_repeat(const char* s, int64_t n) {
    assert(s != NULL);
    
    if (n <= 0 || s[0] == '\0') {
        char* result = FERN_ALLOC(1);
        assert(result != NULL);
        result[0] = '\0';
        return result;
    }
    
    size_t s_len = strlen(s);
    const size_t content_limit = 16 * 1024 * 1024;
    if ((uint64_t)n > content_limit / s_len) {
        fputs("fern: runtime error: string size limit exceeded\n", stderr);
        exit(1);
    }
    size_t total = s_len * (size_t)n;
    assert(total <= content_limit);
    char* result = FERN_ALLOC(total + 1);
    assert(result != NULL);
    
    char* dst = result;
    for (int64_t i = 0; i < n; i++) {
        memcpy(dst, s, s_len);
        dst += s_len;
    }
    *dst = '\0';
    
    return result;
}

/**
 * Get character at index.
 * @param s The string.
 * @param index The index.
 * @return Option: Some(char as int) if valid index, None otherwise.
 */
int64_t fern_str_char_at(const char* s, int64_t index) {
    assert(s != NULL);
    
    if (index < 0) return fern_option_none();
    
    size_t len = strlen(s);
    if ((size_t)index >= len) return fern_option_none();
    
    return fern_option_some((int64_t)(unsigned char)s[index]);
}

/**
 * Check if string is empty.
 * @param s The string.
 * @return 1 if empty, 0 otherwise.
 */
int64_t fern_str_is_empty(const char* s) {
    assert(s != NULL);
    return s[0] == '\0' ? 1 : 0;
}

/**
 * Free a string list.
 * @param list The list to free.
 */
void fern_str_list_FERN_FREE(FernStringList* list) {
    if (list != NULL) {
        for (int64_t i = 0; i < list->len; i++) {
            FERN_FREE(list->data[i]);
        }
        FERN_FREE(list->data);
        FERN_FREE(list);
    }
}

/* ========== List Functions ========== */

/**
 * Create a new empty list.
 * @return Pointer to new list.
 */
FernList* fern_list_new(void) {
    return fern_list_with_capacity(8);
}

/**
 * Create a list with given capacity.
 * @param cap Initial capacity.
 * @return Pointer to new list.
 */
FernList* fern_list_with_capacity(int64_t cap) {
    assert(cap > 0);
    FernList* list = fern_rc_alloc(sizeof(FernList), FERN_RC_TYPE_LIST);
    assert(list != NULL);
    list->data = FERN_ALLOC((size_t)cap * sizeof(int64_t));
    assert(list->data != NULL);
    list->len = 0;
    list->cap = cap;
    return list;
}

/**
 * Get the length of a list.
 * @param list The list.
 * @return Number of elements.
 */
int64_t fern_list_len(FernList* list) {
    assert(list != NULL);
    return list->len;
}

/**
 * Get element at index.
 * @param list The list.
 * @param index The index (0-based).
 * @return The element value.
 */
int64_t fern_list_get(FernList* list, int64_t index) {
    assert(list != NULL);
    if (index < 0 || index >= list->len) {
        fputs("fern: runtime error: list index out of bounds\n", stderr);
        exit(1);
    }
    assert(index >= 0 && index < list->len);
    return list->data[index];
}

/**
 * Append an element to a list in place (mutates the list).
 * Used for list literal construction.
 * @param list The list to modify.
 * @param value The value to append.
 */
void fern_list_push_mut(FernList* list, int64_t value) {
    assert(list != NULL);

    /* Grow if needed */
    if (list->len >= list->cap) {
        int64_t new_cap = list->cap * 2;
        if (new_cap < 8) new_cap = 8;
        list->data = FERN_REALLOC(list->data, (size_t)new_cap * sizeof(int64_t));
        assert(list->data != NULL);
        list->cap = new_cap;
    }
    
    list->data[list->len++] = value;
}

/**
 * Append an element to a list (returns new list).
 * @param list The original list.
 * @param value The value to append.
 * @return New list with element appended.
 */
FernList* fern_list_push(FernList* list, int64_t value) {
    assert(list != NULL);

    /* Create new list with copied data */
    int64_t new_cap = list->len + 1;
    if (new_cap < list->cap) {
        new_cap = list->cap;
    }
    FernList* new_list = fern_list_with_capacity(new_cap);
    assert(new_list != NULL);

    /* Copy existing elements */
    for (int64_t i = 0; i < list->len; i++) {
        new_list->data[i] = list->data[i];
    }
    new_list->data[list->len] = value;
    new_list->len = list->len + 1;

    return new_list;
}

/**
 * Map a function over a list.
 * @param list The list.
 * @param fn Function pointer (int64_t -> int64_t).
 * @return New list with function applied to each element.
 */
FernList* fern_list_map(FernList* list, int64_t (*fn)(int64_t)) {
    assert(list != NULL);
    assert(fn != NULL);

    FernList* new_list = fern_list_with_capacity(list->len > 0 ? list->len : 1);
    assert(new_list != NULL);

    for (int64_t i = 0; i < list->len; i++) {
        new_list->data[i] = fn(list->data[i]);
    }
    new_list->len = list->len;

    return new_list;
}

/**
 * Fold a list from left.
 * @param list The list.
 * @param init Initial accumulator value.
 * @param fn Function (acc, elem) -> acc.
 * @return Final accumulated value.
 */
int64_t fern_list_fold(FernList* list, int64_t init, int64_t (*fn)(int64_t, int64_t)) {
    assert(list != NULL);
    assert(fn != NULL);

    int64_t acc = init;
    for (int64_t i = 0; i < list->len; i++) {
        acc = fn(acc, list->data[i]);
    }
    return acc;
}

/**
 * Free a list.
 * @param list The list to free.
 */
void fern_list_FERN_FREE(FernList* list) {
    if (list != NULL) {
        FERN_FREE(list->data);
        FERN_FREE(list);
    }
}

/**
 * Filter a list with a predicate.
 * @param list The list.
 * @param pred Predicate function (int64_t -> bool as int64_t).
 * @return New list with elements where pred returns non-zero.
 */
FernList* fern_list_filter(FernList* list, int64_t (*pred)(int64_t)) {
    assert(list != NULL);
    assert(pred != NULL);

    FernList* new_list = fern_list_with_capacity(list->len > 0 ? list->len : 1);
    assert(new_list != NULL);

    for (int64_t i = 0; i < list->len; i++) {
        if (pred(list->data[i])) {
            /* Grow if needed */
            if (new_list->len >= new_list->cap) {
                new_list->cap *= 2;
                new_list->data = FERN_REALLOC(new_list->data, 
                    (size_t)new_list->cap * sizeof(int64_t));
                assert(new_list->data != NULL);
            }
            new_list->data[new_list->len++] = list->data[i];
        }
    }

    return new_list;
}

/**
 * Find first element matching predicate.
 * @param list The list.
 * @param pred Predicate function.
 * @return Option: Some(element) if found, None otherwise.
 */
int64_t fern_list_find(FernList* list, int64_t (*pred)(int64_t)) {
    assert(list != NULL);
    assert(pred != NULL);

    for (int64_t i = 0; i < list->len; i++) {
        if (pred(list->data[i])) {
            return fern_option_some(list->data[i]);
        }
    }

    return fern_option_none();
}

/**
 * Reverse a list.
 * @param list The list.
 * @return New list with elements in reverse order.
 */
FernList* fern_list_reverse(FernList* list) {
    assert(list != NULL);

    FernList* new_list = fern_list_with_capacity(list->len > 0 ? list->len : 1);
    assert(new_list != NULL);

    for (int64_t i = list->len - 1; i >= 0; i--) {
        new_list->data[list->len - 1 - i] = list->data[i];
    }
    new_list->len = list->len;

    return new_list;
}

/**
 * Concatenate two lists.
 * @param a First list.
 * @param b Second list.
 * @return New list with all elements from a followed by b.
 */
FernList* fern_list_concat(FernList* a, FernList* b) {
    assert(a != NULL);
    assert(b != NULL);

    int64_t total = a->len + b->len;
    FernList* new_list = fern_list_with_capacity(total > 0 ? total : 1);
    assert(new_list != NULL);

    for (int64_t i = 0; i < a->len; i++) {
        new_list->data[i] = a->data[i];
    }
    for (int64_t i = 0; i < b->len; i++) {
        new_list->data[a->len + i] = b->data[i];
    }
    new_list->len = total;

    return new_list;
}

/**
 * Get first element of a list.
 * @param list The list.
 * @return The first element; empty input exits 1 with a diagnostic before access.
 */
int64_t fern_list_head(FernList* list) {
    assert(list != NULL);
    if (list->len == 0) {
        fputs("fern: runtime error: head of empty list\n", stderr);
        exit(1);
    }
    assert(list->len > 0);

    return list->data[0];
}

/**
 * Get list without first element.
 * @param list The list.
 * @return New list without first element (empty if list was empty/single).
 */
FernList* fern_list_tail(FernList* list) {
    assert(list != NULL);

    if (list->len <= 1) {
        return fern_list_new();
    }

    FernList* new_list = fern_list_with_capacity(list->len - 1);
    assert(new_list != NULL);

    for (int64_t i = 1; i < list->len; i++) {
        new_list->data[i - 1] = list->data[i];
    }
    new_list->len = list->len - 1;

    return new_list;
}

/**
 * Check if list is empty.
 * @param list The list.
 * @return 1 if empty, 0 otherwise.
 */
int64_t fern_list_is_empty(FernList* list) {
    assert(list != NULL);
    return list->len == 0 ? 1 : 0;
}

/**
 * Check if list contains element (by value equality for Int).
 * @param list The list.
 * @param elem The element to find.
 * @return 1 if found, 0 otherwise.
 */
int64_t fern_list_contains(FernList* list, int64_t elem) {
    assert(list != NULL);

    for (int64_t i = 0; i < list->len; i++) {
        if (list->data[i] == elem) {
            return 1;
        }
    }
    return 0;
}

/**
 * Check if string list contains string (by string equality).
 * @param list The list of strings.
 * @param s The string to find.
 * @return 1 if found, 0 otherwise.
 */
int64_t fern_list_contains_str(FernList* list, const char* s) {
    assert(list != NULL);
    assert(s != NULL);

    for (int64_t i = 0; i < list->len; i++) {
        const char* elem = (const char*)list->data[i];
        if (elem != NULL && strcmp(elem, s) == 0) {
            return 1;
        }
    }
    return 0;
}

/**
 * Check if any element matches predicate.
 * @param list The list.
 * @param pred Predicate function.
 * @return 1 if any element matches, 0 otherwise.
 */
int64_t fern_list_any(FernList* list, int64_t (*pred)(int64_t)) {
    assert(list != NULL);
    assert(pred != NULL);

    for (int64_t i = 0; i < list->len; i++) {
        if (pred(list->data[i])) {
            return 1;
        }
    }
    return 0;
}

/**
 * Check if all elements match predicate.
 * @param list The list.
 * @param pred Predicate function.
 * @return 1 if all elements match, 0 otherwise.
 */
int64_t fern_list_all(FernList* list, int64_t (*pred)(int64_t)) {
    assert(list != NULL);
    assert(pred != NULL);

    for (int64_t i = 0; i < list->len; i++) {
        if (!pred(list->data[i])) {
            return 0;
        }
    }
    return 1;
}

/* ========== System Functions (argc/argv access) ========== */

/* Global storage for command-line arguments */
static int g_argc = 0;
static char** g_argv = NULL;

/**
 * Set command-line arguments (called from C main wrapper).
 * @param argc Argument count.
 * @param argv Argument vector.
 */
void fern_set_args(int argc, char** argv) {
    assert(argc >= 0);
    assert(argv != NULL || argc == 0);
    g_argc = argc;
    g_argv = argv;
}

/**
 * Get command-line argument count.
 * @return Number of arguments (including program name).
 */
int64_t fern_args_count(void) {
    return (int64_t)g_argc;
}

/**
 * Get command-line argument at index.
 * @param index The index (0 = program name).
 * @return The argument string, or empty string if out of bounds.
 */
char* fern_arg(int64_t index) {
    assert(g_argv != NULL || g_argc == 0);
    if (index < 0 || index >= (int64_t)g_argc) {
        /* Return empty string for out-of-bounds access */
        char* empty = FERN_ALLOC(1);
        assert(empty != NULL);
        empty[0] = '\0';
        return empty;
    }
    /* Return a copy of the argument string */
    size_t len = strlen(g_argv[index]);
    char* result = FERN_ALLOC(len + 1);
    assert(result != NULL);
    memcpy(result, g_argv[index], len + 1);
    return result;
}

/**
 * Get all command-line arguments as a string list.
 * @return FernStringList containing all arguments.
 */
FernStringList* fern_args(void) {
    assert(g_argv != NULL || g_argc == 0);
    
    FernStringList* list = fern_rc_alloc(sizeof(FernStringList), FERN_RC_TYPE_STRING_LIST);
    assert(list != NULL);
    list->len = g_argc;
    list->cap = g_argc > 0 ? g_argc : 1;
    list->data = FERN_ALLOC((size_t)list->cap * sizeof(char*));
    assert(list->data != NULL);
    
    for (int i = 0; i < g_argc; i++) {
        size_t len = strlen(g_argv[i]);
        list->data[i] = FERN_ALLOC(len + 1);
        assert(list->data[i] != NULL);
        memcpy(list->data[i], g_argv[i], len + 1);
    }
    
    return list;
}

/**
 * Exit program immediately with given code.
 * @param code Exit code.
 */
void fern_exit(int64_t code) {
    exit((int)code);
}

/* C entry point - calls user's fern_main */
extern int fern_main(void);

int main(int argc, char** argv) {
    /* Initialize the garbage collector before any allocations */
    fern_gc_init();

    fern_set_args(argc, argv);
    return fern_main();
}

/**
 * Execute a shell command and capture output.
 * @param cmd The command to execute.
 * @return FernExecResult with exit code, stdout, and stderr.
 */
FernExecResult* fern_exec(const char* cmd) {
    assert(cmd != NULL);
    
    FernExecResult* result = FERN_ALLOC(sizeof(FernExecResult));
    assert(result != NULL);
    result->exit_code = -1;
    result->stdout_str = NULL;
    result->stderr_str = NULL;
    
    /* Create temp file for stderr */
    char stderr_template[] = "/tmp/fern_stderr_XXXXXX";
    int stderr_fd = mkstemp(stderr_template);
    if (stderr_fd < 0) {
        result->stdout_str = FERN_STRDUP("");
        result->stderr_str = FERN_STRDUP("Failed to create temp file for stderr");
        return result;
    }
    close(stderr_fd);
    
    /* Build command that redirects stderr to temp file */
    size_t cmd_len = strlen(cmd) + strlen(stderr_template) + 32;
    char* full_cmd = FERN_ALLOC(cmd_len);
    assert(full_cmd != NULL);
    snprintf(full_cmd, cmd_len, "{ %s; } 2>%s", cmd, stderr_template);
    
    /* Execute command and capture stdout */
    FILE* pipe = popen(full_cmd, "r");
    FERN_FREE(full_cmd);
    
    if (!pipe) {
        result->stdout_str = FERN_STRDUP("");
        result->stderr_str = FERN_STRDUP("Failed to execute command");
        unlink(stderr_template);
        return result;
    }
    
    /* Read stdout */
    size_t stdout_cap = 4096;
    size_t stdout_len = 0;
    char* stdout_buf = FERN_ALLOC(stdout_cap);
    assert(stdout_buf != NULL);
    
    char buf[1024];
    while (fgets(buf, sizeof(buf), pipe) != NULL) {
        size_t buf_len = strlen(buf);
        if (stdout_len + buf_len >= stdout_cap) {
            stdout_cap *= 2;
            stdout_buf = FERN_REALLOC(stdout_buf, stdout_cap);
            assert(stdout_buf != NULL);
        }
        memcpy(stdout_buf + stdout_len, buf, buf_len);
        stdout_len += buf_len;
    }
    stdout_buf[stdout_len] = '\0';
    
    int status = pclose(pipe);
    result->exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;
    result->stdout_str = stdout_buf;
    
    /* Read stderr from temp file */
    FILE* stderr_file = fopen(stderr_template, "r");
    if (stderr_file) {
        fseek(stderr_file, 0, SEEK_END);
        long stderr_size = ftell(stderr_file);
        fseek(stderr_file, 0, SEEK_SET);
        
        char* stderr_buf = FERN_ALLOC((size_t)stderr_size + 1);
        assert(stderr_buf != NULL);
        size_t read_size = fread(stderr_buf, 1, (size_t)stderr_size, stderr_file);
        stderr_buf[read_size] = '\0';
        fclose(stderr_file);
        result->stderr_str = stderr_buf;
    } else {
        result->stderr_str = FERN_STRDUP("");
    }
    
    unlink(stderr_template);
    return result;
}

/**
 * Create an unlinked capture file above the standard descriptor range.
 * @return A private seekable stream, or NULL on I/O failure.
 */
static FILE* fern_exec_capture(void) {
    FILE* initial = tmpfile();
    if (initial == NULL) return NULL;
    assert(fileno(initial) >= 0);
    int descriptor = fcntl(fileno(initial), F_DUPFD_CLOEXEC, 3);
    fclose(initial);
    if (descriptor < 0) return NULL;
    assert(descriptor >= 3);
    FILE* stream = fdopen(descriptor, "w+");
    if (stream == NULL) close(descriptor);
    return stream;
}

/**
 * Read a completed capture stream with checked size and seek operations.
 * @param stream Valid capture file. @return Captured bytes or an empty string on I/O error.
 */
static char* fern_exec_read_capture(FILE* stream) {
    assert(stream != NULL);
    assert(fileno(stream) >= 0);
    if (fseek(stream, 0, SEEK_END) != 0) return FERN_STRDUP("");
    long length = ftell(stream);
    if (length < 0 || (uintmax_t)length >= SIZE_MAX) return FERN_STRDUP("");
    if (fseek(stream, 0, SEEK_SET) != 0) return FERN_STRDUP("");
    char* text = FERN_ALLOC((size_t)length + 1);
    assert(text != NULL);
    size_t received = fread(text, 1, (size_t)length, stream);
    text[received] = '\0';
    return text;
}

/**
 * Spawn literal argv with distinct output streams and the inherited environment.
 * @param argv NULL-terminated argument vector. @param output/error Capture streams.
 * @param child Receives the child PID. @return POSIX error code, or zero on success.
 */
static int fern_exec_spawn(char** argv, FILE* output, FILE* error, pid_t* child) {
    assert(argv != NULL);
    assert(output != NULL);
    assert(error != NULL);
    assert(child != NULL);
    posix_spawn_file_actions_t actions;
    int status = posix_spawn_file_actions_init(&actions);
    if (status != 0) return status;
    status = posix_spawn_file_actions_adddup2(&actions, fileno(output), STDOUT_FILENO);
    if (status == 0) status = posix_spawn_file_actions_adddup2(&actions, fileno(error), STDERR_FILENO);
    if (status == 0) status = posix_spawn_file_actions_addclose(&actions, fileno(output));
    if (status == 0) status = posix_spawn_file_actions_addclose(&actions, fileno(error));
    if (status == 0) status = posix_spawnp(child, argv[0], &actions, NULL, argv, environ);
    posix_spawn_file_actions_destroy(&actions);
    return status;
}

/**
 * Execute a literal argument vector without a shell or argument-string escaping.
 * @param args Valid FernStringList of command and arguments.
 * @return Owned result; exit_code is -1 for spawn/wait failure or signal termination.
 */
FernExecResult* fern_exec_args(FernStringList* args) {
    assert(args != NULL);
    assert(args->len >= 0);
    FernExecResult* result = FERN_ALLOC(sizeof(FernExecResult));
    assert(result != NULL);
    result->exit_code = -1;
    result->stdout_str = FERN_STRDUP("");
    result->stderr_str = FERN_STRDUP("No command specified");
    if (args->len == 0) return result;
    if ((uint64_t)args->len >= SIZE_MAX / sizeof(char*)) return result;
    char** argv = FERN_ALLOC(((size_t)args->len + 1) * sizeof(char*));
    assert(argv != NULL);
    for (int64_t i = 0; i < args->len; i++) {
        assert(args->data[i] != NULL);
        argv[i] = args->data[i];
    }
    argv[args->len] = NULL;
    FILE* output = fern_exec_capture();
    FILE* error = fern_exec_capture();
    if (output == NULL || error == NULL) {
        if (output != NULL) fclose(output);
        if (error != NULL) fclose(error);
        result->stderr_str = FERN_STRDUP("Failed to create process capture files");
        return result;
    }
    pid_t child;
    int spawned = fern_exec_spawn(argv, output, error, &child);
    if (spawned == 0) {
        int status;
        pid_t waited;
        do { waited = waitpid(child, &status, 0); } while (waited < 0 && errno == EINTR);
        if (waited == child && WIFEXITED(status)) result->exit_code = WEXITSTATUS(status);
        result->stdout_str = fern_exec_read_capture(output);
        result->stderr_str = fern_exec_read_capture(error);
    } else {
        result->stderr_str = FERN_STRDUP(strerror(spawned));
    }
    fclose(output);
    fclose(error);
    return result;
}

/**
 * Get environment variable.
 * @param name The variable name.
 * @return The value, or empty string if not set.
 */
char* fern_getenv(const char* name) {
    assert(name != NULL);
    const char* value = getenv(name);
    if (value == NULL) {
        char* empty = FERN_ALLOC(1);
        assert(empty != NULL);
        empty[0] = '\0';
        return empty;
    }
    return FERN_STRDUP(value);
}

/**
 * Set environment variable.
 * @param name The variable name.
 * @param value The value to set.
 * @return 0 on success, non-zero on failure.
 */
int64_t fern_setenv(const char* name, const char* value) {
    assert(name != NULL);
    assert(value != NULL);
    return setenv(name, value, 1);
}

/**
 * Get current working directory.
 * @return The current working directory path.
 */
char* fern_cwd(void) {
    char* buf = FERN_ALLOC(4096);
    assert(buf != NULL);
    if (getcwd(buf, 4096) == NULL) {
        /* Return empty string on error */
        buf[0] = '\0';
    }
    return buf;
}

/**
 * Change current working directory.
 * @param path The path to change to.
 * @return 0 on success, -1 on failure.
 */
int64_t fern_chdir(const char* path) {
    assert(path != NULL);
    return chdir(path);
}

/**
 * Get system hostname.
 * @return The hostname string.
 */
char* fern_hostname(void) {
    char* buf = FERN_ALLOC(256);
    assert(buf != NULL);
    if (gethostname(buf, 256) != 0) {
        /* Return empty string on error */
        buf[0] = '\0';
    }
    return buf;
}

/**
 * Get current username.
 * @return The username string.
 */
char* fern_user(void) {
    const char* user = getenv("USER");
    if (user == NULL) {
        user = getenv("LOGNAME");
    }
    if (user == NULL) {
        char* empty = FERN_ALLOC(1);
        assert(empty != NULL);
        empty[0] = '\0';
        return empty;
    }
    return FERN_STRDUP(user);
}

/**
 * Get home directory.
 * @return The home directory path.
 */
char* fern_home(void) {
    const char* home = getenv("HOME");
    if (home == NULL) {
        char* empty = FERN_ALLOC(1);
        assert(empty != NULL);
        empty[0] = '\0';
        return empty;
    }
    return FERN_STRDUP(home);
}

/* ========== Memory Functions ========== */

/**
 * Allocate memory through the active backend.
 * @param size Number of bytes.
 * @return Allocated pointer.
 */
static void* fern_memory_backend_alloc(size_t size) {
    return FERN_ALLOC(size);
}

/**
 * Duplicate ownership through the active backend.
 * @param ptr Pointer to duplicate.
 * @return Duplicated pointer.
 */
static void* fern_memory_backend_dup(void* ptr) {
    /* Boehm backend: tracing GC, no refcount increment required. */
    return ptr;
}

/**
 * Drop ownership through the active backend.
 * @param ptr Pointer to drop.
 */
static void fern_memory_backend_drop(void* ptr) {
    /* Boehm backend: explicit drop is a semantic no-op. */
    FERN_FREE(ptr);
}

/**
 * Compute payload offset for RC objects with max alignment.
 * @return Offset from block base to payload start.
 */
static size_t fern_rc_payload_offset(void) {
    size_t align = _Alignof(max_align_t);
    size_t header_size = sizeof(FernObjectHeader);
    size_t rem = header_size % align;
    if (rem == 0) return header_size;
    return header_size + (align - rem);
}

/**
 * Convert payload pointer to its RC header pointer.
 * @param ptr RC payload pointer.
 * @return Pointer to RC header.
 */
static FernObjectHeader* fern_rc_header_mut(void* ptr) {
    assert(ptr != NULL);
    size_t offset = fern_rc_payload_offset();
    return (FernObjectHeader*)((char*)ptr - offset);
}

/**
 * Convert payload pointer to const RC header pointer.
 * @param ptr RC payload pointer.
 * @return Pointer to const RC header.
 */
static const FernObjectHeader* fern_rc_header_const(const void* ptr) {
    assert(ptr != NULL);
    size_t offset = fern_rc_payload_offset();
    return (const FernObjectHeader*)((const char*)ptr - offset);
}

/**
 * Allocate an RC-managed payload with header metadata.
 * @param payload_size Payload bytes.
 * @param type_tag Core heap value type tag.
 * @return Payload pointer.
 */
void* fern_rc_alloc(size_t payload_size, uint16_t type_tag) {
    size_t offset = fern_rc_payload_offset();
    size_t alloc_size = offset + (payload_size > 0 ? payload_size : 1);
    char* block = fern_memory_backend_alloc(alloc_size);
    assert(block != NULL);

    FernObjectHeader* header = (FernObjectHeader*)block;
    header->refcount = 1;
    header->type_tag = type_tag;
    header->flags = FERN_RC_FLAG_UNIQUE;

    return block + offset;
}

/**
 * Duplicate an RC-managed reference.
 * @param ptr RC payload pointer.
 * @return Same payload pointer or NULL.
 */
void* fern_rc_dup(void* ptr) {
    if (ptr == NULL) return NULL;
    FernObjectHeader* header = fern_rc_header_mut(ptr);
    if (header->refcount < UINT32_MAX) {
        header->refcount += 1;
    }
    if (header->refcount > 1) {
        header->flags &= (uint16_t)(~FERN_RC_FLAG_UNIQUE);
    }
    return ptr;
}

/**
 * Drop an RC-managed reference.
 * @param ptr RC payload pointer.
 */
void fern_rc_drop(void* ptr) {
    if (ptr == NULL) return;
    FernObjectHeader* header = fern_rc_header_mut(ptr);
    if (header->refcount > 0) {
        header->refcount -= 1;
    }
    if (header->refcount == 1) {
        header->flags |= FERN_RC_FLAG_UNIQUE;
    }
    if (header->refcount == 0) {
        header->flags &= (uint16_t)(~FERN_RC_FLAG_UNIQUE);
    }
}

/**
 * Get RC refcount for a payload pointer.
 * @param ptr RC payload pointer.
 * @return Refcount or 0 for NULL.
 */
uint32_t fern_rc_refcount(const void* ptr) {
    if (ptr == NULL) return 0;
    const FernObjectHeader* header = fern_rc_header_const(ptr);
    return header->refcount;
}

/**
 * Get RC type tag for a payload pointer.
 * @param ptr RC payload pointer.
 * @return Type tag or UNKNOWN for NULL.
 */
uint16_t fern_rc_type_tag(const void* ptr) {
    if (ptr == NULL) return FERN_RC_TYPE_UNKNOWN;
    const FernObjectHeader* header = fern_rc_header_const(ptr);
    return header->type_tag;
}

/**
 * Get RC flags for a payload pointer.
 * @param ptr RC payload pointer.
 * @return Flags or NONE for NULL.
 */
uint16_t fern_rc_flags(const void* ptr) {
    if (ptr == NULL) return FERN_RC_FLAG_NONE;
    const FernObjectHeader* header = fern_rc_header_const(ptr);
    return header->flags;
}

/**
 * Set RC flags for a payload pointer.
 * @param ptr RC payload pointer.
 * @param flags Flags bitmask.
 */
void fern_rc_set_flags(void* ptr, uint16_t flags) {
    if (ptr == NULL) return;
    FernObjectHeader* header = fern_rc_header_mut(ptr);
    header->flags = flags;
}

/**
 * Allocate memory.
 * @param size Number of bytes.
 * @return Pointer to allocated memory.
 */
void* fern_alloc(size_t size) {
    void* ptr = fern_memory_backend_alloc(size);
    assert(ptr != NULL);
    return ptr;
}

/**
 * Duplicate an owned reference.
 * @param ptr Pointer to duplicate ownership for.
 * @return Duplicated pointer (or NULL).
 */
void* fern_dup(void* ptr) {
    if (ptr == NULL) return NULL;
    return fern_memory_backend_dup(ptr);
}

/**
 * Drop an owned reference.
 * @param ptr Pointer to drop ownership for.
 */
void fern_drop(void* ptr) {
    if (ptr == NULL) return;
    fern_memory_backend_drop(ptr);
}

/**
 * Free memory.
 * @param ptr Pointer to release.
 */
void fern_free(void* ptr) {
    fern_drop(ptr);
}

/* ========== Result Type ========== */

/*
 * Result encoding: heap-allocated struct to support full 64-bit values.
 * This allows Result to contain pointers (strings, lists, etc.).
 *
 * FernResult {
 *   tag: int32_t (0 = Ok, 1 = Err)
 *   value: int64_t (the ok/err value, can be a pointer)
 * }
 *
 * The Result is passed around as a pointer (int64_t).
 */

typedef struct {
    int32_t tag;
    int64_t value;
} FernResult;

#define RESULT_TAG_OK  0
#define RESULT_TAG_ERR 1

/**
 * Create an Ok result.
 * @param value The success value (can be a pointer).
 * @return Pointer to heap-allocated Result.
 */
int64_t fern_result_ok(int64_t value) {
    FernResult* result = fern_rc_alloc(sizeof(FernResult), FERN_RC_TYPE_RESULT);
    result->tag = RESULT_TAG_OK;
    result->value = value;
    return (int64_t)(intptr_t)result;
}

/**
 * Create an Err result.
 * @param value The error value.
 * @return Pointer to heap-allocated Result.
 */
int64_t fern_result_err(int64_t value) {
    FernResult* result = fern_rc_alloc(sizeof(FernResult), FERN_RC_TYPE_RESULT);
    result->tag = RESULT_TAG_ERR;
    result->value = value;
    return (int64_t)(intptr_t)result;
}

/**
 * Check if a Result is Ok.
 * @param result Pointer to Result.
 * @return 1 if Ok, 0 if Err.
 */
int64_t fern_result_is_ok(int64_t result) {
    FernResult* r = (FernResult*)(intptr_t)result;
    return r->tag == RESULT_TAG_OK ? 1 : 0;
}

/**
 * Unwrap the value from a Result.
 * @param result Pointer to Result.
 * @return The contained value (ok or err).
 */
int64_t fern_result_unwrap(int64_t result) {
    FernResult* r = (FernResult*)(intptr_t)result;
    return r->value;
}

/**
 * Map a function over an Ok value.
 * @param result The Result.
 * @param fn Function to apply if Ok.
 * @return New Result with mapped value, or original Err.
 */
int64_t fern_result_map(int64_t result, int64_t (*fn)(int64_t)) {
    assert(fn != NULL);
    if (fern_result_is_ok(result)) {
        int64_t value = fern_result_unwrap(result);
        return fern_result_ok(fn(value));
    }
    return result;
}

/**
 * Chain a function that returns Result over an Ok value.
 * @param result The Result.
 * @param fn Function to apply if Ok (returns Result).
 * @return Result from fn if Ok, or original Err.
 */
int64_t fern_result_and_then(int64_t result, int64_t (*fn)(int64_t)) {
    assert(fn != NULL);
    if (fern_result_is_ok(result)) {
        int64_t value = fern_result_unwrap(result);
        return fn(value);
    }
    return result;
}

/**
 * Get the Ok value or a default.
 * @param result The Result.
 * @param default_val Value to return if Err.
 * @return Ok value or default.
 */
int64_t fern_result_unwrap_or(int64_t result, int64_t default_val) {
    if (fern_result_is_ok(result)) {
        return fern_result_unwrap(result);
    }
    return default_val;
}

/**
 * Get the Ok value or compute a default from the error.
 * @param result The Result.
 * @param fn Function to compute default from error.
 * @return Ok value or fn(err_value).
 */
int64_t fern_result_unwrap_or_else(int64_t result, int64_t (*fn)(int64_t)) {
    assert(fn != NULL);
    if (fern_result_is_ok(result)) {
        return fern_result_unwrap(result);
    }
    return fn(fern_result_unwrap(result));
}

/* ========== Option Type ========== */

/*
 * Option encoding: heap-allocated struct to support full 64-bit values.
 * This allows Option to contain pointers (strings, lists, etc.).
 *
 * FernOption {
 *   tag: int32_t (0 = None, 1 = Some)
 *   value: int64_t (the some value, can be a pointer)
 * }
 *
 * The Option is passed around as a pointer (int64_t).
 * None is represented as NULL (0).
 */

typedef struct {
    int32_t tag;
    int64_t value;
} FernOption;

#define OPTION_TAG_NONE 0
#define OPTION_TAG_SOME 1

/**
 * Create a Some option.
 * @param value The contained value (must fit in 32 bits).
 * @return Packed 64-bit value: tag (1) in lower 32 bits, value in upper 32 bits.
 *
 * Format: [value:32][tag:32] where tag=1 for Some
 * This matches the codegen's expected Option representation.
 */
int64_t fern_option_some(int64_t value) {
    /* Pack: value in upper 32 bits, tag=1 in lower 32 bits */
    return ((value & 0xFFFFFFFF) << 32) | 1;
}

/**
 * Create a None option.
 * @return Packed value with tag=0.
 *
 * Format: [0:32][tag:32] where tag=0 for None
 */
int64_t fern_option_none(void) {
    /* Tag = 0 for None */
    return 0;
}

/**
 * Check if an Option is Some.
 * @param option Packed Option value.
 * @return 1 if Some (tag=1), 0 if None (tag=0).
 */
int64_t fern_option_is_some(int64_t option) {
    /* Check if lower 32 bits (tag) == 1 */
    return (option & 0xFFFFFFFF) == OPTION_TAG_SOME ? 1 : 0;
}

/**
 * Unwrap the value from an Option.
 * @param option Packed Option value.
 * @return The contained value (upper 32 bits).
 */
int64_t fern_option_unwrap(int64_t option) {
    assert((option & 0xFFFFFFFF) == OPTION_TAG_SOME);  /* Should not unwrap None */
    /* Extract upper 32 bits as the value */
    return (option >> 32) & 0xFFFFFFFF;
}

/**
 * Map a function over a Some value.
 * @param option The Option.
 * @param fn Function to apply if Some.
 * @return New Option with mapped value, or None.
 */
int64_t fern_option_map(int64_t option, int64_t (*fn)(int64_t)) {
    assert(fn != NULL);
    if (fern_option_is_some(option)) {
        int64_t value = fern_option_unwrap(option);
        return fern_option_some(fn(value));
    }
    return option;
}

/**
 * Get the Some value or a default.
 * @param option The Option.
 * @param default_val Value to return if None.
 * @return Some value or default.
 */
int64_t fern_option_unwrap_or(int64_t option, int64_t default_val) {
    if (fern_option_is_some(option)) {
        return fern_option_unwrap(option);
    }
    return default_val;
}

/* ========== File I/O Functions ========== */

/**
 * Check if file exists.
 * @param path The file path.
 * @return 1 if exists, 0 otherwise.
 */
int64_t fern_file_exists(const char* path) {
    assert(path != NULL);
    FILE* f = fopen(path, "r");
    if (f) {
        fclose(f);
        return 1;
    }
    return 0;
}

/**
 * Delete a file.
 * @param path The file path.
 * @return Result: Ok(0) if deleted, Err(error code) otherwise.
 */
int64_t fern_delete_file(const char* path) {
    assert(path != NULL);
    if (remove(path) == 0) {
        return fern_result_ok(0);
    }
    return fern_result_err(FERN_ERR_FILE_NOT_FOUND);
}

/**
 * Get file size in bytes.
 * @param path The file path.
 * @return Result: Ok(size) or Err(error code).
 */
int64_t fern_file_size(const char* path) {
    assert(path != NULL);
    
    FILE* f = fopen(path, "rb");
    if (!f) {
        return fern_result_err(FERN_ERR_FILE_NOT_FOUND);
    }
    
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return fern_result_err(FERN_ERR_IO);
    }
    
    long size = ftell(f);
    fclose(f);
    
    if (size < 0) {
        return fern_result_err(FERN_ERR_IO);
    }
    
    return fern_result_ok((int64_t)size);
}

/**
 * Check if path is a directory.
 * @param path The path to check.
 * @return 1 if directory, 0 otherwise.
 */
int64_t fern_is_dir(const char* path) {
    assert(path != NULL);
    struct stat st;
    if (stat(path, &st) != 0) {
        return 0;
    }
    return S_ISDIR(st.st_mode) ? 1 : 0;
}

/**
 * List directory contents.
 * @param path The directory path.
 * @return FernStringList containing entry names, or NULL on error.
 */
FernStringList* fern_list_dir(const char* path) {
    assert(path != NULL);
    int64_t result = fern_read_dir_result(path);
    assert(result != 0);
    return fern_result_is_ok(result) ? (FernStringList*)(intptr_t)fern_result_unwrap(result) : NULL;
}

/**
 * Enumerate directory names as a Result, preserving open/read/close failures.
 * @param path Non-NULL directory path; at most 1,048,576 entries are accepted.
 * @return Ok(managed StringList) or Err(file-not-found, permission, not-directory, or IO).
 */
int64_t fern_read_dir_result(const char* path) {
    assert(path != NULL);
    DIR* dir = opendir(path);
    int error = dir ? 0 : errno;
    if (!dir) {
        int code = error == ENOENT ? FERN_ERR_FILE_NOT_FOUND :
            error == ENOTDIR ? FERN_ERR_NOT_A_DIR :
            (error == EACCES || error == EPERM) ? FERN_ERR_PERMISSION : FERN_ERR_IO;
        return fern_result_err(code);
    }
    FernStringList* list = fern_rc_alloc(sizeof(FernStringList), FERN_RC_TYPE_STRING_LIST);
    assert(list != NULL);
    list->cap = 16;
    list->len = 0;
    list->data = FERN_ALLOC((size_t)list->cap * sizeof(char*));
    assert(list->data != NULL);
    error = EOVERFLOW;
    for (size_t visited = 0; visited < 1048579; visited++) {
        errno = 0;
        struct dirent* entry = readdir(dir);
        if (!entry) { error = errno; break; }
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) continue;
        if (list->len == 1048576) break;
        if (list->len == list->cap) {
            list->cap *= 2;
            list->data = FERN_REALLOC(list->data, (size_t)list->cap * sizeof(char*));
            assert(list->data != NULL);
        }
        list->data[list->len] = FERN_STRDUP(entry->d_name);
        assert(list->data[list->len] != NULL);
        list->len++;
    }
    if (closedir(dir) != 0 && error == 0) error = errno;
    if (error != 0) {
        int code = (error == EACCES || error == EPERM) ? FERN_ERR_PERMISSION : FERN_ERR_IO;
        return fern_result_err(code);
    }
    return fern_result_ok((int64_t)(intptr_t)list);
}

/* ========== Gate C Stdlib Runtime Surface ========== */

/**
 * Parse JSON text.
 * @param text UTF-8 JSON text.
 * @return Result: Ok(copy of text) or Err(error code).
 */
int64_t fern_json_parse(const char* text) {
    if (text == NULL || text[0] == '\0') {
        return fern_result_err(FERN_ERR_IO);
    }
    char* copy = FERN_STRDUP(text);
    if (copy == NULL) {
        return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    }
    return fern_result_ok((int64_t)(intptr_t)copy);
}

/**
 * Encode value as JSON text.
 * @param text Input text payload.
 * @return Result: Ok(copy of text) or Err(error code).
 */
int64_t fern_json_stringify(const char* text) {
    if (text == NULL) {
        return fern_result_err(FERN_ERR_IO);
    }
    char* copy = FERN_STRDUP(text);
    if (copy == NULL) {
        return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    }
    return fern_result_ok((int64_t)(intptr_t)copy);
}

typedef struct {
    const char* host;
    size_t host_len;
    const char* path;
    int port;
    int use_ssl;
} FernHttpUrl;

static int g_fern_http_client_initialized = 0;

/**
 * Initialize civetweb client globals once for the process lifetime.
 * @return 1 on success, 0 when initialization fails.
 */
static int fern_http_ensure_client_initialized(void) {
    if (g_fern_http_client_initialized) return 1;
    unsigned features = mg_init_library(MG_FEATURES_DEFAULT | MG_FEATURES_TLS);
    (void)features;
    g_fern_http_client_initialized = 1;
    return 1;
}

/**
 * Parse a positive TCP port from a URL segment.
 * @param start Port segment start (inclusive).
 * @param end Port segment end (exclusive).
 * @param port_out Parsed port number output.
 * @return 1 when parsing succeeds, 0 otherwise.
 */
static int fern_http_parse_port(const char* start, const char* end, int* port_out) {
    assert(start != NULL);
    assert(end != NULL);
    assert(port_out != NULL);
    if (!start || !end || !port_out || start >= end) return 0;

    size_t len = (size_t)(end - start);
    if (len >= 16) return 0;

    char buf[16];
    memcpy(buf, start, len);
    buf[len] = '\0';

    char* parse_end = NULL;
    long parsed = strtol(buf, &parse_end, 10);
    if (parse_end == NULL || *parse_end != '\0') return 0;
    if (parsed <= 0 || parsed > 65535) return 0;
    *port_out = (int)parsed;
    return 1;
}

/**
 * Parse an HTTP/HTTPS URL into request target components.
 * @param url Input URL.
 * @param parsed Parsed output target.
 * @return 1 on success, 0 on invalid URL.
 */
static int fern_http_parse_url(const char* url, FernHttpUrl* parsed) {
    assert(url != NULL);
    assert(parsed != NULL);
    if (!url || !parsed) return 0;

    const char* p = url;
    memset(parsed, 0, sizeof(*parsed));
    if (strncmp(p, "http://", 7) == 0) {
        parsed->use_ssl = 0;
        parsed->port = 80;
        p += 7;
    } else if (strncmp(p, "https://", 8) == 0) {
        parsed->use_ssl = 1;
        parsed->port = 443;
        p += 8;
    } else {
        return 0;
    }

    if (*p == '\0') return 0;

    const char* host_start = p;
    const char* host_end = p;
    if (*p == '[') {
        host_start = ++p;
        while (*p != '\0' && *p != ']') p++;
        if (*p != ']' || p == host_start) return 0;
        host_end = p;
        p++;
    } else {
        while (*p != '\0' && *p != '/' && *p != ':' && *p != '?' && *p != '#') p++;
        host_end = p;
    }

    if (host_end <= host_start) return 0;

    if (*p == ':') {
        const char* port_start = ++p;
        while (*p != '\0' && *p != '/' && *p != '?' && *p != '#') p++;
        if (!fern_http_parse_port(port_start, p, &parsed->port)) return 0;
    }

    parsed->host = host_start;
    parsed->host_len = (size_t)(host_end - host_start);
    parsed->path = (*p == '\0' || *p == '#') ? "/" : p;
    return 1;
}

/**
 * Build null-terminated host and Host header values from parsed URL.
 * @param target Parsed URL target.
 * @param host_out Host output for civetweb connect.
 * @param host_out_len Host output buffer length.
 * @param host_header_out HTTP Host header output.
 * @param host_header_out_len Host header output buffer length.
 * @return 1 on success, 0 on buffer overflow.
 */
static int fern_http_build_host_values(const FernHttpUrl* target,
                                       char* host_out,
                                       size_t host_out_len,
                                       char* host_header_out,
                                       size_t host_header_out_len) {
    assert(target != NULL);
    assert(host_out != NULL);
    assert(host_header_out != NULL);
    if (!target || !host_out || !host_header_out) return 0;

    if (target->host_len + 1 > host_out_len) return 0;
    memcpy(host_out, target->host, target->host_len);
    host_out[target->host_len] = '\0';

    int is_default_port = (!target->use_ssl && target->port == 80) ||
                          (target->use_ssl && target->port == 443);
    if (is_default_port) {
        size_t host_len = strlen(host_out);
        if (host_len + 1 > host_header_out_len) return 0;
        memcpy(host_header_out, host_out, host_len + 1);
        return 1;
    }

    int written = snprintf(host_header_out, host_header_out_len, "%s:%d", host_out, target->port);
    if (written <= 0) return 0;
    return (size_t)written < host_header_out_len;
}

/**
 * Read the full HTTP response body from an open civetweb connection.
 * @param conn Open client connection.
 * @param content_length Declared HTTP content length (-1 if unknown).
 * @param err_out Output Fern error code on failure.
 * @return Newly allocated body string on success, NULL on failure.
 */
static char* fern_http_read_body(struct mg_connection* conn, long long content_length, int64_t* err_out) {
    assert(conn != NULL);
    assert(err_out != NULL);
    if (!conn || !err_out) return NULL;

    *err_out = FERN_ERR_IO;
    if (content_length < -1) return NULL;

    size_t cap = 1024;
    if (content_length == 0) {
        cap = 1;
    } else if (content_length > 0) {
        if ((unsigned long long)content_length >= (unsigned long long)SIZE_MAX) return NULL;
        cap = (size_t)content_length + 1;
    }

    char* body = FERN_ALLOC(cap);
    if (body == NULL) {
        *err_out = FERN_ERR_OUT_OF_MEMORY;
        return NULL;
    }

    size_t len = 0;
    while (1) {
        if (len + 1 >= cap) {
            size_t next_cap = cap * 2;
            if (next_cap <= cap) return NULL;
            char* next = FERN_REALLOC(body, next_cap);
            if (next == NULL) {
                *err_out = FERN_ERR_OUT_OF_MEMORY;
                return NULL;
            }
            body = next;
            cap = next_cap;
        }

        int nread = mg_read(conn, body + len, cap - len - 1);
        if (nread < 0) return NULL;
        if (nread == 0) break;
        len += (size_t)nread;

        if (content_length >= 0 && len >= (size_t)content_length) break;
    }

    if (content_length >= 0 && len < (size_t)content_length) {
        return NULL;
    }

    body[len] = '\0';
    return body;
}

/**
 * Execute a basic HTTP request using civetweb client APIs.
 * @param method HTTP method literal ("GET"/"POST").
 * @param url Request URL.
 * @param body Optional request body for POST; NULL for GET.
 * @return Result: Ok(response body) or Err(error code).
 */
static int64_t fern_http_request(const char* method, const char* url, const char* body) {
    assert(method != NULL);
    assert(url != NULL);
    if (!method || !url) return fern_result_err(FERN_ERR_IO);
    if (!fern_http_ensure_client_initialized()) return fern_result_err(FERN_ERR_IO);

    FernHttpUrl target;
    if (!fern_http_parse_url(url, &target)) {
        return fern_result_err(FERN_ERR_IO);
    }
    if (target.use_ssl && (mg_check_feature(MG_FEATURES_TLS) == 0u)) {
        return fern_result_err(FERN_ERR_IO);
    }

    char host[512];
    char host_header[640];
    if (!fern_http_build_host_values(&target, host, sizeof(host), host_header, sizeof(host_header))) {
        return fern_result_err(FERN_ERR_IO);
    }

    char err_buf[256];
    err_buf[0] = '\0';
    struct mg_connection* conn = NULL;
    if (body == NULL) {
        conn = mg_download(host,
                           target.port,
                           target.use_ssl,
                           err_buf,
                           sizeof(err_buf),
                           "%s %s HTTP/1.1\r\n"
                           "Host: %s\r\n"
                           "Connection: close\r\n"
                           "\r\n",
                           method,
                           target.path,
                           host_header);
    } else {
        size_t body_len = strlen(body);
        conn = mg_download(host,
                           target.port,
                           target.use_ssl,
                           err_buf,
                           sizeof(err_buf),
                           "%s %s HTTP/1.1\r\n"
                           "Host: %s\r\n"
                           "Content-Type: text/plain\r\n"
                           "Content-Length: %zu\r\n"
                           "Connection: close\r\n"
                           "\r\n"
                           "%s",
                           method,
                           target.path,
                           host_header,
                           body_len,
                           body);
    }
    if (conn == NULL) {
        return fern_result_err(FERN_ERR_IO);
    }

    const struct mg_response_info* info = mg_get_response_info(conn);
    if (info == NULL || info->status_code < 200 || info->status_code >= 300) {
        mg_close_connection(conn);
        return fern_result_err(FERN_ERR_IO);
    }

    int64_t body_err = FERN_ERR_IO;
    char* response_body = fern_http_read_body(conn, info->content_length, &body_err);
    mg_close_connection(conn);
    if (response_body == NULL) {
        return fern_result_err(body_err);
    }

    return fern_result_ok((int64_t)(intptr_t)response_body);
}

/**
 * Perform an HTTP GET request.
 * @param url Request URL.
 * @return Result: Ok(response body) or Err(error code).
 */
int64_t fern_http_get(const char* url) {
    if (url == NULL || url[0] == '\0') {
        return fern_result_err(FERN_ERR_IO);
    }
    return fern_http_request("GET", url, NULL);
}

/**
 * Perform an HTTP POST request.
 * @param url Request URL.
 * @param body Request body payload.
 * @return Result: Ok(response body) or Err(error code).
 */
int64_t fern_http_post(const char* url, const char* body) {
    if (url == NULL || url[0] == '\0' || body == NULL) {
        return fern_result_err(FERN_ERR_IO);
    }
    return fern_http_request("POST", url, body);
}

typedef struct {
    int64_t handle_id;
    sqlite3* db;
} FernSqlHandle;

typedef struct {
    FernSqlHandle* handles;
    int64_t handle_len;
    int64_t handle_cap;
    int64_t next_handle_id;
} FernSqlRuntimeState;

static FernSqlRuntimeState g_sql_runtime;

/**
 * Get SQL runtime state with lazy initialization.
 * @return Pointer to SQL runtime state.
 */
static FernSqlRuntimeState* fern_sql_runtime_state(void) {
    FernSqlRuntimeState* state = &g_sql_runtime;
    assert(state != NULL);
    if (state->next_handle_id <= 0) {
        state->next_handle_id = 1;
    }
    assert(state->next_handle_id > 0);
    return state;
}

/**
 * Ensure SQL handle storage has room for one more entry.
 * @param state SQL runtime state.
 * @return 1 on success, 0 on allocation failure.
 */
static int fern_sql_ensure_capacity(FernSqlRuntimeState* state) {
    assert(state != NULL);
    if (state->handle_len < state->handle_cap) return 1;

    int64_t next_cap = state->handle_cap == 0 ? 8 : state->handle_cap * 2;
    size_t bytes = (size_t)next_cap * sizeof(FernSqlHandle);
    FernSqlHandle* next = NULL;
    if (state->handles == NULL) {
        next = FERN_ALLOC(bytes);
    } else {
        next = FERN_REALLOC(state->handles, bytes);
    }
    if (next == NULL) {
        return 0;
    }

    state->handles = next;
    state->handle_cap = next_cap;
    return 1;
}

/**
 * Register a newly opened sqlite3 handle and return Fern handle id.
 * @param db Open sqlite3 connection.
 * @return Positive handle id on success, 0 on allocation failure.
 */
static int64_t fern_sql_register_handle(sqlite3* db) {
    assert(db != NULL);
    FernSqlRuntimeState* state = fern_sql_runtime_state();
    assert(state != NULL);
    if (!fern_sql_ensure_capacity(state)) {
        return 0;
    }

    int64_t handle_id = state->next_handle_id++;
    FernSqlHandle* slot = &state->handles[state->handle_len++];
    slot->handle_id = handle_id;
    slot->db = db;
    return handle_id;
}

/**
 * Look up sqlite3 connection by Fern handle id.
 * @param state SQL runtime state.
 * @param handle_id Fern SQL handle id.
 * @return Matching handle entry, or NULL if not found.
 */
static FernSqlHandle* fern_sql_find_handle(FernSqlRuntimeState* state, int64_t handle_id) {
    assert(state != NULL);
    assert(handle_id >= 0);
    for (int64_t i = 0; i < state->handle_len; i++) {
        FernSqlHandle* entry = &state->handles[i];
        if (entry->handle_id == handle_id) {
            return entry;
        }
    }
    return NULL;
}

/**
 * Open a SQL connection.
 * Uses SQLite backend and returns an opaque Fern handle id.
 * @param path Database path/URL.
 * @return Result: Ok(handle id) or Err(error code).
 */
int64_t fern_sql_open(const char* path) {
    if (path == NULL || path[0] == '\0') {
        return fern_result_err(FERN_ERR_IO);
    }

    sqlite3* db = NULL;
    int rc = sqlite3_open(path, &db);
    if (rc != SQLITE_OK || db == NULL) {
        if (db != NULL) sqlite3_close(db);
        return fern_result_err(FERN_ERR_IO);
    }

    int64_t handle_id = fern_sql_register_handle(db);
    if (handle_id <= 0) {
        sqlite3_close(db);
        return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    }
    return fern_result_ok(handle_id);
}

/**
 * Execute SQL against a connection.
 * Executes statement and returns sqlite3_changes() rows affected.
 * @param handle SQL handle.
 * @param query SQL statement.
 * @return Result: Ok(rows affected) or Err(error code).
 */
int64_t fern_sql_execute(int64_t handle, const char* query) {
    if (query == NULL || query[0] == '\0' || handle <= 0) {
        return fern_result_err(FERN_ERR_IO);
    }

    FernSqlRuntimeState* state = fern_sql_runtime_state();
    assert(state != NULL);
    FernSqlHandle* entry = fern_sql_find_handle(state, handle);
    if (entry == NULL || entry->db == NULL) {
        return fern_result_err(FERN_ERR_IO);
    }

    char* err_msg = NULL;
    int rc = sqlite3_exec(entry->db, query, NULL, NULL, &err_msg);
    if (err_msg != NULL) {
        sqlite3_free(err_msg);
    }
    if (rc != SQLITE_OK) {
        return fern_result_err(FERN_ERR_IO);
    }

    return fern_result_ok((int64_t)sqlite3_changes(entry->db));
}

typedef struct {
    int64_t actor_id;
    int64_t alive;
    int64_t replacement_id;
    int64_t linked_parent_id;
    int64_t supervisor_id;
    int64_t supervision_strategy;
    int64_t supervision_order;
    int64_t supervision_max_restarts;
    int64_t supervision_period_sec;
    int64_t supervision_window_start_sec;
    int64_t supervision_restart_count;
    int64_t* supervision_child_ids;
    int64_t* supervision_child_strategies;
    int64_t* supervision_child_orders;
    int64_t supervision_child_len;
    int64_t supervision_child_cap;
    int64_t supervision_next_order;
    char* name;
    int64_t* monitor_ids;
    int64_t monitor_len;
    int64_t monitor_cap;
    char** mailbox_data;
    int64_t mailbox_head;
    int64_t mailbox_len;
    int64_t mailbox_cap;
    int64_t scheduler_tokens;
    int64_t scheduler_enqueued;
    int64_t exit_child_cursor;
    int64_t exit_next_id;
} FernActorRecord;

typedef struct {
    FernActorRecord* actors;
    int64_t actor_len;
    int64_t actor_cap;
    int64_t next_actor_id;
    int64_t current_actor_id;
    int64_t clock_now_sec;
    int64_t clock_manual_mode;
    int64_t* ready_ids;
    int64_t ready_head;
    int64_t ready_len;
    int64_t ready_cap;
} FernActorRuntimeState;

static FernActorRuntimeState g_actor_runtime;

#define FERN_SUPERVISION_ONE_FOR_ONE 1
#define FERN_SUPERVISION_ONE_FOR_ALL 2
#define FERN_SUPERVISION_REST_FOR_ONE 3

/**
 * Get actor runtime state with lazy initialization.
 * @return Pointer to actor runtime state.
 */
static FernActorRuntimeState* fern_actor_runtime_state(void) {
    FernActorRuntimeState* state = &g_actor_runtime;
    assert(state != NULL);
    if (state->next_actor_id == 0) {
        state->next_actor_id = 1;
        time_t raw_now = time(NULL);
        state->clock_now_sec = raw_now > 0 ? (int64_t)raw_now : 0;
        state->clock_manual_mode = 0;
    }
    assert(state->next_actor_id > 0);
    return state;
}

/**
 * Read actor runtime clock seconds.
 * @param state Actor runtime state.
 * @return Runtime clock seconds.
 */
static int64_t fern_actor_clock_now_state(FernActorRuntimeState* state) {
    assert(state != NULL);
    if (state->clock_manual_mode) {
        return state->clock_now_sec;
    }
    time_t raw_now = time(NULL);
    return raw_now > 0 ? (int64_t)raw_now : 0;
}

/**
 * Reserve actor registry capacity for at least needed entries.
 * @param state Actor runtime state.
 * @param needed Required entry count.
 * @return 1 on success, 0 on allocation failure.
 */
static int fern_actor_registry_reserve(FernActorRuntimeState* state, int64_t needed) {
    assert(state != NULL);
    assert(needed >= 0);
    if (needed <= state->actor_cap) {
        return 1;
    }

    int64_t next_cap = state->actor_cap > 0 ? state->actor_cap : 8;
    while (next_cap < needed) {
        next_cap *= 2;
    }

    FernActorRecord* next = FERN_ALLOC((size_t)next_cap * sizeof(FernActorRecord));
    if (next == NULL) {
        return 0;
    }
    memset(next, 0, (size_t)next_cap * sizeof(FernActorRecord));

    if (state->actors != NULL && state->actor_len > 0) {
        memcpy(next, state->actors, (size_t)state->actor_len * sizeof(FernActorRecord));
    }

    state->actors = next;
    state->actor_cap = next_cap;
    return 1;
}

/**
 * Reserve ready-queue capacity for at least needed entries.
 * @param state Actor runtime state.
 * @param needed Required queue entry count.
 * @return 1 on success, 0 on allocation failure.
 */
static int fern_actor_ready_reserve(FernActorRuntimeState* state, int64_t needed) {
    assert(state != NULL);
    assert(needed >= 0);
    if (needed <= state->ready_cap) {
        return 1;
    }

    int64_t next_cap = state->ready_cap > 0 ? state->ready_cap : 8;
    while (next_cap < needed) {
        next_cap *= 2;
    }

    int64_t* next = FERN_ALLOC((size_t)next_cap * sizeof(int64_t));
    if (next == NULL) {
        return 0;
    }

    for (int64_t i = 0; i < state->ready_len; i++) {
        int64_t idx = 0;
        if (state->ready_cap > 0) {
            idx = (state->ready_head + i) % state->ready_cap;
        }
        next[i] = state->ready_ids[idx];
    }

    state->ready_ids = next;
    state->ready_cap = next_cap;
    state->ready_head = 0;
    return 1;
}

/**
 * Reserve mailbox capacity for at least needed entries.
 * @param actor Actor record.
 * @param needed Required mailbox entry count.
 * @return 1 on success, 0 on allocation failure.
 */
static int fern_actor_mailbox_reserve(FernActorRecord* actor, int64_t needed) {
    assert(actor != NULL);
    assert(needed >= 0);
    if (needed <= actor->mailbox_cap) {
        return 1;
    }

    int64_t next_cap = actor->mailbox_cap > 0 ? actor->mailbox_cap : 8;
    while (next_cap < needed) {
        next_cap *= 2;
    }

    char** next = FERN_ALLOC((size_t)next_cap * sizeof(char*));
    if (next == NULL) {
        return 0;
    }

    for (int64_t i = 0; i < actor->mailbox_len; i++) {
        int64_t idx = 0;
        if (actor->mailbox_cap > 0) {
            idx = (actor->mailbox_head + i) % actor->mailbox_cap;
        }
        next[i] = actor->mailbox_data[idx];
    }

    actor->mailbox_data = next;
    actor->mailbox_cap = next_cap;
    actor->mailbox_head = 0;
    return 1;
}

/**
 * Reserve monitor-list capacity for at least needed entries.
 * @param actor Actor record.
 * @param needed Required monitor id count.
 * @return 1 on success, 0 on allocation failure.
 */
static int fern_actor_monitor_reserve(FernActorRecord* actor, int64_t needed) {
    assert(actor != NULL);
    assert(needed >= 0);
    if (needed <= actor->monitor_cap) {
        return 1;
    }

    int64_t next_cap = actor->monitor_cap > 0 ? actor->monitor_cap : 4;
    while (next_cap < needed) {
        next_cap *= 2;
    }

    int64_t* next = FERN_ALLOC((size_t)next_cap * sizeof(int64_t));
    if (next == NULL) {
        return 0;
    }
    if (actor->monitor_ids != NULL && actor->monitor_len > 0) {
        memcpy(next, actor->monitor_ids, (size_t)actor->monitor_len * sizeof(int64_t));
    }

    actor->monitor_ids = next;
    actor->monitor_cap = next_cap;
    return 1;
}

/**
 * Reserve supervisor child-table capacity for at least needed entries.
 * @param supervisor Supervisor actor record.
 * @param needed Required child entry count.
 * @return 1 on success, 0 on allocation failure.
 */
static int fern_actor_supervision_child_reserve(FernActorRecord* supervisor, int64_t needed) {
    assert(supervisor != NULL);
    assert(needed >= 0);
    if (needed <= supervisor->supervision_child_cap) {
        return 1;
    }

    int64_t next_cap = supervisor->supervision_child_cap > 0 ? supervisor->supervision_child_cap : 4;
    while (next_cap < needed) {
        next_cap *= 2;
    }

    int64_t* next_ids = FERN_ALLOC((size_t)next_cap * sizeof(int64_t));
    int64_t* next_strategies = FERN_ALLOC((size_t)next_cap * sizeof(int64_t));
    int64_t* next_orders = FERN_ALLOC((size_t)next_cap * sizeof(int64_t));
    if (next_ids == NULL || next_strategies == NULL || next_orders == NULL) {
        return 0;
    }

    if (supervisor->supervision_child_len > 0) {
        memcpy(next_ids, supervisor->supervision_child_ids,
            (size_t)supervisor->supervision_child_len * sizeof(int64_t));
        memcpy(next_strategies, supervisor->supervision_child_strategies,
            (size_t)supervisor->supervision_child_len * sizeof(int64_t));
        memcpy(next_orders, supervisor->supervision_child_orders,
            (size_t)supervisor->supervision_child_len * sizeof(int64_t));
    }

    supervisor->supervision_child_ids = next_ids;
    supervisor->supervision_child_strategies = next_strategies;
    supervisor->supervision_child_orders = next_orders;
    supervisor->supervision_child_cap = next_cap;
    return 1;
}

/**
 * Look up actor record by actor id.
 * @param state Actor runtime state.
 * @param actor_id Actor id.
 * @return Actor record pointer, or NULL if actor id is invalid.
 */
static FernActorRecord* fern_actor_lookup(FernActorRuntimeState* state, int64_t actor_id) {
    assert(state != NULL);
    assert(state->actor_len >= 0);
    if (actor_id <= 0 || actor_id > state->actor_len) {
        return NULL;
    }
    return &state->actors[actor_id - 1];
}

/**
 * Check whether an actor record is currently alive.
 * @param actor Actor record.
 * @return 1 when alive, 0 otherwise.
 */
static int fern_actor_is_alive(const FernActorRecord* actor) {
    assert(actor != NULL);
    assert(actor->actor_id >= 0);
    return actor->alive != 0;
}

/**
 * Push actor id to ready queue.
 * @param state Actor runtime state.
 * @param actor_id Actor id.
 */
static void fern_actor_ready_push(FernActorRuntimeState* state, int64_t actor_id) {
    assert(state != NULL);
    assert(actor_id > 0);
    assert(state->ready_cap > 0);
    assert(state->ready_len < state->ready_cap);
    int64_t tail = (state->ready_head + state->ready_len) % state->ready_cap;
    state->ready_ids[tail] = actor_id;
    state->ready_len++;
}

/**
 * Pop actor id from ready queue.
 * @param state Actor runtime state.
 * @return Actor id at queue head, or 0 when queue is empty.
 */
static int64_t fern_actor_ready_pop(FernActorRuntimeState* state) {
    assert(state != NULL);
    assert(state->ready_len >= 0);
    if (state->ready_len == 0) {
        return 0;
    }

    int64_t actor_id = state->ready_ids[state->ready_head];
    state->ready_head = (state->ready_head + 1) % state->ready_cap;
    state->ready_len--;
    if (state->ready_len == 0) {
        state->ready_head = 0;
    }
    return actor_id;
}

/**
 * Rotate ready queue head to the tail.
 * @param state Actor runtime state.
 */
static void fern_actor_ready_rotate(FernActorRuntimeState* state) {
    assert(state != NULL);
    assert(state->ready_len >= 0);
    if (state->ready_len <= 1) {
        return;
    }

    int64_t actor_id = fern_actor_ready_pop(state);
    assert(actor_id > 0);
    fern_actor_ready_push(state, actor_id);
}

/**
 * Spawn actor record with an optional linked supervisor id.
 * @param name Actor name.
 * @param linked_parent_id Supervisor actor id, or 0 for none.
 * @return Actor id (positive integer), or 0 on allocation failure.
 */
static int64_t fern_actor_spawn_with_link(const char* name, int64_t linked_parent_id) {
    assert(name != NULL);
    assert(linked_parent_id >= 0);

    FernActorRuntimeState* state = fern_actor_runtime_state();
    char* saved_name = FERN_STRDUP(name);
    if (saved_name == NULL) return 0;
    if (!fern_actor_registry_reserve(state, state->actor_len + 1)) {
        return 0;
    }

    int64_t actor_id = state->next_actor_id++;
    FernActorRecord* actor = &state->actors[state->actor_len];
    memset(actor, 0, sizeof(*actor));
    actor->actor_id = actor_id;
    actor->alive = 1;
    actor->linked_parent_id = linked_parent_id;
    actor->name = saved_name;
    state->actor_len++;

    return actor_id;
}

/**
 * Spawn an actor and return its id.
 * @param name Actor name.
 * @return Actor id (positive integer), or 0 on allocation failure.
 */
int64_t fern_actor_spawn(const char* name) {
    if (name == NULL) {
        return 0;
    }

    return fern_actor_spawn_with_link(name, 0);
}

/**
 * Spawn an actor linked to the current actor context.
 * @param name Actor name.
 * @return Actor id (positive integer), or 0 when no current actor is set.
 */
int64_t fern_actor_spawn_link(const char* name) {
    if (name == NULL) {
        return 0;
    }

    FernActorRuntimeState* state = fern_actor_runtime_state();
    int64_t linked_parent_id = state->current_actor_id;
    if (linked_parent_id <= 0) {
        return 0;
    }

    FernActorRecord* parent = fern_actor_lookup(state, linked_parent_id);
    if (parent == NULL || !fern_actor_is_alive(parent)) {
        return 0;
    }

    return fern_actor_spawn_with_link(name, linked_parent_id);
}

/**
 * Set current actor context used by spawn_link.
 * @param actor_id Actor id to set as current, or 0 to clear context.
 * @return 0 on success, -1 for invalid/dead actor id.
 */
int64_t fern_actor_set_current(int64_t actor_id) {
    FernActorRuntimeState* state = fern_actor_runtime_state();
    if (actor_id == 0) {
        state->current_actor_id = 0;
        return 0;
    }
    if (actor_id < 0) {
        return -1;
    }

    FernActorRecord* actor = fern_actor_lookup(state, actor_id);
    if (actor == NULL || !fern_actor_is_alive(actor)) {
        return -1;
    }
    state->current_actor_id = actor_id;
    return 0;
}

/**
 * Read current actor context id used by spawn_link.
 * @return Current actor id, or 0 if unset.
 */
int64_t fern_actor_self(void) {
    FernActorRuntimeState* state = fern_actor_runtime_state();
    return state->current_actor_id;
}

/**
 * Set actor runtime clock for deterministic supervision timing.
 * @param now_sec Absolute seconds value.
 * @return 0 on success, -1 on invalid value.
 */
int64_t fern_actor_clock_set(int64_t now_sec) {
    if (now_sec < 0) {
        return -1;
    }
    FernActorRuntimeState* state = fern_actor_runtime_state();
    state->clock_now_sec = now_sec;
    state->clock_manual_mode = 1;
    return 0;
}

/**
 * Advance actor runtime clock by delta seconds.
 * @param delta_sec Seconds to advance.
 * @return 0 on success, -1 on invalid/overflow.
 */
int64_t fern_actor_clock_advance(int64_t delta_sec) {
    if (delta_sec < 0) {
        return -1;
    }
    FernActorRuntimeState* state = fern_actor_runtime_state();
    if (!state->clock_manual_mode) {
        state->clock_now_sec = fern_actor_clock_now_state(state);
        state->clock_manual_mode = 1;
    }
    if (state->clock_now_sec > INT64_MAX - delta_sec) {
        return -1;
    }
    state->clock_now_sec += delta_sec;
    return 0;
}

/**
 * Read actor runtime clock used by supervision timing.
 * @return Runtime clock seconds.
 */
int64_t fern_actor_clock_now(void) {
    FernActorRuntimeState* state = fern_actor_runtime_state();
    return fern_actor_clock_now_state(state);
}

/**
 * Check whether worker already has a given monitor id.
 * @param worker Worker actor record.
 * @param supervisor_id Supervisor actor id.
 * @return 1 when present, 0 otherwise.
 */
static int fern_actor_has_monitor(const FernActorRecord* worker, int64_t supervisor_id) {
    assert(worker != NULL);
    assert(supervisor_id > 0);

    for (int64_t i = 0; i < worker->monitor_len; i++) {
        if (worker->monitor_ids[i] == supervisor_id) {
            return 1;
        }
    }
    return 0;
}

/**
 * Remove a monitor id from a worker monitor list in-place.
 * @param worker Worker actor record.
 * @param supervisor_id Supervisor actor id to remove.
 * @return 1 when removed, 0 when absent.
 */
static int fern_actor_remove_monitor(FernActorRecord* worker, int64_t supervisor_id) {
    assert(worker != NULL);
    assert(supervisor_id > 0);

    for (int64_t i = 0; i < worker->monitor_len; i++) {
        if (worker->monitor_ids[i] == supervisor_id) {
            for (int64_t j = i + 1; j < worker->monitor_len; j++) {
                worker->monitor_ids[j - 1] = worker->monitor_ids[j];
            }
            worker->monitor_len--;
            return 1;
        }
    }
    return 0;
}

/**
 * Find index of child id in supervisor child table.
 * @param supervisor Supervisor actor record.
 * @param worker_id Worker actor id.
 * @return Index when present, -1 otherwise.
 */
static int64_t fern_actor_supervision_child_index(const FernActorRecord* supervisor, int64_t worker_id) {
    assert(supervisor != NULL);
    assert(worker_id > 0);
    for (int64_t i = 0; i < supervisor->supervision_child_len; i++) {
        if (supervisor->supervision_child_ids[i] == worker_id) {
            return i;
        }
    }
    return -1;
}

/**
 * Register or update a worker entry in supervisor child table.
 * @param supervisor Supervisor actor record.
 * @param worker Worker actor record.
 * @param strategy Supervision strategy enum.
 * @return 1 on success, 0 on allocation failure.
 */
static int fern_actor_supervision_register_child(
    FernActorRecord* supervisor,
    FernActorRecord* worker,
    int64_t strategy) {
    assert(supervisor != NULL);
    assert(worker != NULL);
    assert(strategy >= FERN_SUPERVISION_ONE_FOR_ONE);
    assert(strategy <= FERN_SUPERVISION_REST_FOR_ONE);

    int64_t idx = fern_actor_supervision_child_index(supervisor, worker->actor_id);
    if (idx >= 0) {
        worker->supervision_strategy = strategy;
        worker->supervision_order = supervisor->supervision_child_orders[idx];
        supervisor->supervision_child_strategies[idx] = strategy;
        return 1;
    }

    if (!fern_actor_supervision_child_reserve(supervisor, supervisor->supervision_child_len + 1)) {
        return 0;
    }

    int64_t order = supervisor->supervision_next_order++;
    int64_t write_idx = supervisor->supervision_child_len++;
    supervisor->supervision_child_ids[write_idx] = worker->actor_id;
    supervisor->supervision_child_strategies[write_idx] = strategy;
    supervisor->supervision_child_orders[write_idx] = order;

    worker->supervision_strategy = strategy;
    worker->supervision_order = order;
    return 1;
}

/**
 * Replace child id in a supervisor child table after restart.
 * @param supervisor Supervisor actor record.
 * @param old_id Previous worker actor id.
 * @param new_id Replacement worker actor id.
 */
static void fern_actor_supervision_replace_child_id(FernActorRecord* supervisor, int64_t old_id, int64_t new_id) {
    assert(supervisor != NULL);
    assert(old_id > 0);
    assert(new_id > 0);
    for (int64_t i = 0; i < supervisor->supervision_child_len; i++) {
        if (supervisor->supervision_child_ids[i] == old_id) {
            supervisor->supervision_child_ids[i] = new_id;
        }
    }
}

/**
 * Check whether adding a supervision edge preserves a tree with a single owner.
 * @param state Actor registry containing the existing supervision forest.
 * @param supervisor Proposed parent actor.
 * @param worker Proposed child actor.
 * @return 1 for a valid edge, 0 for conflicting ownership or a cycle.
 */
static int fern_actor_supervision_edge_valid(
    FernActorRuntimeState* state,
    const FernActorRecord* supervisor,
    const FernActorRecord* worker) {
    assert(state != NULL);
    assert(supervisor != NULL);
    assert(worker != NULL);
    if (worker->supervisor_id != 0 && worker->supervisor_id != supervisor->actor_id) {
        return 0;
    }
    const FernActorRecord* ancestor = supervisor;
    for (int64_t depth = 0; depth < state->actor_len; depth++) {
        if (ancestor->actor_id == worker->actor_id) {
            return 0;
        }
        if (ancestor->supervisor_id == 0) {
            return 1;
        }
        ancestor = fern_actor_lookup(state, ancestor->supervisor_id);
        if (ancestor == NULL) {
            return 0;
        }
    }
    return 0;
}

/**
 * Register supervision policy for a worker with explicit strategy.
 * @param supervisor_id Supervisor actor id.
 * @param worker_id Worker actor id.
 * @param max_restarts Restart attempts allowed inside the time window.
 * @param period_sec Window size in seconds.
 * @param strategy Supervision strategy enum.
 * @return Result: Ok(0) on success, Err(error code) otherwise.
 */
static int64_t fern_actor_supervise_with_strategy(
    int64_t supervisor_id,
    int64_t worker_id,
    int64_t max_restarts,
    int64_t period_sec,
    int64_t strategy) {
    if (supervisor_id <= 0 || worker_id <= 0 || max_restarts <= 0 || period_sec <= 0) {
        return fern_result_err(FERN_ERR_IO);
    }
    if (strategy < FERN_SUPERVISION_ONE_FOR_ONE || strategy > FERN_SUPERVISION_REST_FOR_ONE) {
        return fern_result_err(FERN_ERR_IO);
    }

    FernActorRuntimeState* state = fern_actor_runtime_state();
    FernActorRecord* supervisor = fern_actor_lookup(state, supervisor_id);
    FernActorRecord* worker = fern_actor_lookup(state, worker_id);
    if (supervisor == NULL || worker == NULL ||
        !fern_actor_is_alive(supervisor) || !fern_actor_is_alive(worker)) {
        return fern_result_err(FERN_ERR_IO);
    }

    if (!fern_actor_supervision_edge_valid(state, supervisor, worker)) {
        return fern_result_err(FERN_ERR_IO);
    }
    /* Reserve before changing ownership so registration failures are atomic. */
    if (!fern_actor_monitor_reserve(worker, worker->monitor_len + 1) ||
        !fern_actor_supervision_register_child(supervisor, worker, strategy)) {
        return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    }
    worker->supervisor_id = supervisor_id;
    worker->supervision_max_restarts = max_restarts;
    worker->supervision_period_sec = period_sec;
    worker->supervision_window_start_sec = -1;
    worker->supervision_restart_count = 0;
    return fern_actor_monitor(supervisor_id, worker_id);
}

/**
 * Register a monitor relation from supervisor to worker.
 * @param supervisor_id Supervisor actor id.
 * @param worker_id Worker actor id.
 * @return Result: Ok(0) on success, Err(error code) otherwise.
 */
int64_t fern_actor_monitor(int64_t supervisor_id, int64_t worker_id) {
    if (supervisor_id <= 0 || worker_id <= 0) {
        return fern_result_err(FERN_ERR_IO);
    }

    FernActorRuntimeState* state = fern_actor_runtime_state();
    FernActorRecord* supervisor = fern_actor_lookup(state, supervisor_id);
    FernActorRecord* worker = fern_actor_lookup(state, worker_id);
    if (supervisor == NULL || worker == NULL ||
        !fern_actor_is_alive(supervisor) || !fern_actor_is_alive(worker)) {
        return fern_result_err(FERN_ERR_IO);
    }

    if (fern_actor_has_monitor(worker, supervisor_id)) {
        return fern_result_ok(0);
    }
    if (!fern_actor_monitor_reserve(worker, worker->monitor_len + 1)) {
        return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    }

    worker->monitor_ids[worker->monitor_len++] = supervisor->actor_id;
    return fern_result_ok(0);
}

/**
 * Remove a monitor relation from supervisor to worker.
 * @param supervisor_id Supervisor actor id.
 * @param worker_id Worker actor id.
 * @return Result: Ok(0) on success, Err(error code) otherwise.
 */
int64_t fern_actor_demonitor(int64_t supervisor_id, int64_t worker_id) {
    if (supervisor_id <= 0 || worker_id <= 0) {
        return fern_result_err(FERN_ERR_IO);
    }

    FernActorRuntimeState* state = fern_actor_runtime_state();
    FernActorRecord* supervisor = fern_actor_lookup(state, supervisor_id);
    FernActorRecord* worker = fern_actor_lookup(state, worker_id);
    if (supervisor == NULL || worker == NULL || !fern_actor_is_alive(supervisor)) {
        return fern_result_err(FERN_ERR_IO);
    }

    (void)fern_actor_remove_monitor(worker, supervisor_id);
    return fern_result_ok(0);
}

/**
 * Register supervision policy for a worker with restart intensity limits.
 * Also ensures supervisor monitors worker so DOWN notifications are delivered.
 * @param supervisor_id Supervisor actor id.
 * @param worker_id Worker actor id.
 * @param max_restarts Restart attempts allowed inside the time window.
 * @param period_sec Window size in seconds.
 * @return Result: Ok(0) on success, Err(error code) otherwise.
 */
int64_t fern_actor_supervise(int64_t supervisor_id, int64_t worker_id, int64_t max_restarts, int64_t period_sec) {
    return fern_actor_supervise_with_strategy(
        supervisor_id,
        worker_id,
        max_restarts,
        period_sec,
        FERN_SUPERVISION_ONE_FOR_ONE);
}

/**
 * Register supervision policy with one_for_all strategy.
 * @param supervisor_id Supervisor actor id.
 * @param worker_id Worker actor id.
 * @param max_restarts Restart attempts allowed inside the time window.
 * @param period_sec Window size in seconds.
 * @return Result: Ok(0) on success, Err(error code) otherwise.
 */
int64_t fern_actor_supervise_one_for_all(
    int64_t supervisor_id,
    int64_t worker_id,
    int64_t max_restarts,
    int64_t period_sec) {
    return fern_actor_supervise_with_strategy(
        supervisor_id,
        worker_id,
        max_restarts,
        period_sec,
        FERN_SUPERVISION_ONE_FOR_ALL);
}

/**
 * Register supervision policy with rest_for_one strategy.
 * @param supervisor_id Supervisor actor id.
 * @param worker_id Worker actor id.
 * @param max_restarts Restart attempts allowed inside the time window.
 * @param period_sec Window size in seconds.
 * @return Result: Ok(0) on success, Err(error code) otherwise.
 */
int64_t fern_actor_supervise_rest_for_one(
    int64_t supervisor_id,
    int64_t worker_id,
    int64_t max_restarts,
    int64_t period_sec) {
    return fern_actor_supervise_with_strategy(
        supervisor_id,
        worker_id,
        max_restarts,
        period_sec,
        FERN_SUPERVISION_REST_FOR_ONE);
}

/**
 * Consume one restart budget token from the worker supervision window.
 * @param worker Supervised worker actor.
 * @return 1 when restart is allowed, 0 when budget is exhausted/invalid.
 */
static int fern_actor_supervision_consume_budget(FernActorRuntimeState* state, FernActorRecord* worker) {
    assert(state != NULL);
    assert(worker != NULL);
    if (worker->supervision_max_restarts <= 0 || worker->supervision_period_sec <= 0) {
        return 0;
    }

    int64_t now_sec = fern_actor_clock_now_state(state);

    if (worker->supervision_window_start_sec < 0 ||
        (now_sec - worker->supervision_window_start_sec) >= worker->supervision_period_sec) {
        worker->supervision_window_start_sec = now_sec;
        worker->supervision_restart_count = 0;
    }

    if (worker->supervision_restart_count >= worker->supervision_max_restarts) {
        return 0;
    }

    worker->supervision_restart_count++;
    return 1;
}

/**
 * Send a message to an actor mailbox.
 * @param actor_id Destination actor id.
 * @param msg Message payload.
 * @return Result: Ok(0) on enqueue, Err(error code) otherwise.
 */
int64_t fern_actor_send(int64_t actor_id, const char* msg) {
    if (actor_id <= 0 || msg == NULL) {
        return fern_result_err(FERN_ERR_IO);
    }

    FernActorRuntimeState* state = fern_actor_runtime_state();
    FernActorRecord* actor = fern_actor_lookup(state, actor_id);
    if (actor == NULL || !fern_actor_is_alive(actor)) {
        return fern_result_err(FERN_ERR_IO);
    }
    if (actor->scheduler_tokens == INT64_MAX) {
        return fern_result_err(FERN_ERR_IO);
    }

    if (!fern_actor_mailbox_reserve(actor, actor->mailbox_len + 1)) {
        return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    }
    if (!actor->scheduler_enqueued && !fern_actor_ready_reserve(state, state->ready_len + 1)) {
        return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    }

    char* copy = FERN_STRDUP(msg);
    if (copy == NULL) {
        return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    }

    int64_t tail = (actor->mailbox_head + actor->mailbox_len) % actor->mailbox_cap;
    actor->mailbox_data[tail] = copy;
    actor->mailbox_len++;
    actor->scheduler_tokens++;

    if (!actor->scheduler_enqueued) {
        fern_actor_ready_push(state, actor->actor_id);
        actor->scheduler_enqueued = 1;
    }

    return fern_result_ok(0);
}

/**
 * Receive the next actor message from mailbox FIFO.
 * @param actor_id Actor id.
 * @return Result: Ok(message) or Err(error code).
 */
int64_t fern_actor_receive(int64_t actor_id) {
    if (actor_id <= 0) {
        return fern_result_err(FERN_ERR_IO);
    }

    FernActorRuntimeState* state = fern_actor_runtime_state();
    FernActorRecord* actor = fern_actor_lookup(state, actor_id);
    if (actor == NULL || !fern_actor_is_alive(actor) || actor->mailbox_len <= 0) {
        return fern_result_err(FERN_ERR_IO);
    }

    char* msg = actor->mailbox_data[actor->mailbox_head];
    actor->mailbox_head = (actor->mailbox_head + 1) % actor->mailbox_cap;
    actor->mailbox_len--;
    if (actor->mailbox_len == 0) {
        actor->mailbox_head = 0;
    }
    if (actor->scheduler_tokens > actor->mailbox_len) {
        actor->scheduler_tokens = actor->mailbox_len;
    }

    return fern_result_ok((int64_t)(intptr_t)msg);
}

/**
 * Build a stable signal message payload.
 * @param tag Signal tag, e.g. "Exit" or "DOWN".
 * @param actor_id Exiting actor id.
 * @param reason Exit reason string.
 * @return Newly allocated message string, or NULL on allocation failure.
 */
static char* fern_actor_format_signal_message(const char* tag, int64_t actor_id, const char* reason) {
    assert(tag != NULL);
    assert(actor_id > 0);

    const char* safe_reason = (reason != NULL && reason[0] != '\0') ? reason : "unknown";
    int needed = snprintf(NULL, 0, "%s(%lld,%s)", tag, (long long)actor_id, safe_reason);
    if (needed < 0) {
        return NULL;
    }

    char* message = FERN_ALLOC((size_t)needed + 1);
    if (message == NULL) {
        return NULL;
    }
    snprintf(message, (size_t)needed + 1, "%s(%lld,%s)", tag, (long long)actor_id, safe_reason);
    return message;
}

/**
 * Check whether exit reason should be treated as normal shutdown.
 * @param reason Exit reason string.
 * @return 1 when reason is normal/shutdown, 0 otherwise.
 */
static int fern_actor_reason_is_normal(const char* reason) {
    if (reason == NULL || reason[0] == '\0') {
        return 1;
    }
    if (strcmp(reason, "normal") == 0) {
        return 1;
    }
    if (strcmp(reason, "shutdown") == 0) {
        return 1;
    }
    return 0;
}

/**
 * Build a stable RESTART(old_pid,new_pid) payload.
 * @param old_actor_id Restarted actor id.
 * @param new_actor_id Replacement actor id.
 * @return Newly allocated message string, or NULL on allocation failure.
 */
static char* fern_actor_format_restart_message(int64_t old_actor_id, int64_t new_actor_id) {
    assert(old_actor_id > 0);
    assert(new_actor_id > 0);

    int needed = snprintf(NULL, 0, "RESTART(%lld,%lld)",
        (long long)old_actor_id, (long long)new_actor_id);
    if (needed < 0) {
        return NULL;
    }

    char* message = FERN_ALLOC((size_t)needed + 1);
    if (message == NULL) {
        return NULL;
    }
    snprintf(message, (size_t)needed + 1, "RESTART(%lld,%lld)",
        (long long)old_actor_id, (long long)new_actor_id);
    return message;
}

/**
 * Check whether id is already present in a small target list.
 * @param ids Target id buffer.
 * @param len Buffer length.
 * @param candidate Actor id candidate.
 * @return 1 when present, 0 otherwise.
 */
static int fern_actor_target_list_contains(const int64_t* ids, int64_t len, int64_t candidate) {
    assert(ids != NULL);
    assert(len >= 0);
    assert(candidate > 0);
    for (int64_t i = 0; i < len; i++) {
        if (ids[i] == candidate) {
            return 1;
        }
    }
    return 0;
}

/**
 * Collect restart targets from supervisor child table for crashed actor strategy.
 * @param state Actor registry for checking child liveness.
 * @param supervisor Supervisor actor record.
 * @param crashed Crashed child actor record.
 * @param out_ids Output actor id buffer.
 * @param out_cap Output buffer capacity.
 * @return Number of collected targets (>=1), or 0 on invalid input.
 */
static int64_t fern_actor_collect_restart_targets(
    FernActorRuntimeState* state,
    const FernActorRecord* supervisor,
    const FernActorRecord* crashed,
    int64_t* out_ids,
    int64_t out_cap) {
    assert(supervisor != NULL);
    assert(crashed != NULL);
    assert(out_ids != NULL);
    assert(out_cap > 0);

    int64_t strategy = crashed->supervision_strategy;
    int64_t len = 0;

    if (strategy == FERN_SUPERVISION_ONE_FOR_ONE) {
        out_ids[len++] = crashed->actor_id;
        return len;
    }

    for (int64_t i = 0; i < supervisor->supervision_child_len && len < out_cap; i++) {
        int64_t child_id = supervisor->supervision_child_ids[i];
        int64_t child_strategy = supervisor->supervision_child_strategies[i];
        int64_t child_order = supervisor->supervision_child_orders[i];
        if (child_id <= 0 || child_strategy != strategy) {
            continue;
        }
        /* A sibling that already terminated normally must remain stopped. */
        FernActorRecord* child = fern_actor_lookup(state, child_id);
        if (child == NULL || (child_id != crashed->actor_id && !fern_actor_is_alive(child))) {
            continue;
        }

        if (strategy == FERN_SUPERVISION_ONE_FOR_ALL) {
            if (!fern_actor_target_list_contains(out_ids, len, child_id)) {
                out_ids[len++] = child_id;
            }
            continue;
        }

        if (strategy == FERN_SUPERVISION_REST_FOR_ONE) {
            if (child_order >= crashed->supervision_order &&
                !fern_actor_target_list_contains(out_ids, len, child_id)) {
                out_ids[len++] = child_id;
            }
        }
    }

    if (!fern_actor_target_list_contains(out_ids, len, crashed->actor_id) && len < out_cap) {
        out_ids[len++] = crashed->actor_id;
    }

    if (len == 0) {
        out_ids[len++] = crashed->actor_id;
    }
    return len;
}

/**
 * Advance a preorder walk using the acyclic ownership forest's parent backlinks.
 * @param state Valid actor registry, unchanged during the walk.
 * @param actor Last visited record, with its next-child cursor initialized.
 * @param root_id Walk boundary; ancestors outside this subtree are never visited.
 * @return Next descendant, or NULL after the whole subtree is visited.
 */
static FernActorRecord* fern_actor_exit_next(
    FernActorRuntimeState* state, FernActorRecord* actor, int64_t root_id) {
    assert(state != NULL);
    assert(actor != NULL);
    for (int64_t depth = 0; depth < state->actor_len; depth++) {
        if (actor->exit_child_cursor < actor->supervision_child_len) {
            int64_t child_id = actor->supervision_child_ids[actor->exit_child_cursor++];
            FernActorRecord* child = fern_actor_lookup(state, child_id);
            assert(child != NULL && child->supervisor_id == actor->actor_id);
            return child;
        }
        if (actor->actor_id == root_id) return NULL;
        actor = fern_actor_lookup(state, actor->supervisor_id);
        assert(actor != NULL);
    }
    return NULL;
}

/**
 * Stop every owned descendant before any notification allocation can fail.
 * @param state Valid single-threaded registry; registration preserves an acyclic forest.
 * @param root Live actor whose subtree is being stopped.
 * @return Nothing; root heads a preorder list of newly stopped records.
 */
static void fern_actor_stop_subtree(FernActorRuntimeState* state, FernActorRecord* root) {
    assert(state != NULL);
    assert(root != NULL && fern_actor_is_alive(root));
    FernActorRecord* actor = root;
    FernActorRecord* previous = NULL;
    for (int64_t visited = 0; visited < state->actor_len && actor != NULL; visited++) {
        actor->exit_child_cursor = 0;
        if (fern_actor_is_alive(actor)) {
            actor->alive = 0;
            actor->scheduler_tokens = 0;
            actor->scheduler_enqueued = 0;
            actor->exit_next_id = 0;
            if (state->current_actor_id == actor->actor_id) state->current_actor_id = 0;
            if (previous != NULL) previous->exit_next_id = actor->actor_id;
            previous = actor;
        }
        actor = fern_actor_exit_next(state, actor, root->actor_id);
    }
    assert(actor == NULL);
}

/**
 * Deliver one signal only to a still-live observer outside the stopped subtree.
 * @param state Valid registry after all lifecycle changes have been applied.
 * @param observer_id Optional linked parent or monitor identity.
 * @param actor_id Stopped actor identity.
 * @param kind Signal spelling, either Exit or DOWN.
 * @param reason Root reason or descendant shutdown reason.
 * @return Ok(0), or the first allocation/send error.
 */
static int64_t fern_actor_exit_signal(FernActorRuntimeState* state, int64_t observer_id,
    int64_t actor_id, const char* kind, const char* reason) {
    assert(state != NULL);
    assert(kind != NULL);
    FernActorRecord* observer = fern_actor_lookup(state, observer_id);
    if (observer == NULL || !fern_actor_is_alive(observer)) return fern_result_ok(0);
    char* message = fern_actor_format_signal_message(kind, actor_id, reason);
    if (message == NULL) return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    return fern_actor_send(observer_id, message);
}

/**
 * Notify surviving links and monitors in deterministic root-first registration order.
 * @param state Registry containing the newly stopped linked list.
 * @param root_id Head of the stopped list.
 * @param reason Original root reason; descendants receive shutdown.
 * @return Ok(0), or first failure; lifecycle transitions are never rolled back.
 */
static int64_t fern_actor_notify_subtree(FernActorRuntimeState* state,
    int64_t root_id, const char* reason) {
    assert(state != NULL);
    assert(root_id > 0);
    int64_t next_id = root_id;
    for (int64_t count = 0; count < state->actor_len && next_id != 0; count++) {
        FernActorRecord* actor = fern_actor_lookup(state, next_id);
        assert(actor != NULL && !fern_actor_is_alive(actor));
        const char* exit_reason = next_id == root_id ? reason : "shutdown";
        next_id = actor->exit_next_id;
        actor->exit_next_id = 0;
        if (actor->linked_parent_id > 0) {
            int64_t status = fern_actor_exit_signal(state, actor->linked_parent_id,
                actor->actor_id, "Exit", exit_reason);
            if (!fern_result_is_ok(status)) return status;
        }
        for (int64_t i = 0; i < actor->monitor_len; i++) {
            int64_t status = fern_actor_exit_signal(state, actor->monitor_ids[i],
                actor->actor_id, "DOWN", exit_reason);
            if (!fern_result_is_ok(status)) return status;
        }
    }
    return fern_result_ok(0);
}

/**
 * Stop strategy siblings and replace selected direct children in registration order.
 * @param state Valid registry; child creation can relocate its record array.
 * @param supervisor_id Live owner identity, retained rather than a relocatable pointer.
 * @param target_ids Selected direct children, including the failed actor.
 * @param target_len Number of selected children.
 * @param actor_id Failed child whose replacement is returned.
 * @return Ok(replacement id), or the first lifecycle/notification error.
 */
static int64_t fern_actor_restart_targets(FernActorRuntimeState* state,
    int64_t supervisor_id, const int64_t* target_ids, int64_t target_len, int64_t actor_id) {
    assert(state != NULL);
    assert(target_ids != NULL && target_len > 0);
    for (int64_t i = 0; i < target_len; i++) {
        FernActorRecord* sibling = fern_actor_lookup(state, target_ids[i]);
        if (target_ids[i] != actor_id && sibling != NULL && fern_actor_is_alive(sibling)) {
            int64_t status = fern_actor_exit(target_ids[i], "shutdown");
            if (!fern_result_is_ok(status)) return status;
        }
    }
    int64_t primary_id = 0;
    for (int64_t i = 0; i < target_len; i++) {
        int64_t restarted = fern_actor_restart(target_ids[i]);
        if (!fern_result_is_ok(restarted)) return restarted;
        int64_t next_id = fern_result_unwrap(restarted);
        if (target_ids[i] == actor_id) primary_id = next_id;
        char* message = fern_actor_format_restart_message(target_ids[i], next_id);
        if (message == NULL) return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
        int64_t status = fern_actor_send(supervisor_id, message);
        if (!fern_result_is_ok(status)) return status;
    }
    return primary_id > 0 ? fern_result_ok(primary_id) : fern_result_err(FERN_ERR_IO);
}

/**
 * Apply the failed root's direct-owner restart policy after subtree notifications.
 * @param state Valid registry with the entire failed subtree stopped.
 * @param actor Failed root record.
 * @param reason Abnormal exit reason.
 * @return Ok(0) without a live owner, Ok(replacement), or policy/allocation error.
 */
static int64_t fern_actor_restart_supervised(FernActorRuntimeState* state,
    FernActorRecord* actor, const char* reason) {
    assert(state != NULL);
    assert(actor != NULL);
    FernActorRecord* supervisor = fern_actor_lookup(state, actor->supervisor_id);
    if (supervisor == NULL || !fern_actor_is_alive(supervisor)) return fern_result_ok(0);
    if (!fern_actor_supervision_consume_budget(state, actor)) {
        char* message = fern_actor_format_signal_message("ESCALATE", actor->actor_id, reason);
        if (message == NULL) return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
        int64_t status = fern_actor_send(supervisor->actor_id, message);
        return fern_result_is_ok(status) ? fern_result_err(FERN_ERR_IO) : status;
    }
    if (supervisor->supervision_child_len >= INT64_MAX ||
        (uint64_t)supervisor->supervision_child_len >= SIZE_MAX / sizeof(int64_t)) {
        return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    }
    int64_t capacity = supervisor->supervision_child_len + 1;
    int64_t* ids = FERN_ALLOC((size_t)capacity * sizeof(int64_t));
    if (ids == NULL) return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    int64_t length = fern_actor_collect_restart_targets(state, supervisor, actor, ids, capacity);
    if (length <= 0) return fern_result_err(FERN_ERR_IO);
    return fern_actor_restart_targets(state, supervisor->actor_id, ids, length, actor->actor_id);
}

/**
 * Stop an actor's owned subtree before delivering links/monitors or restarting its root.
 * @param actor_id Live exiting actor id.
 * @param reason Exit reason; descendants receive shutdown.
 * @return Ok(0/replacement id), or first error with the failed subtree still stopped.
 */
int64_t fern_actor_exit(int64_t actor_id, const char* reason) {
    if (actor_id <= 0) return fern_result_err(FERN_ERR_IO);
    FernActorRuntimeState* state = fern_actor_runtime_state();
    FernActorRecord* actor = fern_actor_lookup(state, actor_id);
    assert(state != NULL);
    assert(state->actor_len >= 0);
    if (actor == NULL || !fern_actor_is_alive(actor)) return fern_result_err(FERN_ERR_IO);
    fern_actor_stop_subtree(state, actor);
    int64_t status = fern_actor_notify_subtree(state, actor_id, reason);
    if (!fern_result_is_ok(status)) return status;
    if (fern_actor_reason_is_normal(reason)) return fern_result_ok(0);
    return fern_actor_restart_supervised(state, actor, reason);
}

/**
 * Restart an actor and return the new actor id.
 * Restart preserves name/link baseline and carries monitor registrations.
 * Actor must already be exited/dead.
 * @param actor_id Actor id to restart.
 * @return Result: Ok(new actor id) or Err(error code).
 */
int64_t fern_actor_restart(int64_t actor_id) {
    if (actor_id <= 0) {
        return fern_result_err(FERN_ERR_IO);
    }

    FernActorRuntimeState* state = fern_actor_runtime_state();
    FernActorRecord* actor = fern_actor_lookup(state, actor_id);
    if (actor == NULL || fern_actor_is_alive(actor) || actor->replacement_id != 0) {
        return fern_result_err(FERN_ERR_IO);
    }

    if (actor->supervisor_id > 0) {
        FernActorRecord* owner = fern_actor_lookup(state, actor->supervisor_id);
        if (owner == NULL || !fern_actor_is_alive(owner)) return fern_result_err(FERN_ERR_IO);
    }

    /* Prepare fallible monitor storage before publishing a live replacement. */
    FernActorRecord prepared = {0};
    if (actor->monitor_len > 0) {
        if (!fern_actor_monitor_reserve(&prepared, actor->monitor_len)) {
            return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
        }
        memcpy(prepared.monitor_ids, actor->monitor_ids,
            (size_t)actor->monitor_len * sizeof(int64_t));
        prepared.monitor_len = actor->monitor_len;
    }
    const char* name = (actor->name != NULL) ? actor->name : "worker";
    int64_t next_id = fern_actor_spawn_with_link(name, actor->linked_parent_id);
    if (next_id <= 0) {
        return fern_result_err(FERN_ERR_OUT_OF_MEMORY);
    }

    /* Spawning may move the registry; never retain a record pointer across it. */
    actor = fern_actor_lookup(state, actor_id);
    FernActorRecord* next = fern_actor_lookup(state, next_id);
    assert(actor != NULL);
    assert(next != NULL);

    next->monitor_ids = prepared.monitor_ids;
    next->monitor_len = prepared.monitor_len;
    next->monitor_cap = prepared.monitor_cap;

    next->supervisor_id = actor->supervisor_id;
    next->supervision_strategy = actor->supervision_strategy;
    next->supervision_order = actor->supervision_order;
    next->supervision_max_restarts = actor->supervision_max_restarts;
    next->supervision_period_sec = actor->supervision_period_sec;
    next->supervision_window_start_sec = actor->supervision_window_start_sec;
    next->supervision_restart_count = actor->supervision_restart_count;

    if (actor->supervisor_id > 0) {
        FernActorRecord* supervisor = fern_actor_lookup(state, actor->supervisor_id);
        if (supervisor != NULL) {
            fern_actor_supervision_replace_child_id(supervisor, actor_id, next_id);
        }
    }

    actor->replacement_id = next_id;
    return fern_result_ok(next_id);
}

/**
 * Get current mailbox length for an actor.
 * @param actor_id Actor id.
 * @return Number of queued messages, or -1 for invalid actor id.
 */
int64_t fern_actor_mailbox_len(int64_t actor_id) {
    FernActorRuntimeState* state = fern_actor_runtime_state();
    FernActorRecord* actor = fern_actor_lookup(state, actor_id);
    if (actor == NULL || !fern_actor_is_alive(actor)) {
        return -1;
    }
    return actor->mailbox_len;
}

/**
 * Return next runnable actor id from scheduler queue.
 * @return Actor id, or 0 when no actor is ready.
 */
int64_t fern_actor_scheduler_next(void) {
    FernActorRuntimeState* state = fern_actor_runtime_state();

    while (state->ready_len > 0) {
        int64_t actor_id = state->ready_ids[state->ready_head];
        FernActorRecord* actor = fern_actor_lookup(state, actor_id);

        if (actor == NULL) {
            (void)fern_actor_ready_pop(state);
            continue;
        }
        if (!fern_actor_is_alive(actor)) {
            (void)fern_actor_ready_pop(state);
            actor->scheduler_enqueued = 0;
            continue;
        }
        if (actor->scheduler_tokens > actor->mailbox_len) {
            actor->scheduler_tokens = actor->mailbox_len;
        }
        if (actor->mailbox_len <= 0 || actor->scheduler_tokens <= 0) {
            (void)fern_actor_ready_pop(state);
            actor->scheduler_enqueued = 0;
            continue;
        }

        actor->scheduler_tokens--;
        if (actor->scheduler_tokens <= 0) {
            (void)fern_actor_ready_pop(state);
            actor->scheduler_enqueued = 0;
        } else {
            fern_actor_ready_rotate(state);
        }
        return actor_id;
    }

    return 0;
}

/**
 * Start an actor and return its id.
 * @param name Actor name.
 * @return Actor id.
 */
int64_t fern_actor_start(const char* name) {
    return fern_actor_spawn(name);
}

/**
 * Post a message to an actor.
 * @param actor_id Destination actor id.
 * @param msg Message payload.
 * @return Result: Ok(0) or Err(error code).
 */
int64_t fern_actor_post(int64_t actor_id, const char* msg) {
    return fern_actor_send(actor_id, msg);
}

/**
 * Receive the next actor message.
 * @param actor_id Actor id.
 * @return Result: Ok(message) or Err(error code).
 */
int64_t fern_actor_next(int64_t actor_id) {
    return fern_actor_receive(actor_id);
}

/* ========== Regex Functions ========== */

/**
 * Check if string matches regex pattern.
 * @param s The string to match.
 * @param pattern The regex pattern.
 * @return 1 if matches, 0 otherwise.
 */
int64_t fern_regex_is_match(const char* s, const char* pattern) {
    assert(s != NULL);
    assert(pattern != NULL);
    
    regex_t regex;
    int ret = regcomp(&regex, pattern, REG_EXTENDED | REG_NOSUB);
    if (ret != 0) {
        return 0; /* Invalid pattern */
    }
    
    ret = regexec(&regex, s, 0, NULL, 0);
    regfree(&regex);
    
    return (ret == 0) ? 1 : 0;
}

/**
 * Find first match of regex in string.
 * @param s The string to search.
 * @param pattern The regex pattern.
 * @return FernRegexMatch with match info (start=-1 if no match).
 */
FernRegexMatch* fern_regex_find(const char* s, const char* pattern) {
    assert(s != NULL);
    assert(pattern != NULL);
    
    FernRegexMatch* result = FERN_ALLOC(sizeof(FernRegexMatch));
    assert(result != NULL);
    result->start = -1;
    result->end = -1;
    result->matched = NULL;
    
    regex_t regex;
    int ret = regcomp(&regex, pattern, REG_EXTENDED);
    if (ret != 0) {
        return result; /* Invalid pattern */
    }
    
    regmatch_t match;
    ret = regexec(&regex, s, 1, &match, 0);
    regfree(&regex);
    
    if (ret == 0) {
        result->start = match.rm_so;
        result->end = match.rm_eo;
        size_t len = (size_t)(match.rm_eo - match.rm_so);
        result->matched = FERN_ALLOC(len + 1);
        assert(result->matched != NULL);
        memcpy(result->matched, s + match.rm_so, len);
        result->matched[len] = '\0';
    }
    
    return result;
}

/**
 * Find all matches of regex in string.
 * @param s The string to search.
 * @param pattern The regex pattern.
 * @return FernStringList of all matched substrings.
 */
FernStringList* fern_regex_find_all(const char* s, const char* pattern) {
    assert(s != NULL);
    assert(pattern != NULL);
    
    FernStringList* list = fern_rc_alloc(sizeof(FernStringList), FERN_RC_TYPE_STRING_LIST);
    assert(list != NULL);
    list->cap = 8;
    list->len = 0;
    list->data = FERN_ALLOC((size_t)list->cap * sizeof(char*));
    assert(list->data != NULL);
    
    regex_t regex;
    int ret = regcomp(&regex, pattern, REG_EXTENDED);
    if (ret != 0) {
        return list; /* Invalid pattern, return empty list */
    }
    
    const char* p = s;
    regmatch_t match;
    
    while (regexec(&regex, p, 1, &match, 0) == 0) {
        /* Grow if needed */
        if (list->len >= list->cap) {
            list->cap *= 2;
            list->data = FERN_REALLOC(list->data, (size_t)list->cap * sizeof(char*));
            assert(list->data != NULL);
        }
        
        /* Copy matched substring */
        size_t len = (size_t)(match.rm_eo - match.rm_so);
        char* matched = FERN_ALLOC(len + 1);
        assert(matched != NULL);
        memcpy(matched, p + match.rm_so, len);
        matched[len] = '\0';
        list->data[list->len++] = matched;
        
        /* Move past this match */
        p += match.rm_eo;
        if (*p == '\0') break;
        
        /* Avoid infinite loop on zero-length matches */
        if (match.rm_eo == match.rm_so) {
            p++;
            if (*p == '\0') break;
        }
    }
    
    regfree(&regex);
    return list;
}

/**
 * Replace first match of regex with replacement.
 * @param s The string to modify.
 * @param pattern The regex pattern.
 * @param replacement The replacement string.
 * @return New string with replacement applied.
 */
char* fern_regex_replace(const char* s, const char* pattern, const char* replacement) {
    assert(s != NULL);
    assert(pattern != NULL);
    assert(replacement != NULL);
    
    regex_t regex;
    int ret = regcomp(&regex, pattern, REG_EXTENDED);
    if (ret != 0) {
        /* Invalid pattern, return copy of original */
        return FERN_STRDUP(s);
    }
    
    regmatch_t match;
    ret = regexec(&regex, s, 1, &match, 0);
    regfree(&regex);
    
    if (ret != 0) {
        /* No match, return copy of original */
        return FERN_STRDUP(s);
    }
    
    /* Build result: prefix + replacement + suffix */
    size_t prefix_len = (size_t)match.rm_so;
    size_t repl_len = strlen(replacement);
    size_t suffix_len = strlen(s + match.rm_eo);
    size_t total_len = prefix_len + repl_len + suffix_len;
    
    char* result = FERN_ALLOC(total_len + 1);
    assert(result != NULL);
    
    memcpy(result, s, prefix_len);
    memcpy(result + prefix_len, replacement, repl_len);
    memcpy(result + prefix_len + repl_len, s + match.rm_eo, suffix_len + 1);
    
    return result;
}

/**
 * Replace all matches of regex with replacement.
 * @param s The string to modify.
 * @param pattern The regex pattern.
 * @param replacement The replacement string.
 * @return New string with all replacements applied.
 */
char* fern_regex_replace_all(const char* s, const char* pattern, const char* replacement) {
    assert(s != NULL);
    assert(pattern != NULL);
    assert(replacement != NULL);
    
    regex_t regex;
    int ret = regcomp(&regex, pattern, REG_EXTENDED);
    if (ret != 0) {
        return FERN_STRDUP(s);
    }
    
    /* Build result incrementally */
    size_t result_cap = strlen(s) * 2 + 1;
    size_t result_len = 0;
    char* result = FERN_ALLOC(result_cap);
    assert(result != NULL);
    
    const char* p = s;
    regmatch_t match;
    size_t repl_len = strlen(replacement);
    
    while (regexec(&regex, p, 1, &match, 0) == 0) {
        /* Append prefix (before match) */
        size_t prefix_len = (size_t)match.rm_so;
        if (result_len + prefix_len + repl_len >= result_cap) {
            result_cap = (result_len + prefix_len + repl_len) * 2;
            result = FERN_REALLOC(result, result_cap);
            assert(result != NULL);
        }
        memcpy(result + result_len, p, prefix_len);
        result_len += prefix_len;
        
        /* Append replacement */
        memcpy(result + result_len, replacement, repl_len);
        result_len += repl_len;
        
        /* Move past this match */
        p += match.rm_eo;
        
        /* Avoid infinite loop on zero-length matches */
        if (match.rm_eo == match.rm_so) {
            if (*p == '\0') break;
            /* Copy one char and move on */
            if (result_len + 1 >= result_cap) {
                result_cap *= 2;
                result = FERN_REALLOC(result, result_cap);
                assert(result != NULL);
            }
            result[result_len++] = *p++;
        }
    }
    
    /* Append remaining suffix */
    size_t suffix_len = strlen(p);
    if (result_len + suffix_len >= result_cap) {
        result_cap = result_len + suffix_len + 1;
        result = FERN_REALLOC(result, result_cap);
        assert(result != NULL);
    }
    memcpy(result + result_len, p, suffix_len + 1);
    
    regfree(&regex);
    return result;
}

/**
 * Split string by regex pattern.
 * @param s The string to split.
 * @param pattern The regex pattern to split on.
 * @return FernStringList of parts.
 */
FernStringList* fern_regex_split(const char* s, const char* pattern) {
    assert(s != NULL);
    assert(pattern != NULL);
    
    FernStringList* list = fern_rc_alloc(sizeof(FernStringList), FERN_RC_TYPE_STRING_LIST);
    assert(list != NULL);
    list->cap = 8;
    list->len = 0;
    list->data = FERN_ALLOC((size_t)list->cap * sizeof(char*));
    assert(list->data != NULL);
    
    regex_t regex;
    int ret = regcomp(&regex, pattern, REG_EXTENDED);
    if (ret != 0) {
        /* Invalid pattern, return original as single element */
        list->data[0] = FERN_STRDUP(s);
        list->len = 1;
        return list;
    }
    
    const char* p = s;
    regmatch_t match;
    
    while (regexec(&regex, p, 1, &match, 0) == 0) {
        /* Grow if needed */
        if (list->len >= list->cap) {
            list->cap *= 2;
            list->data = FERN_REALLOC(list->data, (size_t)list->cap * sizeof(char*));
            assert(list->data != NULL);
        }
        
        /* Copy part before match */
        size_t len = (size_t)match.rm_so;
        char* part = FERN_ALLOC(len + 1);
        assert(part != NULL);
        memcpy(part, p, len);
        part[len] = '\0';
        list->data[list->len++] = part;
        
        /* Move past this match */
        p += match.rm_eo;
        
        /* Avoid infinite loop on zero-length matches */
        if (match.rm_eo == match.rm_so) {
            if (*p == '\0') break;
            p++;
        }
    }
    
    /* Add remaining part */
    if (list->len >= list->cap) {
        list->cap *= 2;
        list->data = FERN_REALLOC(list->data, (size_t)list->cap * sizeof(char*));
        assert(list->data != NULL);
    }
    list->data[list->len++] = FERN_STRDUP(p);
    
    regfree(&regex);
    return list;
}

/**
 * Find match with capture groups.
 * @param s The string to search.
 * @param pattern The regex pattern with groups.
 * @return FernRegexCaptures with all capture groups.
 */
FernRegexCaptures* fern_regex_captures(const char* s, const char* pattern) {
    assert(s != NULL);
    assert(pattern != NULL);
    
    FernRegexCaptures* result = FERN_ALLOC(sizeof(FernRegexCaptures));
    assert(result != NULL);
    result->count = 0;
    result->captures = NULL;
    
    regex_t regex;
    int ret = regcomp(&regex, pattern, REG_EXTENDED);
    if (ret != 0) {
        return result;
    }
    
    /* Preserve the existing bound of nine subexpressions plus the full match. */
    size_t max_groups = 10;
    regmatch_t* matches = FERN_ALLOC(max_groups * sizeof(regmatch_t));
    assert(matches != NULL);
    
    ret = regexec(&regex, s, max_groups, matches, 0);
    if (ret != 0) {
        regfree(&regex);
        FERN_FREE(matches);
        return result;
    }
    
    /* Group slots belong to the pattern, even when an optional group is absent. */
    size_t count = regex.re_nsub < max_groups ? regex.re_nsub + 1 : max_groups;
    
    result->count = (int64_t)count;
    result->captures = FERN_ALLOC(count * sizeof(FernRegexMatch));
    assert(result->captures != NULL);
    
    for (size_t i = 0; i < count; i++) {
        result->captures[i].start = matches[i].rm_so;
        result->captures[i].end = matches[i].rm_eo;
        
        bool matched = matches[i].rm_so >= 0;
        size_t len = matched ? (size_t)(matches[i].rm_eo - matches[i].rm_so) : 0;
        result->captures[i].matched = FERN_ALLOC(len + 1);
        assert(result->captures[i].matched != NULL);
        if (matched) {
            memcpy(result->captures[i].matched, s + matches[i].rm_so, len);
        }
        result->captures[i].matched[len] = '\0';
    }
    
    regfree(&regex);
    FERN_FREE(matches);
    return result;
}

/**
 * Free a regex match result.
 * @param match The match to free.
 */
void fern_regex_match_FERN_FREE(FernRegexMatch* match) {
    if (match) {
        FERN_FREE(match->matched);
        FERN_FREE(match);
    }
}

/**
 * Free regex captures.
 * @param captures The captures to free.
 */
void fern_regex_captures_FERN_FREE(FernRegexCaptures* captures) {
    if (captures) {
        for (int64_t i = 0; i < captures->count; i++) {
            FERN_FREE(captures->captures[i].matched);
        }
        FERN_FREE(captures->captures);
        FERN_FREE(captures);
    }
}

/* ========== TUI: Term Module ========== */

#include <sys/ioctl.h>
#include <termios.h>

/**
 * Get terminal dimensions.
 * @return FernTermSize with columns and rows.
 */
FernTermSize* fern_term_size(void) {
    FernTermSize* size = FERN_ALLOC(sizeof(FernTermSize));
    if (!size) return NULL;
    
    struct winsize ws;
    if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &ws) == 0) {
        size->cols = ws.ws_col;
        size->rows = ws.ws_row;
    } else {
        /* Default fallback */
        size->cols = 80;
        size->rows = 24;
    }
    return size;
}

/**
 * Check if stdout is a TTY.
 * @return 1 if TTY, 0 otherwise.
 */
int64_t fern_term_is_tty(void) {
    return isatty(STDOUT_FILENO) ? 1 : 0;
}

/**
 * Get color support level.
 * Checks COLORTERM, TERM, and NO_COLOR environment variables.
 * @return 0 (no color), 16 (basic), 256 (extended), 16777216 (truecolor).
 */
int64_t fern_term_color_support(void) {
    /* NO_COLOR takes precedence */
    const char* no_color = getenv("NO_COLOR");
    if (no_color && no_color[0] != '\0') {
        return 0;
    }
    
    /* Not a TTY means no color */
    if (!isatty(STDOUT_FILENO)) {
        return 0;
    }
    
    /* Check for truecolor support */
    const char* colorterm = getenv("COLORTERM");
    if (colorterm) {
        if (strcmp(colorterm, "truecolor") == 0 || strcmp(colorterm, "24bit") == 0) {
            return 16777216;
        }
    }
    
    /* Check TERM for 256-color support */
    const char* term = getenv("TERM");
    if (term) {
        if (strstr(term, "256color") || strstr(term, "256")) {
            return 256;
        }
        if (strstr(term, "color") || strstr(term, "xterm") || 
            strstr(term, "screen") || strstr(term, "vt100")) {
            return 16;
        }
    }
    
    /* Default to basic 16 colors if TTY */
    return 16;
}

/* ========== TUI: Style Module ========== */

/* ANSI escape code constants */
#define ANSI_RESET "\x1b[0m"
#define ANSI_BOLD "\x1b[1m"
#define ANSI_DIM "\x1b[2m"
#define ANSI_ITALIC "\x1b[3m"
#define ANSI_UNDERLINE "\x1b[4m"
#define ANSI_BLINK "\x1b[5m"
#define ANSI_REVERSE "\x1b[7m"
#define ANSI_STRIKETHROUGH "\x1b[9m"

/* Foreground colors */
#define ANSI_FG_BLACK "\x1b[30m"
#define ANSI_FG_RED "\x1b[31m"
#define ANSI_FG_GREEN "\x1b[32m"
#define ANSI_FG_YELLOW "\x1b[33m"
#define ANSI_FG_BLUE "\x1b[34m"
#define ANSI_FG_MAGENTA "\x1b[35m"
#define ANSI_FG_CYAN "\x1b[36m"
#define ANSI_FG_WHITE "\x1b[37m"

/* Bright foreground colors */
#define ANSI_FG_BRIGHT_BLACK "\x1b[90m"
#define ANSI_FG_BRIGHT_RED "\x1b[91m"
#define ANSI_FG_BRIGHT_GREEN "\x1b[92m"
#define ANSI_FG_BRIGHT_YELLOW "\x1b[93m"
#define ANSI_FG_BRIGHT_BLUE "\x1b[94m"
#define ANSI_FG_BRIGHT_MAGENTA "\x1b[95m"
#define ANSI_FG_BRIGHT_CYAN "\x1b[96m"
#define ANSI_FG_BRIGHT_WHITE "\x1b[97m"

/* Background colors */
#define ANSI_BG_BLACK "\x1b[40m"
#define ANSI_BG_RED "\x1b[41m"
#define ANSI_BG_GREEN "\x1b[42m"
#define ANSI_BG_YELLOW "\x1b[43m"
#define ANSI_BG_BLUE "\x1b[44m"
#define ANSI_BG_MAGENTA "\x1b[45m"
#define ANSI_BG_CYAN "\x1b[46m"
#define ANSI_BG_WHITE "\x1b[47m"

/* ========== Fern Color Palette ========== */
/* Centralized color definitions for TUI components.
 * These can be customized to change the look of Status badges, etc.
 * 256-color codes use format: \x1b[38;5;Nm (fg) or \x1b[48;5;Nm (bg)
 * RGB colors use format: \x1b[38;2;R;G;Bm (fg) or \x1b[48;2;R;G;Bm (bg)
 */

/* Status badge colors - background */
#define FERN_COLOR_WARN_BG   "\x1b[48;5;214m"  /* Orange (256-color) */
#define FERN_COLOR_OKAY_BG   "\x1b[42m"        /* Green */
#define FERN_COLOR_INFO_BG   "\x1b[44m"        /* Blue */
#define FERN_COLOR_FAIL_BG   "\x1b[41m"        /* Red */
#define FERN_COLOR_DBUG_BG   "\x1b[45m"        /* Magenta */

/* Status badge colors - foreground (text) */
#define FERN_COLOR_WARN_FG   "\x1b[30m"        /* Black */
#define FERN_COLOR_OKAY_FG   "\x1b[30m"        /* Black */
#define FERN_COLOR_INFO_FG   "\x1b[37m"        /* White */
#define FERN_COLOR_FAIL_FG   "\x1b[37m"        /* White */
#define FERN_COLOR_DBUG_FG   "\x1b[37m"        /* White */

/* Panel border colors (defaults, can be overridden per-panel) */
#define FERN_COLOR_PANEL_BORDER "\x1b[36m"    /* Cyan */

/**
 * Helper to wrap text with ANSI escape codes.
 * @param prefix The ANSI escape sequence to apply.
 * @param text The text to wrap.
 * @return New string with escape codes.
 */
static char* style_wrap(const char* prefix, const char* text) {
    if (!text) return NULL;
    size_t prefix_len = strlen(prefix);
    size_t text_len = strlen(text);
    size_t reset_len = strlen(ANSI_RESET);
    
    char* result = FERN_ALLOC(prefix_len + text_len + reset_len + 1);
    if (!result) return NULL;
    
    memcpy(result, prefix, prefix_len);
    memcpy(result + prefix_len, text, text_len);
    memcpy(result + prefix_len + text_len, ANSI_RESET, reset_len);
    result[prefix_len + text_len + reset_len] = '\0';
    
    return result;
}

/* Basic colors (foreground) */
char* fern_style_black(const char* text) { return style_wrap(ANSI_FG_BLACK, text); }
char* fern_style_red(const char* text) { return style_wrap(ANSI_FG_RED, text); }
char* fern_style_green(const char* text) { return style_wrap(ANSI_FG_GREEN, text); }
char* fern_style_yellow(const char* text) { return style_wrap(ANSI_FG_YELLOW, text); }
char* fern_style_blue(const char* text) { return style_wrap(ANSI_FG_BLUE, text); }
char* fern_style_magenta(const char* text) { return style_wrap(ANSI_FG_MAGENTA, text); }
char* fern_style_cyan(const char* text) { return style_wrap(ANSI_FG_CYAN, text); }
char* fern_style_white(const char* text) { return style_wrap(ANSI_FG_WHITE, text); }

/* Bright colors (foreground) */
char* fern_style_bright_black(const char* text) { return style_wrap(ANSI_FG_BRIGHT_BLACK, text); }
char* fern_style_bright_red(const char* text) { return style_wrap(ANSI_FG_BRIGHT_RED, text); }
char* fern_style_bright_green(const char* text) { return style_wrap(ANSI_FG_BRIGHT_GREEN, text); }
char* fern_style_bright_yellow(const char* text) { return style_wrap(ANSI_FG_BRIGHT_YELLOW, text); }
char* fern_style_bright_blue(const char* text) { return style_wrap(ANSI_FG_BRIGHT_BLUE, text); }
char* fern_style_bright_magenta(const char* text) { return style_wrap(ANSI_FG_BRIGHT_MAGENTA, text); }
char* fern_style_bright_cyan(const char* text) { return style_wrap(ANSI_FG_BRIGHT_CYAN, text); }
char* fern_style_bright_white(const char* text) { return style_wrap(ANSI_FG_BRIGHT_WHITE, text); }

/* Background colors */
char* fern_style_on_black(const char* text) { return style_wrap(ANSI_BG_BLACK, text); }
char* fern_style_on_red(const char* text) { return style_wrap(ANSI_BG_RED, text); }
char* fern_style_on_green(const char* text) { return style_wrap(ANSI_BG_GREEN, text); }
char* fern_style_on_yellow(const char* text) { return style_wrap(ANSI_BG_YELLOW, text); }
char* fern_style_on_blue(const char* text) { return style_wrap(ANSI_BG_BLUE, text); }
char* fern_style_on_magenta(const char* text) { return style_wrap(ANSI_BG_MAGENTA, text); }
char* fern_style_on_cyan(const char* text) { return style_wrap(ANSI_BG_CYAN, text); }
char* fern_style_on_white(const char* text) { return style_wrap(ANSI_BG_WHITE, text); }

/* Text attributes */
char* fern_style_bold(const char* text) { return style_wrap(ANSI_BOLD, text); }
char* fern_style_dim(const char* text) { return style_wrap(ANSI_DIM, text); }
char* fern_style_italic(const char* text) { return style_wrap(ANSI_ITALIC, text); }
char* fern_style_underline(const char* text) { return style_wrap(ANSI_UNDERLINE, text); }
char* fern_style_blink(const char* text) { return style_wrap(ANSI_BLINK, text); }
char* fern_style_reverse(const char* text) { return style_wrap(ANSI_REVERSE, text); }
char* fern_style_strikethrough(const char* text) { return style_wrap(ANSI_STRIKETHROUGH, text); }

/**
 * Apply 256-color foreground.
 * @param text The text to style.
 * @param color_code Color index 0-255.
 * @return Styled string.
 */
char* fern_style_color(const char* text, int64_t color_code) {
    if (!text) return NULL;
    if (color_code < 0) color_code = 0;
    if (color_code > 255) color_code = 255;
    
    char prefix[20];
    snprintf(prefix, sizeof(prefix), "\x1b[38;5;%ldm", (long)color_code);
    return style_wrap(prefix, text);
}

/**
 * Apply 256-color background.
 * @param text The text to style.
 * @param color_code Color index 0-255.
 * @return Styled string.
 */
char* fern_style_on_color(const char* text, int64_t color_code) {
    if (!text) return NULL;
    if (color_code < 0) color_code = 0;
    if (color_code > 255) color_code = 255;
    
    char prefix[20];
    snprintf(prefix, sizeof(prefix), "\x1b[48;5;%ldm", (long)color_code);
    return style_wrap(prefix, text);
}

/**
 * Apply RGB foreground color.
 * @param text The text to style.
 * @param r Red component 0-255.
 * @param g Green component 0-255.
 * @param b Blue component 0-255.
 * @return Styled string.
 */
char* fern_style_rgb(const char* text, int64_t r, int64_t g, int64_t b) {
    if (!text) return NULL;
    r = (r < 0) ? 0 : (r > 255) ? 255 : r;
    g = (g < 0) ? 0 : (g > 255) ? 255 : g;
    b = (b < 0) ? 0 : (b > 255) ? 255 : b;
    
    char prefix[30];
    snprintf(prefix, sizeof(prefix), "\x1b[38;2;%ld;%ld;%ldm", (long)r, (long)g, (long)b);
    return style_wrap(prefix, text);
}

/**
 * Apply RGB background color.
 * @param text The text to style.
 * @param r Red component 0-255.
 * @param g Green component 0-255.
 * @param b Blue component 0-255.
 * @return Styled string.
 */
char* fern_style_on_rgb(const char* text, int64_t r, int64_t g, int64_t b) {
    if (!text) return NULL;
    r = (r < 0) ? 0 : (r > 255) ? 255 : r;
    g = (g < 0) ? 0 : (g > 255) ? 255 : g;
    b = (b < 0) ? 0 : (b > 255) ? 255 : b;
    
    char prefix[30];
    snprintf(prefix, sizeof(prefix), "\x1b[48;2;%ld;%ld;%ldm", (long)r, (long)g, (long)b);
    return style_wrap(prefix, text);
}

/**
 * Parse hex color string to RGB.
 * @param hex Hex color string like "#ff8800" or "ff8800".
 * @param r Output red component.
 * @param g Output green component.
 * @param b Output blue component.
 * @return 1 on success, 0 on failure.
 */
static int parse_hex_color(const char* hex, int* r, int* g, int* b) {
    if (!hex) return 0;
    
    /* Skip leading # if present */
    if (hex[0] == '#') hex++;
    
    if (strlen(hex) != 6) return 0;
    
    unsigned int rv, gv, bv;
    if (sscanf(hex, "%2x%2x%2x", &rv, &gv, &bv) != 3) return 0;
    
    *r = (int)rv;
    *g = (int)gv;
    *b = (int)bv;
    return 1;
}

/**
 * Apply hex foreground color.
 * @param text The text to style.
 * @param hex_color Hex color string like "#ff8800".
 * @return Styled string.
 */
char* fern_style_hex(const char* text, const char* hex_color) {
    int r, g, b;
    if (!parse_hex_color(hex_color, &r, &g, &b)) {
        /* Invalid hex, return unstyled copy */
        return FERN_STRDUP(text ? text : "");
    }
    return fern_style_rgb(text, r, g, b);
}

/**
 * Apply hex background color.
 * @param text The text to style.
 * @param hex_color Hex color string like "#ff8800".
 * @return Styled string.
 */
char* fern_style_on_hex(const char* text, const char* hex_color) {
    int r, g, b;
    if (!parse_hex_color(hex_color, &r, &g, &b)) {
        /* Invalid hex, return unstyled copy */
        return FERN_STRDUP(text ? text : "");
    }
    return fern_style_on_rgb(text, r, g, b);
}

/**
 * Strip ANSI escape codes from text.
 * @param text The text to strip.
 * @return New string with escape codes removed.
 */
char* fern_style_reset(const char* text) {
    if (!text) return NULL;
    
    size_t len = strlen(text);
    char* result = FERN_ALLOC(len + 1);
    if (!result) return NULL;
    
    size_t j = 0;
    for (size_t i = 0; i < len; i++) {
        if (text[i] == '\x1b') {
            /* Skip escape sequence: \x1b[...m */
            while (i < len && text[i] != 'm') i++;
        } else {
            result[j++] = text[i];
        }
    }
    result[j] = '\0';
    
    return result;
}

/* ========== TUI: Status Badges ========== */

/**
 * Create a status badge with background color and text color.
 * All badges are 8 visible chars: space + 6-char centered label + space.
 * @param label The badge text (e.g., "WARN", "OK").
 * @param bg_color Background color code.
 * @param fg_color Foreground (text) color code.
 * @return Formatted badge string with colors.
 */
static char* make_badge(const char* label, const char* bg_color, const char* fg_color) {
    if (!label) return FERN_STRDUP("");
    
    /* All labels are 4 chars: WARN, OKAY, INFO, FAIL, DBUG */
    const size_t inner_width = 4;
    size_t label_len = strlen(label);
    size_t total_pad = inner_width - label_len;
    size_t pad_left = total_pad / 2;  /* Round down - extra space goes right */
    size_t pad_right = total_pad - pad_left;
    
    size_t bg_len = bg_color ? strlen(bg_color) : 0;
    size_t fg_len = fg_color ? strlen(fg_color) : 0;
    size_t bold_len = strlen(ANSI_BOLD);
    size_t reset_len = strlen(ANSI_RESET);
    
    /* Total: colors + bold + outer_space + pad_left + label + pad_right + outer_space + reset + null */
    size_t total = bg_len + fg_len + bold_len + 1 + pad_left + label_len + pad_right + 1 + reset_len + 1;
    char* result = FERN_ALLOC(total);
    if (!result) return FERN_STRDUP("");
    
    char* p = result;
    if (bg_color) { memcpy(p, bg_color, bg_len); p += bg_len; }
    if (fg_color) { memcpy(p, fg_color, fg_len); p += fg_len; }
    memcpy(p, ANSI_BOLD, bold_len); p += bold_len;
    *p++ = ' ';
    for (size_t i = 0; i < pad_left; i++) *p++ = ' ';
    memcpy(p, label, label_len); p += label_len;
    for (size_t i = 0; i < pad_right; i++) *p++ = ' ';
    *p++ = ' ';
    memcpy(p, ANSI_RESET, reset_len); p += reset_len;
    *p = '\0';
    
    return result;
}

/**
 * Create a WARN badge (yellow/orange background, black text).
 * @param message Optional message to append after badge.
 * @return Formatted string like "[WARN] message".
 */
char* fern_status_warn(const char* message) {
    char* badge = make_badge("WARN", FERN_COLOR_WARN_BG, FERN_COLOR_WARN_FG);
    if (!message || message[0] == '\0') return badge;
    
    size_t badge_len = strlen(badge);
    size_t msg_len = strlen(message);
    char* result = FERN_ALLOC(badge_len + 1 + msg_len + 1);
    if (!result) { FERN_FREE(badge); return FERN_STRDUP(""); }
    
    memcpy(result, badge, badge_len);
    result[badge_len] = ' ';
    memcpy(result + badge_len + 1, message, msg_len);
    result[badge_len + 1 + msg_len] = '\0';
    
    FERN_FREE(badge);
    return result;
}

/**
 * Create an OK badge (green background, black text).
 * @param message Optional message to append after badge.
 * @return Formatted string like "[OK] message".
 */
char* fern_status_ok(const char* message) {
    char* badge = make_badge("OKAY", FERN_COLOR_OKAY_BG, FERN_COLOR_OKAY_FG);
    if (!message || message[0] == '\0') return badge;
    
    size_t badge_len = strlen(badge);
    size_t msg_len = strlen(message);
    char* result = FERN_ALLOC(badge_len + 1 + msg_len + 1);
    if (!result) { FERN_FREE(badge); return FERN_STRDUP(""); }
    
    memcpy(result, badge, badge_len);
    result[badge_len] = ' ';
    memcpy(result + badge_len + 1, message, msg_len);
    result[badge_len + 1 + msg_len] = '\0';
    
    FERN_FREE(badge);
    return result;
}

/**
 * Create an INFO badge (blue background, white text).
 * @param message Optional message to append after badge.
 * @return Formatted string like "[INFO] message".
 */
char* fern_status_info(const char* message) {
    char* badge = make_badge("INFO", FERN_COLOR_INFO_BG, FERN_COLOR_INFO_FG);
    if (!message || message[0] == '\0') return badge;
    
    size_t badge_len = strlen(badge);
    size_t msg_len = strlen(message);
    char* result = FERN_ALLOC(badge_len + 1 + msg_len + 1);
    if (!result) { FERN_FREE(badge); return FERN_STRDUP(""); }
    
    memcpy(result, badge, badge_len);
    result[badge_len] = ' ';
    memcpy(result + badge_len + 1, message, msg_len);
    result[badge_len + 1 + msg_len] = '\0';
    
    FERN_FREE(badge);
    return result;
}

/**
 * Create an ERROR badge (red background, white text).
 * @param message Optional message to append after badge.
 * @return Formatted string like "[ERROR] message".
 */
char* fern_status_error(const char* message) {
    char* badge = make_badge("FAIL", FERN_COLOR_FAIL_BG, FERN_COLOR_FAIL_FG);
    if (!message || message[0] == '\0') return badge;
    
    size_t badge_len = strlen(badge);
    size_t msg_len = strlen(message);
    char* result = FERN_ALLOC(badge_len + 1 + msg_len + 1);
    if (!result) { FERN_FREE(badge); return FERN_STRDUP(""); }
    
    memcpy(result, badge, badge_len);
    result[badge_len] = ' ';
    memcpy(result + badge_len + 1, message, msg_len);
    result[badge_len + 1 + msg_len] = '\0';
    
    FERN_FREE(badge);
    return result;
}

/**
 * Create a DEBUG badge (magenta background, white text).
 * @param message Optional message to append after badge.
 * @return Formatted string like "[DEBUG] message".
 */
char* fern_status_debug(const char* message) {
    char* badge = make_badge("DBUG", FERN_COLOR_DBUG_BG, FERN_COLOR_DBUG_FG);
    if (!message || message[0] == '\0') return badge;
    
    size_t badge_len = strlen(badge);
    size_t msg_len = strlen(message);
    char* result = FERN_ALLOC(badge_len + 1 + msg_len + 1);
    if (!result) { FERN_FREE(badge); return FERN_STRDUP(""); }
    
    memcpy(result, badge, badge_len);
    result[badge_len] = ' ';
    memcpy(result + badge_len + 1, message, msg_len);
    result[badge_len + 1 + msg_len] = '\0';
    
    FERN_FREE(badge);
    return result;
}

/* ========== TUI: Live/Spinner Module ========== */

/**
 * Print text without a newline (for same-line updates).
 * @param text The text to print.
 */
void fern_live_print(const char* text) {
    if (text) {
        printf("%s", text);
        fflush(stdout);
    }
}

/**
 * Clear the current line and move cursor to beginning.
 */
void fern_live_clear_line(void) {
    printf("\r\x1b[K");
    fflush(stdout);
}

/**
 * Update the current line with new text (clears line first).
 * @param text The new text to display.
 */
void fern_live_update(const char* text) {
    fern_live_clear_line();
    fern_live_print(text);
}

/**
 * Finish live update by printing newline.
 */
void fern_live_done(void) {
    printf("\n");
    fflush(stdout);
}

/**
 * Sleep for specified milliseconds.
 * @param ms Milliseconds to sleep.
 */
void fern_sleep_ms(int64_t ms) {
    if (ms > 0) {
        usleep((useconds_t)(ms * 1000));
    }
}

/* ========== TUI: Panel Module ========== */

/* Box drawing characters for each style */
typedef struct {
    const char* top_left;
    const char* top;
    const char* top_right;
    const char* left;
    const char* right;
    const char* bottom_left;
    const char* bottom;
    const char* bottom_right;
} BoxChars;

static const BoxChars BOX_CHARS[] = {
    /* BOX_ROUNDED */  { "╭", "─", "╮", "│", "│", "╰", "─", "╯" },
    /* BOX_SQUARE */   { "┌", "─", "┐", "│", "│", "└", "─", "┘" },
    /* BOX_DOUBLE */   { "╔", "═", "╗", "║", "║", "╚", "═", "╝" },
    /* BOX_HEAVY */    { "┏", "━", "┓", "┃", "┃", "┗", "━", "┛" },
    /* BOX_ASCII */    { "+", "-", "+", "|", "|", "+", "-", "+" },
    /* BOX_NONE */     { " ", " ", " ", " ", " ", " ", " ", " " },
};

/**
 * Get display width of a string (ignoring ANSI codes).
 * @param s The string.
 * @return Display width in characters.
 */
static size_t display_width(const char* s) {
    if (!s) return 0;
    size_t width = 0;
    size_t len = strlen(s);
    for (size_t i = 0; i < len; i++) {
        if (s[i] == '\n' || s[i] == '\r') {
            /* Don't count newlines as width */
            continue;
        } else if (s[i] == '\x1b') {
            /* Skip ANSI escape sequence */
            while (i < len && s[i] != 'm') i++;
        } else if ((unsigned char)s[i] >= 0x80) {
            /* UTF-8 multi-byte: skip continuation bytes */
            /* This is a simplification - assumes 1 display width per codepoint */
            if ((unsigned char)s[i] >= 0xC0) width++;
            /* Skip continuation bytes (0x80-0xBF) */
        } else {
            width++;
        }
    }
    return width;
}

/**
 * Get the maximum line width from a multi-line string.
 * @param s The string (may contain newlines).
 * @return Maximum display width of any line.
 */
static size_t max_line_width(const char* s) {
    if (!s) return 0;
    size_t max_width = 0;
    size_t current_width = 0;
    size_t len = strlen(s);
    
    for (size_t i = 0; i < len; i++) {
        if (s[i] == '\n') {
            if (current_width > max_width) max_width = current_width;
            current_width = 0;
        } else if (s[i] == '\r') {
            /* Skip carriage return */
            continue;
        } else if (s[i] == '\x1b') {
            /* Skip ANSI escape sequence */
            while (i < len && s[i] != 'm') i++;
        } else if ((unsigned char)s[i] >= 0x80) {
            if ((unsigned char)s[i] >= 0xC0) current_width++;
        } else {
            current_width++;
        }
    }
    /* Check last line (if no trailing newline) */
    if (current_width > max_width) max_width = current_width;
    
    return max_width;
}

/**
 * Repeat a string n times.
 * @param s The string to repeat.
 * @param n Number of repetitions.
 * @return New string.
 */
static char* str_repeat(const char* s, size_t n) {
    size_t slen = strlen(s);
    char* result = FERN_ALLOC(slen * n + 1);
    if (!result) return NULL;
    result[0] = '\0';
    for (size_t i = 0; i < n; i++) {
        strcat(result, s);
    }
    return result;
}

/**
 * Pad string to width (left-aligned).
 * @param s The string.
 * @param width Target width.
 * @return Padded string.
 */
static char* pad_right(const char* s, size_t width) {
    size_t dw = display_width(s);
    if (dw >= width) return FERN_STRDUP(s);
    
    size_t padding = width - dw;
    size_t slen = strlen(s);
    char* result = FERN_ALLOC(slen + padding + 1);
    if (!result) return NULL;
    
    strcpy(result, s);
    for (size_t i = 0; i < padding; i++) {
        result[slen + i] = ' ';
    }
    result[slen + padding] = '\0';
    return result;
}

/**
 * Center string within width.
 * @param s The string.
 * @param width Target width.
 * @return Centered string.
 */
static char* center_text(const char* s, size_t width) {
    size_t dw = display_width(s);
    if (dw >= width) return FERN_STRDUP(s);
    
    size_t total_padding = width - dw;
    size_t left_pad = total_padding / 2;
    size_t right_pad = total_padding - left_pad;
    size_t slen = strlen(s);
    
    char* result = FERN_ALLOC(left_pad + slen + right_pad + 1);
    if (!result) return NULL;
    
    for (size_t i = 0; i < left_pad; i++) result[i] = ' ';
    strcpy(result + left_pad, s);
    for (size_t i = 0; i < right_pad; i++) result[left_pad + slen + i] = ' ';
    result[left_pad + slen + right_pad] = '\0';
    return result;
}

FernPanel* fern_panel_new(const char* content) {
    FernPanel* panel = FERN_ALLOC(sizeof(FernPanel));
    if (!panel) return NULL;
    
    panel->content = content ? FERN_STRDUP(content) : FERN_STRDUP("");
    panel->title = NULL;
    panel->subtitle = NULL;
    panel->border_color = NULL;
    panel->box_style = BOX_ROUNDED;
    panel->width = 0;  /* Auto */
    panel->padding_h = 1;
    panel->padding_v = 0;
    
    return panel;
}

FernPanel* fern_panel_title(FernPanel* panel, const char* title) {
    if (!panel) return NULL;
    if (panel->title) FERN_FREE(panel->title);
    panel->title = title ? FERN_STRDUP(title) : NULL;
    return panel;
}

FernPanel* fern_panel_subtitle(FernPanel* panel, const char* subtitle) {
    if (!panel) return NULL;
    if (panel->subtitle) FERN_FREE(panel->subtitle);
    panel->subtitle = subtitle ? FERN_STRDUP(subtitle) : NULL;
    return panel;
}

FernPanel* fern_panel_border(FernPanel* panel, int64_t style) {
    if (!panel) return NULL;
    if (style >= 0 && style <= 5) {
        panel->box_style = (FernBoxStyle)style;
    }
    return panel;
}

FernPanel* fern_panel_border_str(FernPanel* panel, const char* style) {
    if (!panel || !style) return panel;
    /* Convert style name to enum value */
    if (strcmp(style, "rounded") == 0) panel->box_style = BOX_ROUNDED;
    else if (strcmp(style, "square") == 0) panel->box_style = BOX_SQUARE;
    else if (strcmp(style, "double") == 0) panel->box_style = BOX_DOUBLE;
    else if (strcmp(style, "heavy") == 0) panel->box_style = BOX_HEAVY;
    else if (strcmp(style, "ascii") == 0) panel->box_style = BOX_ASCII;
    else if (strcmp(style, "none") == 0) panel->box_style = BOX_NONE;
    return panel;
}

FernPanel* fern_panel_width(FernPanel* panel, int64_t width) {
    if (!panel) return NULL;
    panel->width = width;
    return panel;
}

FernPanel* fern_panel_padding(FernPanel* panel, int64_t vertical, int64_t horizontal) {
    if (!panel) return NULL;
    panel->padding_v = vertical;
    panel->padding_h = horizontal;
    return panel;
}

FernPanel* fern_panel_border_color(FernPanel* panel, const char* color) {
    if (!panel) return NULL;
    FERN_FREE(panel->border_color);
    panel->border_color = color ? FERN_STRDUP(color) : NULL;
    return panel;
}

/**
 * Get ANSI color code for a color name or hex value.
 * @param color Color name or hex (e.g., "red", "cyan", "#00ff00").
 * @return ANSI escape sequence string (caller must free).
 */
static char* get_ansi_color(const char* color) {
    if (!color || color[0] == '\0') return FERN_STRDUP("");
    
    /* Handle hex colors */
    if (color[0] == '#' && strlen(color) == 7) {
        int r, g, b;
        if (sscanf(color + 1, "%02x%02x%02x", &r, &g, &b) == 3) {
            char* result = FERN_ALLOC(32);
            if (result) {
                snprintf(result, 32, "\x1b[38;2;%d;%d;%dm", r, g, b);
            }
            return result;
        }
    }
    
    /* Named colors */
    if (strcmp(color, "black") == 0) return FERN_STRDUP("\x1b[30m");
    if (strcmp(color, "red") == 0) return FERN_STRDUP("\x1b[31m");
    if (strcmp(color, "green") == 0) return FERN_STRDUP("\x1b[32m");
    if (strcmp(color, "yellow") == 0) return FERN_STRDUP("\x1b[33m");
    if (strcmp(color, "blue") == 0) return FERN_STRDUP("\x1b[34m");
    if (strcmp(color, "magenta") == 0) return FERN_STRDUP("\x1b[35m");
    if (strcmp(color, "cyan") == 0) return FERN_STRDUP("\x1b[36m");
    if (strcmp(color, "white") == 0) return FERN_STRDUP("\x1b[37m");
    /* Bright colors */
    if (strcmp(color, "bright_black") == 0) return FERN_STRDUP("\x1b[90m");
    if (strcmp(color, "bright_red") == 0) return FERN_STRDUP("\x1b[91m");
    if (strcmp(color, "bright_green") == 0) return FERN_STRDUP("\x1b[92m");
    if (strcmp(color, "bright_yellow") == 0) return FERN_STRDUP("\x1b[93m");
    if (strcmp(color, "bright_blue") == 0) return FERN_STRDUP("\x1b[94m");
    if (strcmp(color, "bright_magenta") == 0) return FERN_STRDUP("\x1b[95m");
    if (strcmp(color, "bright_cyan") == 0) return FERN_STRDUP("\x1b[96m");
    if (strcmp(color, "bright_white") == 0) return FERN_STRDUP("\x1b[97m");
    
    return FERN_STRDUP("");
}

char* fern_panel_render(FernPanel* panel) {
    /* FERN_STYLE: allow(function-length) panel rendering with multi-line support */
    if (!panel) return FERN_STRDUP("");
    
    const BoxChars* box = &BOX_CHARS[panel->box_style];
    
    /* Get border color codes */
    char* color_start = get_ansi_color(panel->border_color);
    const char* color_end = (color_start && color_start[0] != '\0') ? "\x1b[0m" : "";
    
    /* Calculate content width - use max line width for multi-line content */
    size_t content_width = max_line_width(panel->content);
    size_t title_width = panel->title ? display_width(panel->title) + 2 : 0;  /* +2 for spaces */
    size_t subtitle_width = panel->subtitle ? display_width(panel->subtitle) + 2 : 0;
    
    /* Determine panel width */
    size_t inner_width = content_width;
    if (title_width > inner_width) inner_width = title_width;
    if (subtitle_width > inner_width) inner_width = subtitle_width;
    inner_width += panel->padding_h * 2;
    
    if (panel->width > 0) {
        inner_width = (size_t)panel->width - 2;  /* -2 for borders */
    } else if (panel->width == -1) {
        /* Expand to terminal width */
        FernTermSize* size = fern_term_size();
        if (size) {
            inner_width = (size_t)size->cols - 2;
            FERN_FREE(size);
        }
    }
    
    /* Build result string */
    size_t result_cap = 8192;
    char* result = FERN_ALLOC(result_cap);
    if (!result) return FERN_STRDUP("");
    result[0] = '\0';
    
    /* Top border with optional title */
    strcat(result, color_start);
    strcat(result, box->top_left);
    if (panel->title && panel->title[0] != '\0') {
        size_t title_len = display_width(panel->title);
        size_t side_len = (inner_width - title_len - 2) / 2;
        char* side = str_repeat(box->top, side_len);
        strcat(result, side);
        strcat(result, color_end);
        strcat(result, " ");
        strcat(result, panel->title);
        strcat(result, " ");
        strcat(result, color_start);
        FERN_FREE(side);
        side = str_repeat(box->top, inner_width - side_len - title_len - 2);
        strcat(result, side);
        FERN_FREE(side);
    } else {
        char* top_line = str_repeat(box->top, inner_width);
        strcat(result, top_line);
        FERN_FREE(top_line);
    }
    strcat(result, box->top_right);
    strcat(result, color_end);
    strcat(result, "\n");
    
    /* Vertical padding (top) */
    for (int64_t i = 0; i < panel->padding_v; i++) {
        strcat(result, color_start);
        strcat(result, box->left);
        strcat(result, color_end);
        char* spaces = str_repeat(" ", inner_width);
        strcat(result, spaces);
        FERN_FREE(spaces);
        strcat(result, color_start);
        strcat(result, box->right);
        strcat(result, color_end);
        strcat(result, "\n");
    }
    
    /* Content lines - split by newlines and render each line */
    char* h_pad = str_repeat(" ", panel->padding_h);
    size_t line_width = inner_width - panel->padding_h * 2;
    
    /* Split content into lines */
    const char* content = panel->content;
    const char* line_start = content;
    const char* p = content;
    
    while (*p != '\0') {
        if (*p == '\n') {
            /* Render this line */
            size_t line_len = (size_t)(p - line_start);
            char* line = FERN_ALLOC(line_len + 1);
            if (line) {
                memcpy(line, line_start, line_len);
                line[line_len] = '\0';
                
                strcat(result, color_start);
                strcat(result, box->left);
                strcat(result, color_end);
                strcat(result, h_pad);
                char* padded_line = pad_right(line, line_width);
                strcat(result, padded_line);
                FERN_FREE(padded_line);
                strcat(result, h_pad);
                strcat(result, color_start);
                strcat(result, box->right);
                strcat(result, color_end);
                strcat(result, "\n");
                FERN_FREE(line);
            }
            line_start = p + 1;
        }
        p++;
    }
    
    /* Render last line (or only line if no newlines) */
    if (line_start <= p) {
        size_t line_len = (size_t)(p - line_start);
        if (line_len > 0 || line_start == content) {
            char* line = FERN_ALLOC(line_len + 1);
            if (line) {
                memcpy(line, line_start, line_len);
                line[line_len] = '\0';
                
                strcat(result, color_start);
                strcat(result, box->left);
                strcat(result, color_end);
                strcat(result, h_pad);
                char* padded_line = pad_right(line, line_width);
                strcat(result, padded_line);
                FERN_FREE(padded_line);
                strcat(result, h_pad);
                strcat(result, color_start);
                strcat(result, box->right);
                strcat(result, color_end);
                strcat(result, "\n");
                FERN_FREE(line);
            }
        }
    }
    
    FERN_FREE(h_pad);
    
    /* Vertical padding (bottom) */
    for (int64_t i = 0; i < panel->padding_v; i++) {
        strcat(result, color_start);
        strcat(result, box->left);
        strcat(result, color_end);
        char* spaces = str_repeat(" ", inner_width);
        strcat(result, spaces);
        FERN_FREE(spaces);
        strcat(result, color_start);
        strcat(result, box->right);
        strcat(result, color_end);
        strcat(result, "\n");
    }
    
    /* Bottom border with optional subtitle */
    strcat(result, color_start);
    strcat(result, box->bottom_left);
    if (panel->subtitle && panel->subtitle[0] != '\0') {
        size_t subtitle_len = display_width(panel->subtitle);
        size_t side_len = (inner_width - subtitle_len - 2) / 2;
        char* side = str_repeat(box->bottom, side_len);
        strcat(result, side);
        strcat(result, color_end);
        strcat(result, " ");
        strcat(result, panel->subtitle);
        strcat(result, " ");
        strcat(result, color_start);
        FERN_FREE(side);
        side = str_repeat(box->bottom, inner_width - side_len - subtitle_len - 2);
        strcat(result, side);
        FERN_FREE(side);
    } else {
        char* bottom_line = str_repeat(box->bottom, inner_width);
        strcat(result, bottom_line);
        FERN_FREE(bottom_line);
    }
    strcat(result, box->bottom_right);
    strcat(result, color_end);
    
    FERN_FREE(color_start);
    
    return result;
}

void fern_panel_FERN_FREE(FernPanel* panel) {
    if (panel) {
        FERN_FREE(panel->content);
        FERN_FREE(panel->title);
        FERN_FREE(panel->subtitle);
        FERN_FREE(panel->border_color);
        FERN_FREE(panel);
    }
}

/* ========== TUI: Table Module ========== */

FernTable* fern_table_new(void) {
    FernTable* table = FERN_ALLOC(sizeof(FernTable));
    if (!table) return NULL;
    
    table->title = NULL;
    table->caption = NULL;
    table->columns = NULL;
    table->column_count = 0;
    table->rows = NULL;
    table->row_count = 0;
    table->row_capacity = 0;
    table->box_style = BOX_ROUNDED;
    table->show_header = 1;
    table->show_lines = 0;
    table->expand = 0;
    
    return table;
}

FernTable* fern_table_add_column(FernTable* table, const char* header) {
    if (!table) return NULL;
    
    table->column_count++;
    table->columns = FERN_REALLOC(table->columns, table->column_count * sizeof(FernTableColumn));
    if (!table->columns) return NULL;
    
    FernTableColumn* col = &table->columns[table->column_count - 1];
    col->header = header ? FERN_STRDUP(header) : FERN_STRDUP("");
    col->min_width = 0;
    col->max_width = 0;
    col->justify = 0;  /* Left */
    
    return table;
}

FernTable* fern_table_add_row(FernTable* table, FernStringList* cells) {
    if (!table || !cells) return NULL;
    
    /* Grow rows array if needed */
    if (table->row_count >= table->row_capacity) {
        table->row_capacity = table->row_capacity == 0 ? 8 : table->row_capacity * 2;
        table->rows = FERN_REALLOC(table->rows, table->row_capacity * sizeof(FernTableRow));
        if (!table->rows) return NULL;
    }
    
    FernTableRow* row = &table->rows[table->row_count++];
    row->cell_count = cells->len;
    row->cells = FERN_ALLOC(cells->len * sizeof(char*));
    if (!row->cells) return NULL;
    
    for (int64_t i = 0; i < cells->len; i++) {
        row->cells[i] = cells->data[i] ? FERN_STRDUP(cells->data[i]) : FERN_STRDUP("");
    }
    
    return table;
}

FernTable* fern_table_title(FernTable* table, const char* title) {
    if (!table) return NULL;
    if (table->title) FERN_FREE(table->title);
    table->title = title ? FERN_STRDUP(title) : NULL;
    return table;
}

FernTable* fern_table_border(FernTable* table, int64_t style) {
    if (!table) return NULL;
    if (style >= 0 && style <= 5) {
        table->box_style = (FernBoxStyle)style;
    }
    return table;
}

FernTable* fern_table_show_header(FernTable* table, int64_t show) {
    if (!table) return NULL;
    table->show_header = show ? 1 : 0;
    return table;
}

char* fern_table_render(FernTable* table) {
    if (!table || table->column_count == 0) return FERN_STRDUP("");
    
    const BoxChars* box = &BOX_CHARS[table->box_style];
    
    /* Calculate column widths */
    size_t* col_widths = FERN_ALLOC(table->column_count * sizeof(size_t));
    if (!col_widths) return FERN_STRDUP("");
    
    for (int64_t i = 0; i < table->column_count; i++) {
        col_widths[i] = display_width(table->columns[i].header);
    }
    
    /* Find max width from rows */
    for (int64_t r = 0; r < table->row_count; r++) {
        FernTableRow* row = &table->rows[r];
        for (int64_t c = 0; c < row->cell_count && c < table->column_count; c++) {
            size_t w = display_width(row->cells[c]);
            if (w > col_widths[c]) col_widths[c] = w;
        }
    }
    
    /* Add padding */
    for (int64_t i = 0; i < table->column_count; i++) {
        col_widths[i] += 2;  /* Padding on each side */
    }
    
    /* Build result */
    size_t result_cap = 8192;
    char* result = FERN_ALLOC(result_cap);
    if (!result) { FERN_FREE(col_widths); return FERN_STRDUP(""); }
    result[0] = '\0';
    
    /* Top border */
    strcat(result, box->top_left);
    for (int64_t i = 0; i < table->column_count; i++) {
        char* line = str_repeat(box->top, col_widths[i]);
        strcat(result, line);
        FERN_FREE(line);
        if (i < table->column_count - 1) {
            /* Use top for separator (simplified) */
            strcat(result, box->top);
        }
    }
    strcat(result, box->top_right);
    strcat(result, "\n");
    
    /* Header row */
    if (table->show_header) {
        strcat(result, box->left);
        for (int64_t i = 0; i < table->column_count; i++) {
            char* centered = center_text(table->columns[i].header, col_widths[i]);
            strcat(result, centered);
            FERN_FREE(centered);
            if (i < table->column_count - 1) {
                strcat(result, box->left);
            }
        }
        strcat(result, box->right);
        strcat(result, "\n");
        
        /* Header separator */
        strcat(result, box->left);
        for (int64_t i = 0; i < table->column_count; i++) {
            char* line = str_repeat(box->top, col_widths[i]);
            strcat(result, line);
            FERN_FREE(line);
            if (i < table->column_count - 1) {
                strcat(result, box->top);
            }
        }
        strcat(result, box->right);
        strcat(result, "\n");
    }
    
    /* Data rows */
    for (int64_t r = 0; r < table->row_count; r++) {
        FernTableRow* row = &table->rows[r];
        strcat(result, box->left);
        for (int64_t c = 0; c < table->column_count; c++) {
            const char* cell = (c < row->cell_count) ? row->cells[c] : "";
            char* padded = pad_right(cell, col_widths[c] - 1);
            strcat(result, " ");
            strcat(result, padded);
            FERN_FREE(padded);
            if (c < table->column_count - 1) {
                strcat(result, box->left);
            }
        }
        strcat(result, box->right);
        strcat(result, "\n");
    }
    
    /* Bottom border */
    strcat(result, box->bottom_left);
    for (int64_t i = 0; i < table->column_count; i++) {
        char* line = str_repeat(box->bottom, col_widths[i]);
        strcat(result, line);
        FERN_FREE(line);
        if (i < table->column_count - 1) {
            strcat(result, box->bottom);
        }
    }
    strcat(result, box->bottom_right);
    
    FERN_FREE(col_widths);
    return result;
}

void fern_table_FERN_FREE(FernTable* table) {
    if (table) {
        FERN_FREE(table->title);
        FERN_FREE(table->caption);
        for (int64_t i = 0; i < table->column_count; i++) {
            FERN_FREE(table->columns[i].header);
        }
        FERN_FREE(table->columns);
        for (int64_t r = 0; r < table->row_count; r++) {
            for (int64_t c = 0; c < table->rows[r].cell_count; c++) {
                FERN_FREE(table->rows[r].cells[c]);
            }
            FERN_FREE(table->rows[r].cells);
        }
        FERN_FREE(table->rows);
        FERN_FREE(table);
    }
}

/* ========== Progress Bar Module ========== */

FernProgress* fern_progress_new(int64_t total) {
    FernProgress* p = FERN_ALLOC(sizeof(FernProgress));
    if (!p) return NULL;
    
    p->total = total > 0 ? total : 100;
    p->completed = 0;
    p->width = 40;
    p->description = NULL;
    p->fill_char = FERN_STRDUP("█");
    p->empty_char = FERN_STRDUP("░");
    p->show_percentage = 1;
    p->show_count = 0;
    
    return p;
}

FernProgress* fern_progress_description(FernProgress* p, const char* desc) {
    if (!p) return NULL;
    if (p->description) FERN_FREE(p->description);
    p->description = desc ? FERN_STRDUP(desc) : NULL;
    return p;
}

FernProgress* fern_progress_width(FernProgress* p, int64_t width) {
    if (!p) return NULL;
    p->width = width > 0 ? width : 40;
    return p;
}

FernProgress* fern_progress_advance(FernProgress* p) {
    if (!p) return NULL;
    if (p->completed < p->total) {
        p->completed++;
    }
    return p;
}

FernProgress* fern_progress_set(FernProgress* p, int64_t value) {
    if (!p) return NULL;
    if (value < 0) value = 0;
    if (value > p->total) value = p->total;
    p->completed = value;
    return p;
}

char* fern_progress_render(FernProgress* p) {
    if (!p) return FERN_STRDUP("");
    
    double ratio = p->total > 0 ? (double)p->completed / p->total : 0;
    int filled = (int)(ratio * p->width);
    int empty = (int)p->width - filled;
    
    /* Calculate buffer size */
    size_t desc_len = p->description ? strlen(p->description) + 1 : 0;
    size_t fill_len = strlen(p->fill_char);
    size_t empty_len = strlen(p->empty_char);
    size_t buf_size = desc_len + filled * fill_len + empty * empty_len + 50;
    
    char* result = FERN_ALLOC(buf_size);
    if (!result) return FERN_STRDUP("");
    result[0] = '\0';
    
    /* Description */
    if (p->description) {
        strcat(result, p->description);
        strcat(result, " ");
    }
    
    /* Opening bracket */
    strcat(result, "[");
    
    /* Filled portion */
    for (int i = 0; i < filled; i++) {
        strcat(result, p->fill_char);
    }
    
    /* Empty portion */
    for (int i = 0; i < empty; i++) {
        strcat(result, p->empty_char);
    }
    
    /* Closing bracket */
    strcat(result, "]");
    
    /* Percentage */
    if (p->show_percentage) {
        char pct[20];
        snprintf(pct, sizeof(pct), " %3d%%", (int)(ratio * 100));
        strcat(result, pct);
    }
    
    /* Count */
    if (p->show_count) {
        char count[40];
        snprintf(count, sizeof(count), " (%lld/%lld)", 
            (long long)p->completed, (long long)p->total);
        strcat(result, count);
    }
    
    return result;
}

void fern_progress_FERN_FREE(FernProgress* p) {
    if (p) {
        FERN_FREE(p->description);
        FERN_FREE(p->fill_char);
        FERN_FREE(p->empty_char);
        FERN_FREE(p);
    }
}

/* ========== Spinner Module ========== */

/* Spinner frame sequences */
static const char* SPINNER_FRAMES_DOTS[] = {
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"
};
static const int SPINNER_FRAMES_DOTS_COUNT = 10;

static const char* SPINNER_FRAMES_LINE[] = {
    "-", "\\", "|", "/"
};
static const int SPINNER_FRAMES_LINE_COUNT = 4;

static const char* SPINNER_FRAMES_CIRCLE[] = {
    "◐", "◓", "◑", "◒"
};
static const int SPINNER_FRAMES_CIRCLE_COUNT = 4;

static const char* SPINNER_FRAMES_SQUARE[] = {
    "◰", "◳", "◲", "◱"
};
static const int SPINNER_FRAMES_SQUARE_COUNT = 4;

static const char* SPINNER_FRAMES_ARROW[] = {
    "←", "↖", "↑", "↗", "→", "↘", "↓", "↙"
};
static const int SPINNER_FRAMES_ARROW_COUNT = 8;

static const char* SPINNER_FRAMES_BOUNCE[] = {
    "⠁", "⠂", "⠄", "⠂"
};
static const int SPINNER_FRAMES_BOUNCE_COUNT = 4;

static const char* SPINNER_FRAMES_CLOCK[] = {
    "🕐", "🕑", "🕒", "🕓", "🕔", "🕕", "🕖", "🕗", "🕘", "🕙", "🕚", "🕛"
};
static const int SPINNER_FRAMES_CLOCK_COUNT = 12;

FernSpinner* fern_spinner_new(void) {
    FernSpinner* s = FERN_ALLOC(sizeof(FernSpinner));
    if (!s) return NULL;
    
    s->style = SPINNER_DOTS;
    s->frame = 0;
    s->message = NULL;
    
    return s;
}

FernSpinner* fern_spinner_message(FernSpinner* s, const char* message) {
    if (!s) return NULL;
    if (s->message) FERN_FREE(s->message);
    s->message = message ? FERN_STRDUP(message) : NULL;
    return s;
}

FernSpinner* fern_spinner_style(FernSpinner* s, const char* style) {
    if (!s || !style) return s;
    
    if (strcmp(style, "dots") == 0) s->style = SPINNER_DOTS;
    else if (strcmp(style, "line") == 0) s->style = SPINNER_LINE;
    else if (strcmp(style, "circle") == 0) s->style = SPINNER_CIRCLE;
    else if (strcmp(style, "square") == 0) s->style = SPINNER_SQUARE;
    else if (strcmp(style, "arrow") == 0) s->style = SPINNER_ARROW;
    else if (strcmp(style, "bounce") == 0) s->style = SPINNER_BOUNCE;
    else if (strcmp(style, "clock") == 0) s->style = SPINNER_CLOCK;
    
    s->frame = 0;  /* Reset frame on style change */
    return s;
}

FernSpinner* fern_spinner_tick(FernSpinner* s) {
    if (!s) return NULL;
    
    int count = 0;
    switch (s->style) {
        case SPINNER_DOTS:   count = SPINNER_FRAMES_DOTS_COUNT; break;
        case SPINNER_LINE:   count = SPINNER_FRAMES_LINE_COUNT; break;
        case SPINNER_CIRCLE: count = SPINNER_FRAMES_CIRCLE_COUNT; break;
        case SPINNER_SQUARE: count = SPINNER_FRAMES_SQUARE_COUNT; break;
        case SPINNER_ARROW:  count = SPINNER_FRAMES_ARROW_COUNT; break;
        case SPINNER_BOUNCE: count = SPINNER_FRAMES_BOUNCE_COUNT; break;
        case SPINNER_CLOCK:  count = SPINNER_FRAMES_CLOCK_COUNT; break;
    }
    
    s->frame = (s->frame + 1) % count;
    return s;
}

char* fern_spinner_render(FernSpinner* s) {
    if (!s) return FERN_STRDUP("");
    
    const char* frame = NULL;
    switch (s->style) {
        case SPINNER_DOTS:   frame = SPINNER_FRAMES_DOTS[s->frame]; break;
        case SPINNER_LINE:   frame = SPINNER_FRAMES_LINE[s->frame]; break;
        case SPINNER_CIRCLE: frame = SPINNER_FRAMES_CIRCLE[s->frame]; break;
        case SPINNER_SQUARE: frame = SPINNER_FRAMES_SQUARE[s->frame]; break;
        case SPINNER_ARROW:  frame = SPINNER_FRAMES_ARROW[s->frame]; break;
        case SPINNER_BOUNCE: frame = SPINNER_FRAMES_BOUNCE[s->frame]; break;
        case SPINNER_CLOCK:  frame = SPINNER_FRAMES_CLOCK[s->frame]; break;
    }
    
    if (!frame) frame = "?";
    
    size_t msg_len = s->message ? strlen(s->message) : 0;
    size_t buf_size = strlen(frame) + msg_len + 10;
    
    char* result = FERN_ALLOC(buf_size);
    if (!result) return FERN_STRDUP("");
    
    if (s->message) {
        snprintf(result, buf_size, "%s %s", frame, s->message);
    } else {
        strcpy(result, frame);
    }
    
    return result;
}

void fern_spinner_FERN_FREE(FernSpinner* s) {
    if (s) {
        FERN_FREE(s->message);
        FERN_FREE(s);
    }
}

/* ========== Prompt Module ========== */

char* fern_prompt_input(const char* prompt) {
    /* Transfer linenoise-owned input into runtime-managed memory. */
    if (isatty(STDIN_FILENO) && isatty(STDOUT_FILENO)) {
        char *edited = linenoise(prompt ? prompt : "");
        char *result = FERN_STRDUP(edited ? edited : "");
        linenoiseFree(edited);
        return result;
    }
    if (prompt) {
        printf("%s", prompt);
        fflush(stdout);
    }
    
    char* line = NULL;
    size_t len = 0;
    ssize_t read = getline(&line, &len, stdin);
    
    if (read <= 0) {
        linenoiseFree(line);
        return FERN_STRDUP("");
    }
    
    /* Remove trailing newline */
    if (read > 0 && line[read - 1] == '\n') {
        line[read - 1] = '\0';
    }
    
    char *result = FERN_STRDUP(line);
    linenoiseFree(line);
    return result;
}

int fern_prompt_confirm(const char* prompt) {
    if (prompt) {
        printf("%s [y/N] ", prompt);
        fflush(stdout);
    }
    
    char* line = NULL;
    size_t len = 0;
    ssize_t read = getline(&line, &len, stdin);
    
    if (read <= 0) {
        FERN_FREE(line);
        return 0;
    }
    
    /* Check for yes */
    int result = (line[0] == 'y' || line[0] == 'Y');
    FERN_FREE(line);
    return result;
}

int fern_prompt_select(const char* prompt, FernStringList* choices) {
    if (!choices || choices->len == 0) return -1;
    
    /* Display prompt */
    if (prompt) {
        printf("%s\n", prompt);
    }
    
    /* Display choices */
    for (int64_t i = 0; i < choices->len; i++) {
        printf("  %lld. %s\n", (long long)(i + 1), choices->data[i]);
    }
    
    printf("Enter choice (1-%lld): ", (long long)choices->len);
    fflush(stdout);
    
    char* line = NULL;
    size_t len = 0;
    ssize_t read = getline(&line, &len, stdin);
    
    if (read <= 0) {
        FERN_FREE(line);
        return -1;
    }
    
    /* Parse selection */
    long selection = strtol(line, NULL, 10);
    FERN_FREE(line);
    
    if (selection < 1 || selection > choices->len) {
        return -1;
    }
    
    return (int)(selection - 1);
}

/**
 * Read a password, masking interactive input and preserving pipe behavior.
 * @param prompt Prompt text, or NULL for no prompt.
 * @return Runtime-managed input; empty on EOF or cancellation.
 */
char* fern_prompt_password(const char* prompt) {
    struct termios original;
    int terminal = isatty(STDIN_FILENO);
    if (terminal) {
        /* Suppress echo even when linenoise falls back for TERM=dumb or pipes. */
        if (tcgetattr(STDIN_FILENO, &original) != 0) return FERN_STRDUP("");
        struct termios hidden = original;
        hidden.c_lflag &= ~(ECHO | ECHONL);
        if (tcsetattr(STDIN_FILENO, TCSANOW, &hidden) != 0) return FERN_STRDUP("");
    }
    linenoiseMaskModeEnable();
    char *result = fern_prompt_input(prompt);
    linenoiseMaskModeDisable();
    if (terminal) tcsetattr(STDIN_FILENO, TCSANOW, &original);
    return result;
}

int64_t fern_prompt_int(const char* prompt, int64_t min, int64_t max) {
    if (prompt) {
        printf("%s (%lld-%lld): ", prompt, (long long)min, (long long)max);
        fflush(stdout);
    }
    
    char* line = NULL;
    size_t len = 0;
    ssize_t read = getline(&line, &len, stdin);
    
    if (read <= 0) {
        FERN_FREE(line);
        return min;
    }
    
    /* Parse number */
    char* endptr;
    long long value = strtoll(line, &endptr, 10);
    FERN_FREE(line);
    
    /* Validate */
    if (value < min) return min;
    if (value > max) return max;
    
    return (int64_t)value;
}
