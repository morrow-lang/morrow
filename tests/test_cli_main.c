/* CLI Main Integration Tests */

#ifdef __linux__
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#endif

#include "test.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>
#include <sys/stat.h>

static char* read_pipe_all(FILE* pipe) {
    if (!pipe) return NULL;

    size_t cap = 1024;
    size_t len = 0;
    char* buf = (char*)malloc(cap);
    if (!buf) return NULL;

    int ch = 0;
    while ((ch = fgetc(pipe)) != EOF) {
        if (len + 1 >= cap) {
            size_t next_cap = cap * 2;
            char* next = (char*)realloc(buf, next_cap);
            if (!next) {
                free(buf);
                return NULL;
            }
            buf = next;
            cap = next_cap;
        }
        buf[len++] = (char)ch;
    }
    buf[len] = '\0';
    return buf;
}

typedef struct {
    int exit_code;
    char* output;
} CmdResult;

static CmdResult run_cmd(const char* cmd) {
    CmdResult result;
    result.exit_code = -1;
    result.output = NULL;

    FILE* pipe = popen(cmd, "r");
    if (!pipe) {
        return result;
    }

    result.output = read_pipe_all(pipe);
    int status = pclose(pipe);
    if (WIFEXITED(status)) {
        result.exit_code = WEXITSTATUS(status);
    }

    return result;
}

static char* write_tmp_source(const char* source) {
    char tmpl[] = "/tmp/fern_cli_test_XXXXXX.fn";
    int fd = mkstemps(tmpl, 3);
    if (fd < 0) return NULL;

    FILE* file = fdopen(fd, "w");
    if (!file) {
        close(fd);
        unlink(tmpl);
        return NULL;
    }

    fputs(source, file);
    fclose(file);

    char* path = (char*)malloc(strlen(tmpl) + 1);
    if (!path) {
        unlink(tmpl);
        return NULL;
    }

    strcpy(path, tmpl);
    return path;
}

static char* read_file_all(const char* path) {
    if (!path) return NULL;

    FILE* file = fopen(path, "rb");
    if (!file) return NULL;

    if (fseek(file, 0, SEEK_END) != 0) {
        fclose(file);
        return NULL;
    }
    long size = ftell(file);
    if (size < 0) {
        fclose(file);
        return NULL;
    }
    if (fseek(file, 0, SEEK_SET) != 0) {
        fclose(file);
        return NULL;
    }

    char* buf = (char*)malloc((size_t)size + 1);
    if (!buf) {
        fclose(file);
        return NULL;
    }

    size_t read_size = fread(buf, 1, (size_t)size, file);
    fclose(file);
    if (read_size != (size_t)size) {
        free(buf);
        return NULL;
    }
    buf[size] = '\0';
    return buf;
}

static char* make_tmp_output_path(void) {
    char tmpl[] = "/tmp/fern_cli_out_XXXXXX";
    int fd = mkstemp(tmpl);
    if (fd < 0) return NULL;
    close(fd);
    unlink(tmpl);

    char* path = (char*)malloc(strlen(tmpl) + 1);
    if (!path) return NULL;
    strcpy(path, tmpl);
    return path;
}

void test_cli_help_lists_global_flags(void) {
    CmdResult result = run_cmd("./bin/fern-c --help 2>&1");
    ASSERT_EQ(result.exit_code, 0);
    ASSERT_NOT_NULL(result.output);

    ASSERT_TRUE(strstr(result.output, "--color=auto|always|never") != NULL);
    ASSERT_TRUE(strstr(result.output, "--quiet") != NULL);
    ASSERT_TRUE(strstr(result.output, "--verbose") != NULL);

    free(result.output);
}

void test_cli_quiet_suppresses_check_success_output(void) {
    char* source_path = write_tmp_source("fn main():\n    0\n");
    ASSERT_NOT_NULL(source_path);

    char cmd[512];
    snprintf(cmd, sizeof(cmd), "./bin/fern-c check %s 2>&1", source_path);
    CmdResult normal = run_cmd(cmd);
    ASSERT_EQ(normal.exit_code, 0);
    ASSERT_NOT_NULL(normal.output);
    ASSERT_TRUE(strstr(normal.output, "No type errors") != NULL);

    snprintf(cmd, sizeof(cmd), "./bin/fern-c --quiet check %s 2>&1", source_path);
    CmdResult quiet = run_cmd(cmd);
    ASSERT_EQ(quiet.exit_code, 0);
    ASSERT_NOT_NULL(quiet.output);
    ASSERT_STR_EQ(quiet.output, "");

    free(normal.output);
    free(quiet.output);
    unlink(source_path);
    free(source_path);
}

void test_cli_verbose_emits_debug_lines(void) {
    char* source_path = write_tmp_source("fn main():\n    0\n");
    ASSERT_NOT_NULL(source_path);

    char cmd[512];
    snprintf(cmd, sizeof(cmd), "./bin/fern-c --verbose check %s 2>&1", source_path);
    CmdResult verbose = run_cmd(cmd);

    ASSERT_EQ(verbose.exit_code, 0);
    ASSERT_NOT_NULL(verbose.output);
    ASSERT_TRUE(strstr(verbose.output, "verbose: command=check") != NULL);

    free(verbose.output);
    unlink(source_path);
    free(source_path);
}

void test_cli_verbose_after_command_emits_debug_lines(void) {
    char* source_path = write_tmp_source("fn main():\n    0\n");
    ASSERT_NOT_NULL(source_path);

    char cmd[512];
    snprintf(cmd, sizeof(cmd), "./bin/fern-c check --verbose %s 2>&1", source_path);
    CmdResult verbose = run_cmd(cmd);

    ASSERT_EQ(verbose.exit_code, 0);
    ASSERT_NOT_NULL(verbose.output);
    ASSERT_TRUE(strstr(verbose.output, "verbose: command=check") != NULL);

    free(verbose.output);
    unlink(source_path);
    free(source_path);
}

void test_cli_color_mode_always_and_never(void) {
    CmdResult always = run_cmd("./bin/fern-c --color=always build 2>&1");
    ASSERT_EQ(always.exit_code, 1);
    ASSERT_NOT_NULL(always.output);
    ASSERT_TRUE(strstr(always.output, "\033[") != NULL);

    CmdResult never = run_cmd("./bin/fern-c --color=never build 2>&1");
    ASSERT_EQ(never.exit_code, 1);
    ASSERT_NOT_NULL(never.output);
    ASSERT_TRUE(strstr(never.output, "\033[") == NULL);

    free(always.output);
    free(never.output);
}

void test_cli_unknown_global_option_reports_unknown_option(void) {
    CmdResult result = run_cmd("./bin/fern-c --bogus 2>&1");
    ASSERT_EQ(result.exit_code, 1);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, "unknown option '--bogus'") != NULL);

    free(result.output);
}

void test_cli_fmt_normalizes_and_is_deterministic(void) {
    char* source_path = write_tmp_source("fn main():  \r\n\tlet x = 1\t\t\r\n\tx\r\n\r\n");
    ASSERT_NOT_NULL(source_path);

    char cmd[512];
    snprintf(cmd, sizeof(cmd), "./bin/fern-c fmt %s 2>&1", source_path);
    CmdResult first = run_cmd(cmd);
    ASSERT_EQ(first.exit_code, 0);
    ASSERT_NOT_NULL(first.output);

    char* once = read_file_all(source_path);
    ASSERT_NOT_NULL(once);
    ASSERT_STR_EQ(once, "fn main():\n\tlet x = 1\n\tx\n");

    CmdResult second = run_cmd(cmd);
    ASSERT_EQ(second.exit_code, 0);
    ASSERT_NOT_NULL(second.output);

    char* twice = read_file_all(source_path);
    ASSERT_NOT_NULL(twice);
    ASSERT_STR_EQ(twice, once);

    free(first.output);
    free(second.output);
    free(once);
    free(twice);
    unlink(source_path);
    free(source_path);
}

void test_cli_e2e_command_flow_fmt_parse_check_build(void) {
    char* source_path = write_tmp_source("fn main():  \r\n\t42\t \r\n");
    ASSERT_NOT_NULL(source_path);
    char* output_path = make_tmp_output_path();
    ASSERT_NOT_NULL(output_path);

    char cmd[768];
    snprintf(cmd, sizeof(cmd), "./bin/fern-c fmt %s 2>&1", source_path);
    CmdResult fmt_result = run_cmd(cmd);
    ASSERT_EQ(fmt_result.exit_code, 0);
    ASSERT_NOT_NULL(fmt_result.output);

    char* formatted = read_file_all(source_path);
    ASSERT_NOT_NULL(formatted);
    ASSERT_STR_EQ(formatted, "fn main():\n\t42\n");

    snprintf(cmd, sizeof(cmd), "./bin/fern-c parse %s 2>&1", source_path);
    CmdResult parse_result = run_cmd(cmd);
    ASSERT_EQ(parse_result.exit_code, 0);
    ASSERT_NOT_NULL(parse_result.output);
    ASSERT_TRUE(strstr(parse_result.output, "AST for ") != NULL);
    ASSERT_TRUE(strstr(parse_result.output, "Fn: main") != NULL);

    snprintf(cmd, sizeof(cmd), "./bin/fern-c check %s 2>&1", source_path);
    CmdResult check_result = run_cmd(cmd);
    ASSERT_EQ(check_result.exit_code, 0);
    ASSERT_NOT_NULL(check_result.output);
    ASSERT_TRUE(strstr(check_result.output, "No type errors") != NULL);

    snprintf(cmd, sizeof(cmd), "./bin/fern-c build -o %s %s 2>&1", output_path, source_path);
    CmdResult build_result = run_cmd(cmd);
    ASSERT_EQ(build_result.exit_code, 0);
    ASSERT_NOT_NULL(build_result.output);
    ASSERT_TRUE(strstr(build_result.output, "Created executable:") != NULL);

    struct stat st = {0};
    ASSERT_EQ(stat(output_path, &st), 0);
    ASSERT_TRUE((st.st_mode & S_IXUSR) != 0);

    snprintf(cmd, sizeof(cmd), "%s 2>&1", output_path);
    CmdResult run_result = run_cmd(cmd);
    ASSERT_EQ(run_result.exit_code, 0);
    ASSERT_NOT_NULL(run_result.output);

    free(fmt_result.output);
    free(parse_result.output);
    free(check_result.output);
    free(build_result.output);
    free(run_result.output);
    free(formatted);
    unlink(source_path);
    free(source_path);
    unlink(output_path);
    free(output_path);
}

void test_cli_check_syntax_error_includes_note_and_help(void) {
    char* source_path = write_tmp_source("fn main():\n    let = 5\n");
    ASSERT_NOT_NULL(source_path);

    char cmd[512];
    snprintf(cmd, sizeof(cmd), "./bin/fern-c check %s 2>&1", source_path);
    CmdResult result = run_cmd(cmd);

    ASSERT_EQ(result.exit_code, 1);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, "error:") != NULL);
    ASSERT_TRUE(strstr(result.output, "let = 5") != NULL);
    ASSERT_TRUE(strstr(result.output, "note:") != NULL);
    ASSERT_TRUE(strstr(result.output, "help:") != NULL);

    free(result.output);
    unlink(source_path);
    free(source_path);
}

void test_cli_check_type_error_includes_snippet_note_and_help(void) {
    char* source_path = write_tmp_source("fn main() -> Int:\n    \"oops\"\n");
    ASSERT_NOT_NULL(source_path);

    char cmd[512];
    snprintf(cmd, sizeof(cmd), "./bin/fern-c check %s 2>&1", source_path);
    CmdResult result = run_cmd(cmd);

    ASSERT_EQ(result.exit_code, 1);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, "error:") != NULL);
    ASSERT_TRUE(strstr(result.output, "declared return type") != NULL);
    ASSERT_TRUE(strstr(result.output, "\"oops\"") != NULL);
    ASSERT_TRUE(strstr(result.output, "note:") != NULL);
    ASSERT_TRUE(strstr(result.output, "help:") != NULL);

    free(result.output);
    unlink(source_path);
    free(source_path);
}

void test_cli_check_binary_type_error_includes_snippet_note_and_help(void) {
    char* source_path = write_tmp_source(
        "fn main() -> Int:\n"
        "    let x = \"hi\"\n"
        "    x + 1\n"
    );
    ASSERT_NOT_NULL(source_path);

    char cmd[512];
    snprintf(cmd, sizeof(cmd), "./bin/fern-c check %s 2>&1", source_path);
    CmdResult result = run_cmd(cmd);

    ASSERT_EQ(result.exit_code, 1);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, "error:") != NULL);
    ASSERT_TRUE(strstr(result.output, "Cannot apply '+' to String and Int") != NULL);
    ASSERT_TRUE(strstr(result.output, "x + 1") != NULL);
    ASSERT_TRUE(strstr(result.output, "note:") != NULL);
    ASSERT_TRUE(strstr(result.output, "help:") != NULL);

    free(result.output);
    unlink(source_path);
    free(source_path);
}

void test_cli_test_command_runs_unit_tests(void) {
    CmdResult result = run_cmd(
        "FERN_TEST_CMD='echo unit-tests-ok' "
        "FERN_TEST_DOC_CMD='echo doc-tests-ok' "
        "./bin/fern-c test 2>&1"
    );
    ASSERT_EQ(result.exit_code, 0);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, "unit-tests-ok") != NULL);
    ASSERT_TRUE(strstr(result.output, "doc-tests-ok") != NULL);

    free(result.output);
}

void test_cli_test_doc_command_runs_doc_tests(void) {
    CmdResult result = run_cmd("FERN_TEST_DOC_CMD='echo doc-tests-ok' ./bin/fern-c test --doc 2>&1");
    ASSERT_EQ(result.exit_code, 0);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, "doc-tests-ok") != NULL);
    ASSERT_TRUE(strstr(result.output, "unit-tests-ok") == NULL);

    free(result.output);
}

void test_cli_test_doc_command_honors_file_argument(void) {
    char* source_path = write_tmp_source(
        "module doctest_fixture\n"
        "\n"
        "@doc \"\"\"\n"
        "# Examples\n"
        "\n"
        "```fern\n"
        "missing_identifier\n"
        "```\n"
        "\"\"\"\n"
        "fn marker() -> Int:\n"
        "    0\n"
    );
    ASSERT_NOT_NULL(source_path);

    char cmd[1024];
    snprintf(cmd, sizeof(cmd), "./bin/fern-c test --doc %s 2>&1", source_path);
    CmdResult result = run_cmd(cmd);

    ASSERT_NE(result.exit_code, 0);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, source_path) != NULL);
    ASSERT_TRUE(strstr(result.output, "doc test") != NULL);

    free(result.output);
    unlink(source_path);
    free(source_path);
}

void test_cli_help_lists_doc_command(void) {
    CmdResult result = run_cmd("./bin/fern-c --help 2>&1");
    ASSERT_EQ(result.exit_code, 0);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, "doc") != NULL);
    ASSERT_TRUE(strstr(result.output, "Generate documentation") != NULL);

    free(result.output);
}

void test_cli_doc_command_runs_generator(void) {
    CmdResult result = run_cmd("FERN_DOC_CMD='echo docs-ok' ./bin/fern-c doc 2>&1");
    ASSERT_EQ(result.exit_code, 0);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, "docs-ok") != NULL);

    free(result.output);
}

void test_cli_doc_open_command_runs_generator(void) {
    CmdResult result = run_cmd("FERN_DOC_OPEN_CMD='echo docs-open-ok' ./bin/fern-c doc --open 2>&1");
    ASSERT_EQ(result.exit_code, 0);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, "docs-open-ok") != NULL);

    free(result.output);
}

void test_cli_open_option_only_valid_for_doc(void) {
    CmdResult result = run_cmd("./bin/fern-c test --open 2>&1");
    ASSERT_EQ(result.exit_code, 1);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, "--open is only valid for the doc command") != NULL);

    free(result.output);
}

void test_cli_doc_generates_cross_linked_markdown_with_doc_blocks(void) {
    unlink("docs/generated/test_doc_source.fn");
    unlink("docs/generated/fern-docs.md");

    char* source_path = write_tmp_source(
        "module demo\n"
        "\n"
        "@doc \"\"\"\n"
        "Increment an integer by one.\n"
        "\n"
        "Used by other examples.\n"
        "\"\"\"\n"
        "fn inc(x: Int) -> Int:\n"
        "    x + 1\n"
        "\n"
        "fn other() -> Int:\n"
        "    0\n"
    );
    ASSERT_NOT_NULL(source_path);

    CmdResult result = run_cmd("mkdir -p docs/generated && ./bin/fern-c doc docs/generated/test_doc_source.fn 2>&1");
    ASSERT_EQ(result.exit_code, 1);
    free(result.output);

    char cmd[768];
    snprintf(cmd, sizeof(cmd), "cp %s docs/generated/test_doc_source.fn", source_path);
    CmdResult copy_result = run_cmd(cmd);
    ASSERT_EQ(copy_result.exit_code, 0);
    free(copy_result.output);

    CmdResult doc_result = run_cmd("./bin/fern-c doc docs/generated/test_doc_source.fn 2>&1");
    ASSERT_EQ(doc_result.exit_code, 0);
    ASSERT_NOT_NULL(doc_result.output);

    char* docs = read_file_all("docs/generated/fern-docs.md");
    ASSERT_NOT_NULL(docs);
    ASSERT_TRUE(strstr(docs, "## Modules") != NULL);
    ASSERT_TRUE(strstr(docs, "[`demo`]") != NULL);
    ASSERT_TRUE(strstr(docs, "Increment an integer by one.") != NULL);
    ASSERT_TRUE(strstr(docs, "[`inc`](#demo-inc)") != NULL);

    free(doc_result.output);
    free(docs);
    unlink("docs/generated/test_doc_source.fn");
    unlink("docs/generated/fern-docs.md");
    unlink(source_path);
    free(source_path);
}

void test_cli_doc_html_output_generation(void) {
    unlink("docs/generated/test_doc_html.fn");
    unlink("docs/generated/fern-docs.html");

    char* source_path = write_tmp_source(
        "module html_demo\n"
        "fn main() -> Int:\n"
        "    0\n"
    );
    ASSERT_NOT_NULL(source_path);

    char cmd[768];
    snprintf(cmd, sizeof(cmd), "mkdir -p docs/generated && cp %s docs/generated/test_doc_html.fn", source_path);
    CmdResult prep = run_cmd(cmd);
    ASSERT_EQ(prep.exit_code, 0);
    free(prep.output);

    CmdResult result = run_cmd("./bin/fern-c doc --html docs/generated/test_doc_html.fn 2>&1");
    ASSERT_EQ(result.exit_code, 0);
    ASSERT_NOT_NULL(result.output);

    char* html = read_file_all("docs/generated/fern-docs.html");
    ASSERT_NOT_NULL(html);
    ASSERT_TRUE(strstr(html, "<html") != NULL);
    ASSERT_TRUE(strstr(html, "Fern Documentation") != NULL);
    ASSERT_TRUE(strstr(html, "href=\"#html_demo\"") != NULL);

    free(result.output);
    free(html);
    unlink("docs/generated/test_doc_html.fn");
    unlink("docs/generated/fern-docs.html");
    unlink(source_path);
    free(source_path);
}

void run_cli_main_tests(void) {
    printf("\n=== CLI Main Tests ===\n");
    TEST_RUN(test_cli_help_lists_global_flags);
    TEST_RUN(test_cli_help_lists_doc_command);
    TEST_RUN(test_cli_quiet_suppresses_check_success_output);
    TEST_RUN(test_cli_verbose_emits_debug_lines);
    TEST_RUN(test_cli_verbose_after_command_emits_debug_lines);
    TEST_RUN(test_cli_color_mode_always_and_never);
    TEST_RUN(test_cli_unknown_global_option_reports_unknown_option);
    TEST_RUN(test_cli_fmt_normalizes_and_is_deterministic);
    TEST_RUN(test_cli_e2e_command_flow_fmt_parse_check_build);
    TEST_RUN(test_cli_check_syntax_error_includes_note_and_help);
    TEST_RUN(test_cli_check_type_error_includes_snippet_note_and_help);
    TEST_RUN(test_cli_check_binary_type_error_includes_snippet_note_and_help);
    TEST_RUN(test_cli_test_command_runs_unit_tests);
    TEST_RUN(test_cli_test_doc_command_runs_doc_tests);
    TEST_RUN(test_cli_test_doc_command_honors_file_argument);
    TEST_RUN(test_cli_doc_command_runs_generator);
    TEST_RUN(test_cli_doc_open_command_runs_generator);
    TEST_RUN(test_cli_open_option_only_valid_for_doc);
    TEST_RUN(test_cli_doc_generates_cross_linked_markdown_with_doc_blocks);
    TEST_RUN(test_cli_doc_html_output_generation);
}
