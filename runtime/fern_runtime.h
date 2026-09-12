/**
 * Fern Runtime Library
 *
 * This library provides core functions for compiled Fern programs.
 * It is linked into every Fern executable.
 *
 * Memory Management:
 * - Uses Boehm GC for automatic garbage collection
 * - All allocated memory is GC-managed (no manual free needed)
 * - Future: BEAM-style per-process heaps for actor isolation
 */

#ifndef FERN_RUNTIME_H
#define FERN_RUNTIME_H

#include <stdint.h>
#include <stddef.h>

/* ========== I/O Functions ========== */

/**
 * Print an integer to stdout (no newline).
 * @param n The integer to print.
 */
void fern_print_int(int64_t n);

/**
 * Print an integer to stdout with newline.
 * @param n The integer to print.
 */
void fern_println_int(int64_t n);

/** Print an IEEE double with 17 significant digits, preserving the existing Fern format. */
void fern_print_float(double n);

/** Print an IEEE double with the same format followed by a newline. */
void fern_println_float(double n);

/** Return a GC-managed, NUL-terminated IEEE double representation (at most 31 bytes). */
char* fern_float_to_str(double n);

/**
 * Print a string to stdout (no newline).
 * @param s The null-terminated string to print.
 */
void fern_print_str(const char* s);

/** Stable stderr failure codes; IO retains the ordinary File error code3. */
enum FernStderrError { FERN_STDERR_INVALID=1, FERN_STDERR_LIMIT=2, FERN_STDERR_IO=3 };

/**
 * Write exact text to descriptor2, with no newline and no global SIGPIPE handler change.
 * @param text UTF8 native CString, <=16MiB; native CStrings cannot carry interior NUL.
 * @return Heap Result: Ok(Unit0), Err1 invalid text, Err2 size limit, Err3 IO/work failure.
 * Partial output may precede Err3. Empty input succeeds even with closed stderr.
 * At most65536 write attempts; blocking kernel writes have no hard deadline.
 * Preserve descriptors, thread mask and preexisting pending SIGPIPE. Hosts must not
 * concurrently change SIGPIPE disposition or consume/inject SIGPIPE in this thread.
 */
int64_t fern_write_stderr(const char* text);

/**
 * Print a string to stdout with newline.
 * @param s The null-terminated string to print.
 */
void fern_println_str(const char* s);

/**
 * Print a boolean to stdout (no newline).
 * @param b The boolean to print (0 = false, non-zero = true).
 */
void fern_print_bool(int64_t b);

/**
 * Print a boolean to stdout with newline.
 * @param b The boolean to print.
 */
void fern_println_bool(int64_t b);

/* ========== String Functions ========== */

/**
 * Convert an integer to a string.
 * @param n The integer to convert.
 * @return Newly allocated string (GC-managed).
 */
char* fern_int_to_str(int64_t n);

/**
 * Convert a boolean to a string.
 * @param b The boolean (0 = false, non-zero = true).
 * @return Newly allocated string (GC-managed).
 */
char* fern_bool_to_str(int64_t b);

/**
 * Get the length of a string.
 * @param s The null-terminated string.
 * @return The length in bytes.
 */
int64_t fern_str_len(const char* s);

/**
 * Concatenate two strings.
 * @param a First string.
 * @param b Second string.
 * @return Newly allocated concatenated string (GC-managed).
 */
char* fern_str_concat(const char* a, const char* b);

/**
 * Compare two strings for equality.
 * @param a First string.
 * @param b Second string.
 * @return 1 if equal, 0 otherwise.
 */
int64_t fern_str_eq(const char* a, const char* b);

/**
 * Check if string starts with prefix.
 * @param s The string.
 * @param prefix The prefix to check.
 * @return 1 if s starts with prefix, 0 otherwise.
 */
int64_t fern_str_starts_with(const char* s, const char* prefix);

/**
 * Check if string ends with suffix.
 * @param s The string.
 * @param suffix The suffix to check.
 * @return 1 if s ends with suffix, 0 otherwise.
 */
int64_t fern_str_ends_with(const char* s, const char* suffix);

/**
 * Check if string contains substring.
 * @param s The string.
 * @param substr The substring to find.
 * @return 1 if s contains substr, 0 otherwise.
 */
int64_t fern_str_contains(const char* s, const char* substr);

/**
 * Find index of substring.
 * @param s The string.
 * @param substr The substring to find.
 * @return Option: Some(index) if found, None otherwise.
 */
int64_t fern_str_index_of(const char* s, const char* substr);

/**
 * Check clamped byte slice endpoints without allocation or termination.
 * @param s Non-null UTF-8 string.
 * @param start Start byte index.
 * @param end Exclusive end byte index.
 * @return 1 for scalar boundaries, otherwise 0.
 */
int64_t fern_str_slice_is_valid(const char* s, int64_t start, int64_t end);

/**
 * Get substring from start to end (exclusive).
 * @param s The string.
 * @param start Start index.
 * @param end End index (exclusive).
 * @return New string containing the substring.
 */
char* fern_str_slice(const char* s, int64_t start, int64_t end);

/**
 * Trim whitespace from both ends.
 * @param s The string.
 * @return New string with whitespace trimmed.
 */
char* fern_str_trim(const char* s);

/**
 * Trim whitespace from start.
 * @param s The string.
 * @return New string with leading whitespace trimmed.
 */
char* fern_str_trim_start(const char* s);

/**
 * Trim whitespace from end.
 * @param s The string.
 * @return New string with trailing whitespace trimmed.
 */
char* fern_str_trim_end(const char* s);

/**
 * Convert string to uppercase.
 * @param s The string.
 * @return New string in uppercase.
 */
char* fern_str_to_upper(const char* s);

/**
 * Convert string to lowercase.
 * @param s The string.
 * @return New string in lowercase.
 */
char* fern_str_to_lower(const char* s);

/**
 * Replace all occurrences of old with new.
 * @param s The string.
 * @param old_str Substring to replace.
 * @param new_str Replacement string.
 * @return New string with replacements.
 */
char* fern_str_replace(const char* s, const char* old_str, const char* new_str);

/**
 * Validate scalar input for empty-delimiter splitting without terminating.
 * @param s Non-null source bytes.
 * @param delim Non-null delimiter.
 * @return 0 for malformed UTF-8 with empty delimiter, otherwise 1.
 */
int64_t fern_str_split_is_valid(const char* s, const char* delim);

/** Unicode16 Nd predicate, empty/malformed false; >16MiB is a runtime size fault. */
int64_t fern_str_is_decimal(const char* text);
/** Internal allocation-free decimal size preflight, for compiler-controlled cleanup. */
int64_t fern_str_decimal_size_is_valid(const char* text);

/**
 * Split string by delimiter.
 * @param s The string.
 * @param delim The delimiter.
 * @return List of strings (as FernStringList).
 */
struct FernStringList* fern_str_split(const char* s, const char* delim);

/**
 * Split string into lines.
 * Handles \n, \r\n, and \r line endings.
 * @param s The string.
 * @return List of lines (as FernStringList).
 */
struct FernStringList* fern_str_lines(const char* s);

/**
 * Join list of strings with separator.
 * @param list The string list.
 * @param sep The separator.
 * @return New joined string.
 */
char* fern_str_join(struct FernStringList* list, const char* sep);

/**
 * Repeat string n times.
 * @param s The string.
 * @param n Number of repetitions.
 * @return New string; empty for nonpositive n or empty s. Content is capped at
 * 16 MiB; larger requests terminate with a diagnostic before allocating.
 */
char* fern_str_repeat(const char* s, int64_t n);

/**
 * Get character at index.
 * @param s The string.
 * @param index The index.
 * @return Option: Some(char as int) if valid index, None otherwise.
 */
int64_t fern_str_char_at(const char* s, int64_t index);

/**
 * Check if string is empty.
 * @param s The string.
 * @return 1 if empty, 0 otherwise.
 */
int64_t fern_str_is_empty(const char* s);

/**
 * String list type for split/join operations.
 */
typedef struct FernStringList {
    char** data;
    int64_t len;
    int64_t cap;
} FernStringList;

/**
 * Free a string list.
 * @param list The list to free.
 */
void fern_str_list_free(FernStringList* list);

/* ========== List Functions ========== */

/**
 * Fern list structure.
 * Lists are immutable; operations return new lists.
 */
typedef struct FernList {
    int64_t* data;      /* Array of elements (int64_t for now) */
    int64_t len;        /* Number of elements */
    int64_t cap;        /* Allocated capacity */
} FernList;

/**
 * Create a new empty list.
 * @return Pointer to new list.
 */
FernList* fern_list_new(void);

/**
 * Create a list with given capacity.
 * @param cap Initial capacity.
 * @return Pointer to new list.
 */
FernList* fern_list_with_capacity(int64_t cap);

/**
 * Get the length of a list.
 * @param list The list.
 * @return Number of elements.
 */
int64_t fern_list_len(FernList* list);

/**
 * Get element at index.
 * @param list The list.
 * @param index The index (0-based).
 * @return The element value; invalid indices exit 1 with a diagnostic before access.
 */
int64_t fern_list_get(FernList* list, int64_t index);

/**
 * Append an element to a list in place (mutates the list).
 * Used for list literal construction.
 * @param list The list to modify.
 * @param value The value to append.
 */
void fern_list_push_mut(FernList* list, int64_t value);

/**
 * Append an element to a list (returns new list).
 * @param list The original list.
 * @param value The value to append.
 * @return New list with element appended.
 */
FernList* fern_list_push(FernList* list, int64_t value);

/**
 * Map a function over a list.
 * @param list The list.
 * @param fn Function pointer (int64_t -> int64_t).
 * @return New list with function applied to each element.
 */
FernList* fern_list_map(FernList* list, int64_t (*fn)(int64_t));

/**
 * Fold a list from left.
 * @param list The list.
 * @param init Initial accumulator value.
 * @param fn Function (acc, elem) -> acc.
 * @return Final accumulated value.
 */
int64_t fern_list_fold(FernList* list, int64_t init, int64_t (*fn)(int64_t, int64_t));

/**
 * Free a list.
 * @param list The list to free.
 */
void fern_list_free(FernList* list);

/**
 * Filter a list with a predicate.
 * @param list The list.
 * @param pred Predicate function (int64_t -> bool as int64_t).
 * @return New list with elements where pred returns non-zero.
 */
FernList* fern_list_filter(FernList* list, int64_t (*pred)(int64_t));

/**
 * Find first element matching predicate.
 * @param list The list.
 * @param pred Predicate function.
 * @return Option: Some(element) if found, None otherwise.
 */
int64_t fern_list_find(FernList* list, int64_t (*pred)(int64_t));

/**
 * Reverse a list.
 * @param list The list.
 * @return New list with elements in reverse order.
 */
FernList* fern_list_reverse(FernList* list);

/**
 * Concatenate two lists.
 * @param a First list.
 * @param b Second list.
 * @return New list with all elements from a followed by b.
 */
FernList* fern_list_concat(FernList* a, FernList* b);

/**
 * Get first element of a list.
 * @param list The list.
 * @return The first element; empty input exits 1 with a diagnostic before access.
 */
int64_t fern_list_head(FernList* list);

/**
 * Get list without first element.
 * @param list The list.
 * @return New list without first element (empty if list was empty/single).
 */
FernList* fern_list_tail(FernList* list);

/**
 * Check if list is empty.
 * @param list The list.
 * @return 1 if empty, 0 otherwise.
 */
int64_t fern_list_is_empty(FernList* list);

/**
 * Check if list contains element (by value equality for Int).
 * @param list The list.
 * @param elem The element to find.
 * @return 1 if found, 0 otherwise.
 */
int64_t fern_list_contains(FernList* list, int64_t elem);

/**
 * Check if string list contains string (by string equality).
 * @param list The list of strings.
 * @param s The string to find.
 * @return 1 if found, 0 otherwise.
 */
int64_t fern_list_contains_str(FernList* list, const char* s);

/**
 * Check if any element matches predicate.
 * @param list The list.
 * @param pred Predicate function.
 * @return 1 if any element matches, 0 otherwise.
 */
int64_t fern_list_any(FernList* list, int64_t (*pred)(int64_t));

/**
 * Check if all elements match predicate.
 * @param list The list.
 * @param pred Predicate function.
 * @return 1 if all elements match, 0 otherwise.
 */
int64_t fern_list_all(FernList* list, int64_t (*pred)(int64_t));

/* ========== Result Type ========== */

/**
 * Fern Result type.
 * Tag: 0 = Ok, 1 = Err
 * Value: The ok or err value (int64_t for simplicity).
 *
 * Represented as a packed 64-bit value:
 * - Upper 32 bits: value
 * - Lower 32 bits: tag (0 = Ok, 1 = Err)
 */

/**
 * Create an Ok result.
 * @param value The success value.
 * @return Packed Result value.
 */
int64_t fern_result_ok(int64_t value);

/**
 * Create an Err result.
 * @param value The error value.
 * @return Packed Result value.
 */
int64_t fern_result_err(int64_t value);

/**
 * Check if a Result is Ok.
 * @param result The packed Result value.
 * @return 1 if Ok, 0 if Err.
 */
int64_t fern_result_is_ok(int64_t result);

/**
 * Unwrap the value from a Result.
 * @param result The packed Result value.
 * @return The contained value (ok or err).
 */
int64_t fern_result_unwrap(int64_t result);

/**
 * Map a function over an Ok value.
 * @param result The Result.
 * @param fn Function to apply if Ok.
 * @return New Result with mapped value, or original Err.
 */
int64_t fern_result_map(int64_t result, int64_t (*fn)(int64_t));

/**
 * Chain a function that returns Result over an Ok value.
 * @param result The Result.
 * @param fn Function to apply if Ok (returns Result).
 * @return Result from fn if Ok, or original Err.
 */
int64_t fern_result_and_then(int64_t result, int64_t (*fn)(int64_t));

/**
 * Get the Ok value or a default.
 * @param result The Result.
 * @param default_val Value to return if Err.
 * @return Ok value or default.
 */
int64_t fern_result_unwrap_or(int64_t result, int64_t default_val);

/**
 * Get the Ok value or compute a default from the error.
 * @param result The Result.
 * @param fn Function to compute default from error.
 * @return Ok value or fn(err_value).
 */
int64_t fern_result_unwrap_or_else(int64_t result, int64_t (*fn)(int64_t));

/* ========== Option Type ========== */

/**
 * Fern Option type.
 * Tag: 0 = None, 1 = Some
 * Represented as packed 64-bit value (same as Result).
 */

/**
 * Create a Some option.
 * @param value The contained value.
 * @return Packed Option value.
 */
int64_t fern_option_some(int64_t value);

/**
 * Create a None option.
 * @return Packed Option value representing None.
 */
int64_t fern_option_none(void);

/**
 * Check if an Option is Some.
 * @param option The packed Option value.
 * @return 1 if Some, 0 if None.
 */
int64_t fern_option_is_some(int64_t option);

/**
 * Unwrap the value from an Option.
 * @param option The packed Option value.
 * @return The contained value (undefined if None).
 */
int64_t fern_option_unwrap(int64_t option);

/**
 * Map a function over a Some value.
 * @param option The Option.
 * @param fn Function to apply if Some.
 * @return New Option with mapped value, or None.
 */
int64_t fern_option_map(int64_t option, int64_t (*fn)(int64_t));

/**
 * Get the Some value or a default.
 * @param option The Option.
 * @param default_val Value to return if None.
 * @return Some value or default.
 */
int64_t fern_option_unwrap_or(int64_t option, int64_t default_val);

/* ========== System Functions (argc/argv access) ========== */

/**
 * Set command-line arguments (called from C main wrapper).
 * @param argc Argument count.
 * @param argv Argument vector.
 */
void fern_set_args(int argc, char** argv);

/**
 * Get command-line argument count.
 * @return Number of arguments (including program name).
 */
int64_t fern_args_count(void);

/**
 * Get command-line argument at index.
 * @param index The index (0 = program name).
 * @return The argument string, or empty string if out of bounds.
 */
char* fern_arg(int64_t index);

/**
 * Get all command-line arguments as a string list.
 * @return FernStringList containing all arguments.
 */
FernStringList* fern_args(void);

/**
 * Exit program immediately with given code.
 * @param code Exit code.
 */
void fern_exit(int64_t code);

/**
 * Result of executing a command.
 */
typedef struct FernExecResult {
    int64_t exit_code;   /* Process exit code */
    char* stdout_str;    /* Captured stdout */
    char* stderr_str;    /* Captured stderr */
} FernExecResult;

/**
 * Execute a shell command and capture output.
 * @param cmd The command to execute.
 * @return FernExecResult with exit code, stdout, and stderr.
 */
FernExecResult* fern_exec(const char* cmd);

/**
 * Execute a command with arguments (no shell).
 * @param args FernStringList of command and arguments.
 * @return FernExecResult with exit code, stdout, and stderr.
 */
FernExecResult* fern_exec_args(FernStringList* args);

/** Stable bounded-process failure codes; normal child statuses, including127, stay in Ok. */
enum FernExecError {
    FERN_EXEC_INVALID = 1, FERN_EXEC_SPAWN = 2, FERN_EXEC_TIMEOUT = 3,
    FERN_EXEC_OUTPUT_LIMIT = 4, FERN_EXEC_IO = 5, FERN_EXEC_TEXT = 6,
    FERN_EXEC_SIGNAL = 7
};

/**
 * Execute literal UTF8 argv with a private process group and bounded text captures.
 * @param args 1..4096 strings, nonempty argv0, <=1MiB including terminators.
 * @param timeout_ms Monotonic capture deadline in1..600000ms; kernel reap may extend it.
 * @param max_output_bytes Independent stdout/stderr caps in0..16MiB.
 * @return Existing heap Result: Ok(pointer to24-byte FernExecResult), or Err(FernExecError).
 * Requires initialized Fern GC and no competing child reaper or concurrent SIGCHLD policy changes.
 * SIGCHLD ignored/SA_NOCLDWAIT is rejected. Private-group cleanup is not descendant containment.
 * Basename PATH lookup allows <=1MiB/4096 components; unset PATH uses /usr/bin:/bin.
 * Slash paths bypass lookup. No implicit ENOEXEC shell fallback occurs.
 */
int64_t fern_exec_args_bounded(FernStringList* args, int64_t timeout_ms, int64_t max_output_bytes);


/**
 * Get environment variable.
 * @param name The variable name.
 * @return The value, or empty string if not set.
 */
char* fern_getenv(const char* name);

/**
 * Set environment variable.
 * @param name The variable name.
 * @param value The value to set.
 * @return 0 on success, non-zero on failure.
 */
int64_t fern_setenv(const char* name, const char* value);

/**
 * Get current working directory.
 * @return The current working directory path.
 */
char* fern_cwd(void);

/**
 * Change current working directory.
 * @param path The path to change to.
 * @return 0 on success, -1 on failure.
 */
int64_t fern_chdir(const char* path);

/**
 * Get system hostname.
 * @return The hostname string.
 */
char* fern_hostname(void);

/**
 * Get current username.
 * @return The username string.
 */
char* fern_user(void);

/**
 * Get home directory.
 * @return The home directory path.
 */
char* fern_home(void);

/* ========== Memory Functions ========== */

/**
 * Perceus-style object header for heap values.
 *
 * This header lives immediately before payload data for RC-managed objects.
 * Under the current Boehm backend it is used for metadata and refcount
 * bookkeeping only; memory reclamation is still GC-driven.
 */
typedef struct {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} FernObjectHeader;

/* Core heap value type tags for RC metadata. */
#define FERN_RC_TYPE_UNKNOWN      ((uint16_t)0)
#define FERN_RC_TYPE_STRING       ((uint16_t)1)
#define FERN_RC_TYPE_LIST         ((uint16_t)2)
#define FERN_RC_TYPE_STRING_LIST  ((uint16_t)3)
#define FERN_RC_TYPE_RESULT       ((uint16_t)4)
#define FERN_RC_TYPE_TUPLE        ((uint16_t)5)

/* RC header flags. */
#define FERN_RC_FLAG_NONE      ((uint16_t)0)
#define FERN_RC_FLAG_UNIQUE    ((uint16_t)(1u << 0))
#define FERN_RC_FLAG_BORROWED  ((uint16_t)(1u << 1))

/**
 * Allocate memory.
 * @param size Number of bytes.
 * @return Pointer to allocated memory.
 */
void* fern_alloc(size_t size);

/**
 * Allocate an RC-managed payload with an object header.
 * @param payload_size Payload bytes (excluding header).
 * @param type_tag Core heap type tag.
 * @return Pointer to payload (header is stored immediately before payload).
 */
void* fern_rc_alloc(size_t payload_size, uint16_t type_tag);

/**
 * Duplicate an RC-managed reference.
 * @param ptr Payload pointer returned by fern_rc_alloc().
 * @return Same payload pointer (or NULL).
 */
void* fern_rc_dup(void* ptr);

/**
 * Drop an RC-managed reference.
 * @param ptr Payload pointer returned by fern_rc_alloc().
 */
void fern_rc_drop(void* ptr);

/**
 * Get current RC refcount for an object payload pointer.
 * @param ptr Payload pointer returned by fern_rc_alloc().
 * @return Current refcount, or 0 for NULL.
 */
uint32_t fern_rc_refcount(const void* ptr);

/**
 * Get RC type tag for an object payload pointer.
 * @param ptr Payload pointer returned by fern_rc_alloc().
 * @return Object type tag, or FERN_RC_TYPE_UNKNOWN for NULL.
 */
uint16_t fern_rc_type_tag(const void* ptr);

/**
 * Get RC flags for an object payload pointer.
 * @param ptr Payload pointer returned by fern_rc_alloc().
 * @return Object flags, or FERN_RC_FLAG_NONE for NULL.
 */
uint16_t fern_rc_flags(const void* ptr);

/**
 * Set RC flags for an object payload pointer.
 * @param ptr Payload pointer returned by fern_rc_alloc().
 * @param flags Flags bitmask.
 */
void fern_rc_set_flags(void* ptr, uint16_t flags);

/**
 * Duplicate an owned reference.
 *
 * Memory backend contract:
 * - Boehm backend (current): returns the same pointer
 * - Perceus backend (future): increments reference count
 *
 * @param ptr Pointer to duplicate ownership for.
 * @return The duplicated pointer (or NULL if ptr is NULL).
 */
void* fern_dup(void* ptr);

/**
 * Drop an owned reference.
 *
 * Memory backend contract:
 * - Boehm backend (current): no-op
 * - Perceus backend (future): decrements reference count and may free
 *
 * @param ptr Pointer to drop ownership for.
 */
void fern_drop(void* ptr);

/**
 * Free memory (compatibility alias for fern_drop()).
 * @param ptr Pointer to release.
 */
void fern_free(void* ptr);

/* ========== File I/O Functions ========== */

/**
 * Fern file handle.
 */
typedef struct FernFile {
    void* handle;   /* FILE* internally */
    int64_t error;  /* Last error code, 0 if none */
} FernFile;

/**
 * Read complete seekable file text (strict UTF8, no interior NUL, <=16MiB).
 * @param path The file path.
 * @return Heap Result: Ok(String), or Err(open1/IO-or-invalid-text3/allocation4).
 * Complete read, ferror and fclose are checked before successful publication.
 */
int64_t fern_read_file(const char* path);

/**
 * Write UTF8 text <=16MiB to file (overwrites if exists), preflighted before opening.
 * @param path The file path.
 * @param contents The string to write.
 * @return Heap Result: Ok(bytes written) only after fwrite/ferror/fclose succeed.
 * Errors may leave a modified target; no atomicity, fsync or durability guarantee.
 * Invalid text/size and IO return Err3; open failure remains Err2.
 */
int64_t fern_write_file(const char* path, const char* contents);

/**
 * Append UTF8 text <=16MiB to file (creates if not exists), preflighted before opening.
 * @param path The file path.
 * @param contents The string to append.
 * @return Heap Result: Ok(bytes written) only after fwrite/ferror/fclose succeed.
 * Errors may leave a modified target; no atomicity, fsync or durability guarantee.
 * Invalid text/size and IO return Err3; open failure remains Err2.
 */
int64_t fern_append_file(const char* path, const char* contents);

/**
 * Check if file exists.
 * @param path The file path.
 * @return 1 if exists, 0 otherwise.
 */
int64_t fern_file_exists(const char* path);

/**
 * Delete a file.
 * @param path The file path.
 * @return Result: Ok(0) if deleted, Err(error code) otherwise.
 */
int64_t fern_delete_file(const char* path);

/**
 * Get file size in bytes.
 * @param path The file path.
 * @return Result: Ok(size) or Err(error code).
 */
int64_t fern_file_size(const char* path);

/**
 * Check if path is a directory.
 * @param path The path to check.
 * @return 1 if directory, 0 otherwise.
 */
int64_t fern_is_dir(const char* path);

/**
 * List directory contents.
 * @param path The directory path.
 * @return FernStringList containing entry names, or NULL on error.
 */
FernStringList* fern_list_dir(const char* path);

/**
 * List directory names with explicit filesystem errors; path must not be NULL.
 * @param path Directory to enumerate; dot entries are excluded.
 * @return Result: Ok(FernStringList pointer) or Err(FERN_ERR_*); no partial success.
 */
int64_t fern_read_dir_result(const char* path);

/* Error codes for file operations */
#define FERN_ERR_FILE_NOT_FOUND  1
#define FERN_ERR_PERMISSION      2
#define FERN_ERR_IO              3
#define FERN_ERR_OUT_OF_MEMORY   4
#define FERN_ERR_NOT_A_DIR       5

/* ========== Gate C Stdlib Runtime Surface ========== */

/* Immutable JSON native core: legacy C source bindings below remain unchanged. */
typedef struct FernJsonValue FernJsonValue;
typedef struct FernJsonError FernJsonError;

/** Audited native member ABI; source tuples require an explicit tagged-layout adapter. */
typedef struct { const FernJsonValue* key; const FernJsonValue* value; } FernJsonMember;

/**
 * Construct JSON null.
 *
 * @return Runtime-owned immutable value.
 */
FernJsonValue* fern_json_value_null(void);

/**
 * Construct JSON Boolean.
 * @param boolean zero or one.
 * @return Runtime-owned immutable value.
 */
FernJsonValue* fern_json_value_from_bool(int64_t boolean);

/**
 * Construct exact signed64 JSON number.
 * @param number signed integer.
 * @return Runtime-owned immutable value.
 */
FernJsonValue* fern_json_value_from_int(int64_t number);

/**
 * Construct finite JSON number, rejecting nonfinite Float.
 * @param number binary64 Float.
 * @return Heap Result with full-width success or opaque Error pointer; failures have offset-1.
 */
int64_t fern_json_value_from_float(double number);

/**
 * Validate and copy a decoded JSON String.
 * @param text non-NULL text, at most1 MiB.
 * @return Heap Result with full-width success or opaque Error pointer; failures have offset-1.
 */
int64_t fern_json_value_from_string(const char* text);

/**
 * Preserve exactly one JSON number token without whitespace.
 * @param text non-NULL number text, at most1 MiB.
 * @return Heap Result with full-width success or opaque Error pointer; failures have offset-1.
 */
int64_t fern_json_value_from_number_text(const char* text);

/**
 * Copy immutable JSON array references.
 * @param list valid Value-pointer list.
 * @return Heap Result with full-width success or opaque Error pointer; failures have offset-1.
 */
int64_t fern_json_value_from_array(const FernList* list);

/**
 * Build an ordered object from parallel lists.
 * @param keys valid String-pointer list.
 * @param values valid Value-pointer list with matching length.
 * @return Heap Result with full-width success or opaque Error pointer; failures have offset-1.
 */
int64_t fern_json_value_from_object(const FernList* keys, const FernList* values);

/**
 * Copy array references into fresh FernList storage.
 * @param value valid opaque value.
 * @return Heap Result with full-width success or opaque Error pointer; failures have offset-1.
 */
int64_t fern_json_value_elements(const FernJsonValue* value);

/**
 * Return ordered FernList of FernJsonMember pointers; keys retain NUL.
 * @param value valid opaque value.
 * @return Heap Result with full-width success or opaque Error pointer; failures have offset-1.
 */
int64_t fern_json_value_members(const FernJsonValue* value);

/**
 * Fail a compiler-side adapter preflight without oversized allocation.
 *
 * @return Heap Result with full-width success or opaque Error pointer; failures have offset-1.
 */
int64_t fern_json_value_limit_error(void);

/**
 * Parse a bounded JSON document; Result(Value pointer, Error pointer).
 * @param text non-NULL NUL-terminated input (maximum1 MiB).
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_parse(const char* text);

/**
 * Compactly encode a valid immutable value; Result(String, Error pointer).
 * @param value valid opaque value returned by this API.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_stringify(const FernJsonValue* value);

/**
 * Test the JSON null kind; returns zero or one.
 * @param value valid opaque value.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_is_null(const FernJsonValue* value);

/**
 * Look up an object member; Result(Value pointer, Error pointer).
 * @param value valid opaque value.
 * @param key non-NULL NUL-terminated lookup name.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_get(const FernJsonValue* value, const char* key);

/**
 * Read an array element with bounds checking; Result(Value pointer, Error pointer).
 * @param value valid opaque value.
 * @param index signed zero-based index.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_at(const FernJsonValue* value, int64_t index);

/**
 * Count array elements or object members; Result(Int, Error pointer).
 * @param value valid opaque value.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_length(const FernJsonValue* value);

/**
 * Extract Bool; failure carries Error pointer.
 * @param value valid opaque value.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_as_bool(const FernJsonValue* value);

/**
 * Convert exact integral decimal to signed64; failure carries Error pointer.
 * @param value valid opaque value.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_as_int(const FernJsonValue* value);

/**
 * Convert to binary64; successful Result payload contains Float bits; failure carries Error pointer.
 * @param value valid opaque value.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_as_float(const FernJsonValue* value);

/**
 * Extract NUL-free decoded text; failure carries Error pointer.
 * @param value valid opaque value.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_as_string(const FernJsonValue* value);

/**
 * Extract original number lexeme; failure carries Error pointer.
 * @param value valid opaque value.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_number_text(const FernJsonValue* value);

/**
 * Read stable code1..11.
 * @param error valid opaque error.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_error_code(const FernJsonError* error);

/**
 * Read zero-based input byte offset or -1.
 * @param error valid opaque error.
 * @return See description; all referenced allocations are runtime-owned.
 */
int64_t fern_json_value_error_offset(const FernJsonError* error);

/**
 * Read a stable static message.
 * @param error valid opaque error.
 * @return See description; all referenced allocations are runtime-owned.
 */
const char* fern_json_value_error_message(const FernJsonError* error);

/**
 * Parse JSON text.
 * Placeholder Gate C contract:
 * - Empty input returns Err(FERN_ERR_IO)
 * - Otherwise returns Ok(copy of input)
 * @param text UTF-8 JSON text.
 * @return Result: Ok(JSON text copy) or Err(error code).
 */
int64_t fern_json_parse(const char* text);

/**
 * Encode value as JSON text.
 * Placeholder Gate C contract:
 * - Returns Ok(copy of input), including empty strings
 * - Returns Err(FERN_ERR_IO) for invalid pointers
 * @param text Input text payload.
 * @return Result: Ok(JSON string copy) or Err(error code).
 */
int64_t fern_json_stringify(const char* text);

/**
 * Perform an HTTP GET request.
 * Current implementation returns Err(FERN_ERR_IO) until HTTP runtime lands.
 * @param url Request URL.
 * @return Result: Ok(response body) or Err(error code).
 */
int64_t fern_http_get(const char* url);

/**
 * Perform an HTTP POST request.
 * Current implementation returns Err(FERN_ERR_IO) until HTTP runtime lands.
 * @param url Request URL.
 * @param body Request body payload.
 * @return Result: Ok(response body) or Err(error code).
 */
int64_t fern_http_post(const char* url, const char* body);

/**
 * Open a SQL connection handle.
 * Uses SQLite backend and returns an opaque Fern handle id.
 * @param path Database path/URL.
 * @return Result: Ok(handle) or Err(error code).
 */
int64_t fern_sql_open(const char* path);

/**
 * Execute a SQL statement.
 * Executes SQL against a previously opened handle.
 * @param handle SQL handle.
 * @param query SQL statement.
 * @return Result: Ok(rows affected from sqlite3_changes) or Err(error code).
 */
int64_t fern_sql_execute(int64_t handle, const char* query);

/**
 * Allocate an actor mailbox record and return its id.
 * This API does not execute a function or start an autonomous process.
 * @param name Actor name.
 * @return Actor id (positive integer), or 0 on allocation failure.
 */
int64_t fern_actor_spawn(const char* name);

/**
 * Spawn an actor linked to the current actor context.
 *
 * Requires a current actor to be set via `fern_actor_set_current`.
 * If no current actor exists, spawn_link returns 0.
 *
 * @param name Actor name.
 * @return Actor id (positive integer), or 0 on allocation failure.
 */
int64_t fern_actor_spawn_link(const char* name);

/**
 * Set current actor context used by spawn_link.
 * @param actor_id Actor id to set as current, or 0 to clear context.
 * @return 0 on success, -1 for invalid/dead actor id.
 */
int64_t fern_actor_set_current(int64_t actor_id);

/**
 * Read current actor context id used by spawn_link.
 * @return Current actor id, or 0 if no actor is active in context.
 */
int64_t fern_actor_self(void);

/**
 * Set actor runtime clock (seconds) for deterministic supervision timing.
 * @param now_sec Absolute seconds value (non-negative).
 * @return 0 on success, -1 on invalid value.
 */
int64_t fern_actor_clock_set(int64_t now_sec);

/**
 * Advance actor runtime clock by delta seconds.
 * @param delta_sec Seconds to advance (non-negative).
 * @return 0 on success, -1 on invalid/overflow.
 */
int64_t fern_actor_clock_advance(int64_t delta_sec);

/**
 * Read actor runtime clock seconds used by supervision timing.
 * @return Current runtime clock seconds.
 */
int64_t fern_actor_clock_now(void);

/**
 * Register a monitor relation from supervisor to worker.
 * Monitors receive `DOWN(pid, reason)` notifications when worker exits.
 * @param supervisor_id Supervisor actor id.
 * @param worker_id Worker actor id.
 * @return Result: Ok(0) when monitor registration succeeds, Err(error code) otherwise.
 * Both actors must exist and be alive.
 */
int64_t fern_actor_monitor(int64_t supervisor_id, int64_t worker_id);

/**
 * Remove a monitor relation from supervisor to worker.
 * @param supervisor_id Supervisor actor id.
 * @param worker_id Worker actor id.
 * @return Result: Ok(0) when monitor is removed/already absent, Err(error code) otherwise.
 */
int64_t fern_actor_demonitor(int64_t supervisor_id, int64_t worker_id);

/**
 * Register a supervised child spec with restart-intensity policy.
 * Each worker has one supervisor; self-supervision and cycles are rejected.
 * @param supervisor_id Supervisor actor id.
 * @param worker_id Worker actor id.
 * @param max_restarts Maximum restart attempts allowed in period_sec window.
 * @param period_sec Restart budget window size in seconds.
 * @return Result: Ok(0) when supervision is configured, Err(error code) otherwise.
 * Both actors must exist and be alive.
 */
int64_t fern_actor_supervise(int64_t supervisor_id, int64_t worker_id, int64_t max_restarts, int64_t period_sec);

/**
 * Register supervised child spec using one_for_all restart strategy.
 * @param supervisor_id Supervisor actor id.
 * @param worker_id Worker actor id.
 * @param max_restarts Maximum restart attempts allowed in period_sec window.
 * @param period_sec Restart budget window size in seconds.
 * @return Result: Ok(0) when supervision is configured, Err(error code) otherwise.
 */
int64_t fern_actor_supervise_one_for_all(int64_t supervisor_id, int64_t worker_id, int64_t max_restarts, int64_t period_sec);

/**
 * Register supervised child spec using rest_for_one restart strategy.
 * @param supervisor_id Supervisor actor id.
 * @param worker_id Worker actor id.
 * @param max_restarts Maximum restart attempts allowed in period_sec window.
 * @param period_sec Restart budget window size in seconds.
 * @return Result: Ok(0) when supervision is configured, Err(error code) otherwise.
 */
int64_t fern_actor_supervise_rest_for_one(int64_t supervisor_id, int64_t worker_id, int64_t max_restarts, int64_t period_sec);

/**
 * Send a message to an actor mailbox.
 * @param actor_id Destination actor id.
 * @param msg Message payload.
 * @return Result: Ok(0) on enqueue, Err(error code) for invalid actor/input.
 */
int64_t fern_actor_send(int64_t actor_id, const char* msg);

/**
 * Receive the next actor message from a mailbox.
 * @param actor_id Actor id.
 * @return Result: Ok(message) or Err(error code) if mailbox is empty/invalid.
 */
int64_t fern_actor_receive(int64_t actor_id);

/**
 * Mark an actor dead, notify links/monitors, and apply its supervision policy.
 * Normal/shutdown exits do not restart; abnormal exits may replace the actor.
 * @param actor_id Exiting actor id.
 * @param reason Exit reason string.
 * @return Result: Ok(replacement id) after automatic restart, Ok(0) after an
 * ordinary exit, or Err(error code) on failure/restart budget exhaustion.
 * Owned descendants stop with shutdown before any fallible notification; external
 * live observers are notified in preorder. Notification failure never revives them.
 * Restarting the root does not recreate its former descendant subtree.
 */
int64_t fern_actor_exit(int64_t actor_id, const char* reason);

/**
 * Restart an actor and return the new actor id.
 * Restart preserves actor name and linked parent baseline.
 * Actor must already be dead and must not have received a replacement.
 * A dead PID can be replaced only once, even after its replacement exits.
 * A supervised actor requires its original owner to remain alive. Restarting a
 * dead owner does not silently reparent its former children.
 * Name/monitor allocation failures publish no new live actor or replacement ID.
 * @param actor_id Actor id to restart.
 * @return Result: Ok(new actor id) or Err(error code).
 */
int64_t fern_actor_restart(int64_t actor_id);

/**
 * Get current mailbox length for an actor.
 * @param actor_id Actor id.
 * @return Number of queued messages, or -1 for invalid actor id.
 */
int64_t fern_actor_mailbox_len(int64_t actor_id);

/**
 * Return next runnable actor id from scheduler queue.
 *
 * The scheduler returns actors in round-robin order across ready actors.
 * Each successful send contributes one scheduler ticket for that actor.
 *
 * @return Actor id, or 0 when no actor is ready.
 */
int64_t fern_actor_scheduler_next(void);

/**
 * Start an actor and return its id.
 * Compatibility alias for fern_actor_spawn().
 * @param name Actor name.
 * @return Actor id (positive integer), or 0 on allocation failure.
 */
int64_t fern_actor_start(const char* name);

/**
 * Post a message to an actor.
 * Compatibility alias for fern_actor_send().
 * @param actor_id Destination actor id.
 * @param msg Message payload.
 * @return Result: Ok(0) on enqueue, Err(error code) for invalid actor/input.
 */
int64_t fern_actor_post(int64_t actor_id, const char* msg);

/**
 * Receive the next actor message.
 * Compatibility alias for fern_actor_receive().
 * @param actor_id Actor id.
 * @return Result: Ok(message) or Err(error code) if mailbox is empty/invalid.
 */
int64_t fern_actor_next(int64_t actor_id);

/* ========== Regex Functions ========== */

/**
 * Regex match result.
 */
typedef struct FernRegexMatch {
    int64_t start;       /* Start index of match (-1 if no match) */
    int64_t end;         /* End index of match (exclusive) */
    char* matched;       /* The matched substring (NULL if no match) */
} FernRegexMatch;

/**
 * Regex match with capture groups.
 */
typedef struct FernRegexCaptures {
    int64_t count;              /* Number of captures (0 = full match) */
    FernRegexMatch* captures;   /* Array of captures */
} FernRegexCaptures;

/**
 * Check if string matches regex pattern.
 * @param s The string to match.
 * @param pattern The regex pattern.
 * @return 1 if matches, 0 otherwise.
 */
int64_t fern_regex_is_match(const char* s, const char* pattern);

/**
 * Find first match of regex in string.
 * @param s The string to search.
 * @param pattern The regex pattern.
 * @return FernRegexMatch with match info (start=-1 if no match).
 */
FernRegexMatch* fern_regex_find(const char* s, const char* pattern);

/**
 * Find all matches of regex in string.
 * @param s The string to search.
 * @param pattern The regex pattern.
 * @return FernStringList of all matched substrings.
 */
FernStringList* fern_regex_find_all(const char* s, const char* pattern);

/**
 * Replace first match of regex with replacement.
 * @param s The string to modify.
 * @param pattern The regex pattern.
 * @param replacement The replacement string.
 * @return New string with replacement applied.
 */
char* fern_regex_replace(const char* s, const char* pattern, const char* replacement);

/**
 * Replace all matches of regex with replacement.
 * @param s The string to modify.
 * @param pattern The regex pattern.
 * @param replacement The replacement string.
 * @return New string with all replacements applied.
 */
char* fern_regex_replace_all(const char* s, const char* pattern, const char* replacement);

/**
 * Split string by regex pattern.
 * @param s The string to split.
 * @param pattern The regex pattern to split on.
 * @return FernStringList of parts.
 */
FernStringList* fern_regex_split(const char* s, const char* pattern);

/**
 * Find match with capture groups.
 * @param s The string to search.
 * @param pattern The regex pattern with groups.
 * @return Full match and up to nine capture groups in pattern order. An absent
 *         group has start/end -1 and empty text; an empty participating group has
 *         equal nonnegative offsets. Invalid patterns/no match return zero groups.
 */
FernRegexCaptures* fern_regex_captures(const char* s, const char* pattern);

/**
 * Free a regex match result.
 * @param match The match to free.
 */
void fern_regex_match_free(FernRegexMatch* match);

/**
 * Free regex captures.
 * @param captures The captures to free.
 */
void fern_regex_captures_free(FernRegexCaptures* captures);

/* ========== TUI: Term Module ========== */

/**
 * Terminal size result.
 */
typedef struct FernTermSize {
    int64_t cols;
    int64_t rows;
} FernTermSize;

/**
 * Get terminal dimensions.
 * @return FernTermSize with columns and rows.
 */
FernTermSize* fern_term_size(void);

/**
 * Check if stdout is a TTY.
 * @return 1 if TTY, 0 otherwise.
 */
int64_t fern_term_is_tty(void);

/**
 * Get color support level.
 * @return 0 (no color), 16 (basic), 256 (extended), 16777216 (truecolor).
 */
int64_t fern_term_color_support(void);

/* ========== TUI: Style Module ========== */

/**
 * Apply ANSI style to text.
 * All style functions return a new string with ANSI codes applied.
 */

/* Basic colors (foreground) */
char* fern_style_black(const char* text);
char* fern_style_red(const char* text);
char* fern_style_green(const char* text);
char* fern_style_yellow(const char* text);
char* fern_style_blue(const char* text);
char* fern_style_magenta(const char* text);
char* fern_style_cyan(const char* text);
char* fern_style_white(const char* text);

/* Bright colors (foreground) */
char* fern_style_bright_black(const char* text);
char* fern_style_bright_red(const char* text);
char* fern_style_bright_green(const char* text);
char* fern_style_bright_yellow(const char* text);
char* fern_style_bright_blue(const char* text);
char* fern_style_bright_magenta(const char* text);
char* fern_style_bright_cyan(const char* text);
char* fern_style_bright_white(const char* text);

/* Background colors */
char* fern_style_on_black(const char* text);
char* fern_style_on_red(const char* text);
char* fern_style_on_green(const char* text);
char* fern_style_on_yellow(const char* text);
char* fern_style_on_blue(const char* text);
char* fern_style_on_magenta(const char* text);
char* fern_style_on_cyan(const char* text);
char* fern_style_on_white(const char* text);

/* Text attributes */
char* fern_style_bold(const char* text);
char* fern_style_dim(const char* text);
char* fern_style_italic(const char* text);
char* fern_style_underline(const char* text);
char* fern_style_blink(const char* text);
char* fern_style_reverse(const char* text);
char* fern_style_strikethrough(const char* text);

/* 256-color palette */
char* fern_style_color(const char* text, int64_t color_code);
char* fern_style_on_color(const char* text, int64_t color_code);

/* True color (RGB) */
char* fern_style_rgb(const char* text, int64_t r, int64_t g, int64_t b);
char* fern_style_on_rgb(const char* text, int64_t r, int64_t g, int64_t b);

/* Hex color */
char* fern_style_hex(const char* text, const char* hex_color);
char* fern_style_on_hex(const char* text, const char* hex_color);

/* Reset/strip styling */
char* fern_style_reset(const char* text);

/* ========== TUI: Status Badges ========== */

/**
 * Create a WARN badge (orange background, black text).
 * @param message Optional message to append after badge.
 * @return Formatted string like " WARN  message".
 */
char* fern_status_warn(const char* message);

/**
 * Create an OK badge (green background, black text).
 * @param message Optional message to append after badge.
 * @return Formatted string like " OK  message".
 */
char* fern_status_ok(const char* message);

/**
 * Create an INFO badge (blue background, white text).
 * @param message Optional message to append after badge.
 * @return Formatted string like " INFO  message".
 */
char* fern_status_info(const char* message);

/**
 * Create an ERROR badge (red background, white text).
 * @param message Optional message to append after badge.
 * @return Formatted string like " ERROR  message".
 */
char* fern_status_error(const char* message);

/**
 * Create a DEBUG badge (magenta background, white text).
 * @param message Optional message to append after badge.
 * @return Formatted string like " DEBUG  message".
 */
char* fern_status_debug(const char* message);

/* ========== TUI: Live/Spinner Module ========== */

/**
 * Print text without a newline (for same-line updates).
 * @param text The text to print.
 */
void fern_live_print(const char* text);

/**
 * Clear the current line and move cursor to beginning.
 */
void fern_live_clear_line(void);

/**
 * Update the current line with new text (clears line first).
 * @param text The new text to display.
 */
void fern_live_update(const char* text);

/**
 * Finish live update by printing newline.
 */
void fern_live_done(void);

/**
 * Sleep for specified milliseconds.
 * @param ms Milliseconds to sleep.
 */
void fern_sleep_ms(int64_t ms);

/* ========== TUI: Panel Module ========== */

/**
 * Box drawing style for panels and tables.
 */
typedef enum {
    BOX_ROUNDED,    /* ╭─╮│╰─╯ */
    BOX_SQUARE,     /* ┌─┐│└─┘ */
    BOX_DOUBLE,     /* ╔═╗║╚═╝ */
    BOX_HEAVY,      /* ┏━┓┃┗━┛ */
    BOX_ASCII,      /* +-+|+-+ */
    BOX_NONE        /* No borders */
} FernBoxStyle;

/**
 * Panel configuration.
 */
typedef struct FernPanel {
    char* content;
    char* title;
    char* subtitle;
    char* border_color;      /* ANSI color for border (e.g., "cyan", "green", "#00ff00") */
    FernBoxStyle box_style;
    int64_t width;           /* 0 = auto-fit to content, -1 = expand to terminal */
    int64_t padding_h;       /* Horizontal padding */
    int64_t padding_v;       /* Vertical padding */
} FernPanel;

/**
 * Create a new panel with content.
 * @param content The panel content.
 * @return New panel struct.
 */
FernPanel* fern_panel_new(const char* content);

/**
 * Set panel title.
 * @param panel The panel.
 * @param title The title text.
 * @return The panel (for chaining).
 */
FernPanel* fern_panel_title(FernPanel* panel, const char* title);

/**
 * Set panel subtitle.
 * @param panel The panel.
 * @param subtitle The subtitle text.
 * @return The panel (for chaining).
 */
FernPanel* fern_panel_subtitle(FernPanel* panel, const char* subtitle);

/**
 * Set panel border style.
 * @param panel The panel.
 * @param style The box style (0=rounded, 1=square, 2=double, 3=heavy, 4=ascii, 5=none).
 * @return The panel (for chaining).
 */
FernPanel* fern_panel_border(FernPanel* panel, int64_t style);

/**
 * Set panel border style by name.
 * @param panel The panel.
 * @param style Style name ("rounded", "square", "double", "heavy", "ascii", "none").
 * @return The panel (for chaining).
 */
FernPanel* fern_panel_border_str(FernPanel* panel, const char* style);

/**
 * Set panel width.
 * @param panel The panel.
 * @param width The width (0=auto, -1=expand to terminal).
 * @return The panel (for chaining).
 */
FernPanel* fern_panel_width(FernPanel* panel, int64_t width);

/**
 * Set panel padding.
 * @param panel The panel.
 * @param vertical Vertical padding (lines).
 * @param horizontal Horizontal padding (chars).
 * @return The panel (for chaining).
 */
FernPanel* fern_panel_padding(FernPanel* panel, int64_t vertical, int64_t horizontal);

/**
 * Set panel border color.
 * @param panel The panel.
 * @param color Color name ("red", "green", "cyan", etc.) or hex ("#00ff00").
 * @return The panel (for chaining).
 */
FernPanel* fern_panel_border_color(FernPanel* panel, const char* color);

/**
 * Render panel to string.
 * @param panel The panel to render.
 * @return Rendered panel as string.
 */
char* fern_panel_render(FernPanel* panel);

/**
 * Free panel.
 * @param panel The panel to free.
 */
void fern_panel_free(FernPanel* panel);

/* ========== TUI: Table Module ========== */

/**
 * Table column definition.
 */
typedef struct FernTableColumn {
    char* header;
    int64_t min_width;
    int64_t max_width;
    int64_t justify;  /* 0=left, 1=center, 2=right */
} FernTableColumn;

/**
 * Table row.
 */
typedef struct FernTableRow {
    char** cells;
    int64_t cell_count;
} FernTableRow;

/**
 * Table configuration.
 */
typedef struct FernTable {
    char* title;
    char* caption;
    FernTableColumn* columns;
    int64_t column_count;
    FernTableRow* rows;
    int64_t row_count;
    int64_t row_capacity;
    FernBoxStyle box_style;
    int64_t show_header;
    int64_t show_lines;  /* Lines between rows */
    int64_t expand;      /* Expand to terminal width */
} FernTable;

/**
 * Create a new table.
 * @return New table struct.
 */
FernTable* fern_table_new(void);

/**
 * Add a column to the table.
 * @param table The table.
 * @param header The column header.
 * @return The table (for chaining).
 */
FernTable* fern_table_add_column(FernTable* table, const char* header);

/**
 * Add a row to the table.
 * @param table The table.
 * @param cells List of cell values (FernStringList*).
 * @return The table (for chaining).
 */
FernTable* fern_table_add_row(FernTable* table, FernStringList* cells);

/**
 * Set table title.
 * @param table The table.
 * @param title The title text.
 * @return The table (for chaining).
 */
FernTable* fern_table_title(FernTable* table, const char* title);

/**
 * Set table border style.
 * @param table The table.
 * @param style The box style (0=rounded, 1=square, 2=double, 3=heavy, 4=ascii, 5=none).
 * @return The table (for chaining).
 */
FernTable* fern_table_border(FernTable* table, int64_t style);

/**
 * Set whether to show the header row.
 * @param table The table.
 * @param show 1 to show header, 0 to hide.
 * @return The table (for chaining).
 */
FernTable* fern_table_show_header(FernTable* table, int64_t show);

/**
 * Render table to string.
 * @param table The table to render.
 * @return Rendered table as string.
 */
char* fern_table_render(FernTable* table);

/**
 * Free table.
 * @param table The table to free.
 */
void fern_table_free(FernTable* table);

/* ========== Progress Bar Module ========== */

/**
 * Progress bar structure for showing task progress.
 */
typedef struct {
    int64_t total;           /* Total steps */
    int64_t completed;       /* Completed steps */
    int64_t width;           /* Bar width in characters */
    char* description;       /* Description text */
    char* fill_char;         /* Character for filled portion */
    char* empty_char;        /* Character for empty portion */
    int show_percentage;     /* Whether to show percentage */
    int show_count;          /* Whether to show count (x/y) */
} FernProgress;

/**
 * Create a new progress bar.
 * @param total Total number of steps.
 * @return New progress bar.
 */
FernProgress* fern_progress_new(int64_t total);

/**
 * Set progress bar description.
 * @param progress The progress bar.
 * @param description Description text.
 * @return The progress bar (for chaining).
 */
FernProgress* fern_progress_description(FernProgress* progress, const char* description);

/**
 * Set progress bar width.
 * @param progress The progress bar.
 * @param width Width in characters.
 * @return The progress bar (for chaining).
 */
FernProgress* fern_progress_width(FernProgress* progress, int64_t width);

/**
 * Advance progress bar by one step.
 * @param progress The progress bar.
 * @return The progress bar (for chaining).
 */
FernProgress* fern_progress_advance(FernProgress* progress);

/**
 * Set progress to a specific value.
 * @param progress The progress bar.
 * @param value The value to set.
 * @return The progress bar (for chaining).
 */
FernProgress* fern_progress_set(FernProgress* progress, int64_t value);

/**
 * Render progress bar to string.
 * @param progress The progress bar.
 * @return Rendered progress bar as string.
 */
char* fern_progress_render(FernProgress* progress);

/**
 * Free progress bar.
 * @param progress The progress bar to free.
 */
void fern_progress_free(FernProgress* progress);

/* ========== Spinner Module ========== */

/**
 * Spinner style enumeration.
 */
typedef enum {
    SPINNER_DOTS,       /* ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ */
    SPINNER_LINE,       /* -\|/ */
    SPINNER_CIRCLE,     /* ◐◓◑◒ */
    SPINNER_SQUARE,     /* ◰◳◲◱ */
    SPINNER_ARROW,      /* ←↖↑↗→↘↓↙ */
    SPINNER_BOUNCE,     /* ⠁⠂⠄⠂ */
    SPINNER_CLOCK,      /* 🕐🕑🕒🕓🕔🕕🕖🕗🕘🕙🕚🕛 */
} FernSpinnerStyle;

/**
 * Spinner structure for showing activity.
 */
typedef struct {
    FernSpinnerStyle style;  /* Spinner style */
    int frame;               /* Current animation frame */
    char* message;           /* Message to display */
} FernSpinner;

/**
 * Create a new spinner.
 * @return New spinner.
 */
FernSpinner* fern_spinner_new(void);

/**
 * Set spinner message.
 * @param spinner The spinner.
 * @param message Message text.
 * @return The spinner (for chaining).
 */
FernSpinner* fern_spinner_message(FernSpinner* spinner, const char* message);

/**
 * Set spinner style.
 * @param spinner The spinner.
 * @param style Style name ("dots", "line", "circle", etc.).
 * @return The spinner (for chaining).
 */
FernSpinner* fern_spinner_style(FernSpinner* spinner, const char* style);

/**
 * Advance spinner to next frame.
 * @param spinner The spinner.
 * @return The spinner (for chaining).
 */
FernSpinner* fern_spinner_tick(FernSpinner* spinner);

/**
 * Render spinner to string.
 * @param spinner The spinner.
 * @return Rendered spinner as string.
 */
char* fern_spinner_render(FernSpinner* spinner);

/**
 * Free spinner.
 * @param spinner The spinner to free.
 */
void fern_spinner_free(FernSpinner* spinner);

/* ========== Trees, Logs, and Cursor Controls ========== */

/** Opaque immutable terminal tree. */
typedef struct FernTree FernTree;
/** Create a root. @param label Root text. @return New immutable tree. */
FernTree *fern_tree_new(const char *label);
/** Append a subtree. @param tree Parent. @param child Child. @return New immutable tree. */
FernTree *fern_tree_add(FernTree *tree, FernTree *child);
/** Render plain text. @param tree Tree to render. @return Text without trailing newline. */
char *fern_tree_render(FernTree *tree);
/** Format debug text. @param message Text. @return Escaped, single-line record. */
char *fern_log_debug(const char *message);
/** Format info text. @param message Text. @return Escaped, single-line record. */
char *fern_log_info(const char *message);
/** Format warning text. @param message Text. @return Escaped, single-line record. */
char *fern_log_warn(const char *message);
/** Format error text. @param message Text. @return Escaped, single-line record. */
char *fern_log_error(const char *message);
/** Position cursor in TTY. @param row One-based row. @param column One-based column. */
void fern_term_move_to(int64_t row, int64_t column);
/** Move up in TTY. @param count Positive cells, otherwise no-op. */
void fern_term_up(int64_t count);
/** Move down in TTY. @param count Positive cells, otherwise no-op. */
void fern_term_down(int64_t count);
/** Move left in TTY. @param count Positive cells, otherwise no-op. */
void fern_term_left(int64_t count);
/** Move right in TTY. @param count Positive cells, otherwise no-op. */
void fern_term_right(int64_t count);
/** Clear screen and home cursor; no-op in pipes. */
void fern_term_clear(void);
/** Hide cursor; no-op in pipes. */
void fern_term_hide_cursor(void);
/** Show cursor; no-op in pipes. */
void fern_term_show_cursor(void);
/** Save cursor position; no-op in pipes. */
void fern_term_save_cursor(void);
/** Restore cursor position; no-op in pipes. */
void fern_term_restore_cursor(void);

/* ========== Prompt Module ========== */

/**
 * Read a line of input from the user.
 * @param prompt The prompt text to display.
 * @return The user's input (without newline).
 */
char* fern_prompt_input(const char* prompt);

/**
 * Ask a yes/no confirmation question.
 * @param prompt The question to ask.
 * @return 1 for yes, 0 for no.
 */
int fern_prompt_confirm(const char* prompt);

/**
 * Ask user to select from a list of choices.
 * @param prompt The prompt text.
 * @param choices Array of choice strings.
 * @param count Number of choices.
 * @return Index of selected choice (0-based), or -1 on error.
 */
int fern_prompt_select(const char* prompt, FernStringList* choices);

/**
 * Ask for password input (hidden).
 * @param prompt The prompt text.
 * @return The password string.
 */
char* fern_prompt_password(const char* prompt);

/**
 * Ask for integer input with validation.
 * @param prompt The prompt text.
 * @param min Minimum allowed value.
 * @param max Maximum allowed value.
 * @return The entered integer, or min if invalid.
 */
int64_t fern_prompt_int(const char* prompt, int64_t min, int64_t max);


/** Read an immutable JSON Pointer path; dynamic errors have an empty path. @param error opaque error; @return retained path. */
const char* fern_json_value_error_path(const FernJsonError* error);

/** One source-named constructor with its real ordered payload codecs (Decision103 ABI). */
typedef struct FernJsonCodecVariant {
    const char* name;
    int64_t count;
    const struct FernJsonCodec* const* children;
} FernJsonCodecVariant;
/** Compiler-validated finite codec graph; tag11 transparently follows one newtype child. */
typedef struct FernJsonCodec {
    int64_t kind;
    int64_t count;
    /* The compiler-validated kind selects this external ABI pointer union only. */
    union {
        const struct FernJsonCodec* const* children;
        const FernJsonCodecVariant* variants;
    };
    const char* const* names;
} FernJsonCodec;
/** Encode a full-width typed payload under one shared codec budget. @param plan checked descriptor; @param payload native bits; @return Result(String,Error). */
int64_t fern_json_codec_encode(const FernJsonCodec* plan, int64_t payload);
/** Decode into checked native storage. @param plan checked descriptor; @param text source String; @return Result(payload,Error). */
int64_t fern_json_codec_decode(const FernJsonCodec* plan, const char* text);

#endif /* FERN_RUNTIME_H */
