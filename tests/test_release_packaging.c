/* Release Packaging Tests */

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
#include <limits.h>

typedef struct {
    int exit_code;
    char* output;
} CmdResult;

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

static char* make_tmp_dir(void) {
    char templ[] = "/tmp/fern_release_pkg_XXXXXX";
    char* made = mkdtemp(templ);
    if (!made) return NULL;

    char* out = (char*)malloc(strlen(made) + 1);
    if (!out) return NULL;
    strcpy(out, made);
    return out;
}

void test_release_package_script_creates_archive_and_checksum(void) {
    char* tmp = make_tmp_dir();
    ASSERT_NOT_NULL(tmp);

    char staging[PATH_MAX];
    char outdir[PATH_MAX];
    snprintf(staging, sizeof(staging), "%s/staging", tmp);
    snprintf(outdir, sizeof(outdir), "%s/dist", tmp);

    char cmd[4096];
    snprintf(
        cmd,
        sizeof(cmd),
        "mkdir -p %s %s && "
        "printf '#!/bin/sh\\necho fern-test\\n' > %s/fern && chmod +x %s/fern && "
        "for name in fern-c fern-qbe fern-test-supervisor; do cp %s/fern %s/$name; done && "
        "printf '{\"format\":1,\"compiler\":\"rust\",\"backend\":\"qbe\"}\\n' > %s/fern-package.json && "
        "printf 'runtime' > %s/libfern_runtime.a && "
        "cp LICENSE THIRD_PARTY_NOTICES.md %s/ && "
        "python3 scripts/package_release.py package "
        "--version 0.0.0-test --os testos --arch testarch "
        "--staging %s --out-dir %s 2>&1",
        staging, outdir,
        staging, staging,
        staging, staging, staging,
        staging,
        staging,
        staging, outdir
    );
    CmdResult result = run_cmd(cmd);
    ASSERT_EQ(result.exit_code, 0);
    ASSERT_NOT_NULL(result.output);

    char archive[PATH_MAX];
    char checksum[PATH_MAX];
    snprintf(archive, sizeof(archive), "%s/fern-0.0.0-test-testos-testarch.tar.gz", outdir);
    snprintf(checksum, sizeof(checksum), "%s/fern-0.0.0-test-testos-testarch.tar.gz.sha256", outdir);

    struct stat st = {0};
    ASSERT_EQ(stat(archive, &st), 0);
    ASSERT_TRUE(st.st_size > 0);
    ASSERT_EQ(stat(checksum, &st), 0);
    ASSERT_TRUE(st.st_size > 0);

    free(result.output);
    snprintf(cmd, sizeof(cmd), "rm -rf %s", tmp);
    CmdResult cleanup = run_cmd(cmd);
    free(cleanup.output);
    free(tmp);
}

void test_release_package_script_verify_layout_requires_runtime(void) {
    char* tmp = make_tmp_dir();
    ASSERT_NOT_NULL(tmp);

    char staging[PATH_MAX];
    snprintf(staging, sizeof(staging), "%s/staging", tmp);

    char cmd[4096];
    snprintf(
        cmd,
        sizeof(cmd),
        "mkdir -p %s && "
        "printf '#!/bin/sh\\necho fern-test\\n' > %s/fern && chmod +x %s/fern && "
        "cp LICENSE %s/LICENSE && "
        "python3 scripts/package_release.py verify-layout --staging %s 2>&1",
        staging, staging, staging, staging, staging
    );
    CmdResult result = run_cmd(cmd);
    ASSERT_EQ(result.exit_code, 1);
    ASSERT_NOT_NULL(result.output);
    ASSERT_TRUE(strstr(result.output, "libfern_runtime.a") != NULL);

    free(result.output);
    snprintf(cmd, sizeof(cmd), "rm -rf %s", tmp);
    CmdResult cleanup = run_cmd(cmd);
    free(cleanup.output);
    free(tmp);
}

void run_release_packaging_tests(void) {
    printf("\n=== Release Packaging Tests ===\n");
    TEST_RUN(test_release_package_script_creates_archive_and_checksum);
    TEST_RUN(test_release_package_script_verify_layout_requires_runtime);
}
