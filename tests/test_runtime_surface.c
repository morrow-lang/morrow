/* Gate C Runtime Surface Integration Tests */

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
#include <sys/socket.h>
#include <netinet/in.h>
#include <signal.h>
#include <errno.h>

typedef struct {
    int exit_code;
    char* output;
} CmdResult;

typedef struct {
    CmdResult build;
    CmdResult run;
} BuildRunResult;

typedef struct {
    pid_t pid;
    int port;
} TestHttpServer;

static const char* TEST_HTTPS_SERVER_SCRIPT = "tests/fixtures/runtime_https_server.py";
static const char* TEST_HTTPS_SERVER_CERT = "tests/fixtures/runtime_https_cert.pem";
static const char* TEST_HTTPS_SERVER_KEY = "tests/fixtures/runtime_https_key.pem";

static char* dup_cstr(const char* text) {
    if (!text) return NULL;
    size_t len = strlen(text);
    char* copy = (char*)malloc(len + 1);
    if (!copy) return NULL;
    memcpy(copy, text, len + 1);
    return copy;
}

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

static char* write_tmp_source(const char* source) {
    char tmpl[] = "/tmp/fern_runtime_surface_XXXXXX.fn";
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

    return dup_cstr(tmpl);
}

static char* write_tmp_c_source(const char* source) {
    char tmpl[] = "/tmp/fern_runtime_surface_XXXXXX.c";
    int fd = mkstemps(tmpl, 2);
    if (fd < 0) return NULL;

    FILE* file = fdopen(fd, "w");
    if (!file) {
        close(fd);
        unlink(tmpl);
        return NULL;
    }

    fputs(source, file);
    fclose(file);

    return dup_cstr(tmpl);
}

static char* make_tmp_output_path(void) {
    char tmpl[] = "/tmp/fern_runtime_surface_out_XXXXXX";
    int fd = mkstemp(tmpl);
    if (fd < 0) return NULL;
    close(fd);
    unlink(tmpl);
    return dup_cstr(tmpl);
}

static void free_cmd_result(CmdResult* result) {
    if (!result) return;
    free(result->output);
    result->output = NULL;
}

static void free_build_run_result(BuildRunResult* result) {
    if (!result) return;
    free_cmd_result(&result->build);
    free_cmd_result(&result->run);
}

static int parse_content_length(const char* request) {
    if (!request) return -1;
    const char* marker = strstr(request, "Content-Length:");
    if (!marker) return -1;
    marker += strlen("Content-Length:");
    while (*marker == ' ') marker++;
    return atoi(marker);
}

static int write_http_response(int fd, int status, const char* body, size_t body_len) {
    const char* status_text = status == 200 ? "OK" : "Not Found";
    char header[256];
    int header_len = snprintf(
        header,
        sizeof(header),
        "HTTP/1.1 %d %s\r\n"
        "Content-Length: %zu\r\n"
        "Connection: close\r\n"
        "Content-Type: text/plain\r\n"
        "\r\n",
        status,
        status_text,
        body_len
    );
    if (header_len <= 0 || (size_t)header_len >= sizeof(header)) {
        return -1;
    }
    if (write(fd, header, (size_t)header_len) < 0) return -1;
    if (body_len > 0 && write(fd, body, body_len) < 0) return -1;
    return 0;
}

static int run_test_http_server(int port) {
    int listen_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (listen_fd < 0) return 1;

    int yes = 1;
    setsockopt(listen_fd, SOL_SOCKET, SO_REUSEADDR, &yes, sizeof(yes));

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((uint16_t)port);
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);

    if (bind(listen_fd, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
        close(listen_fd);
        return 2;
    }
    if (listen(listen_fd, 8) != 0) {
        close(listen_fd);
        return 3;
    }

    for (int i = 0; i < 6; i++) {
        int client_fd = accept(listen_fd, NULL, NULL);
        if (client_fd < 0) {
            if (errno == EINTR) continue;
            close(listen_fd);
            return 4;
        }

        char req[8192];
        ssize_t nread = read(client_fd, req, sizeof(req) - 1);
        if (nread <= 0) {
            close(client_fd);
            continue;
        }
        req[nread] = '\0';

        const char* body_start = strstr(req, "\r\n\r\n");
        if (body_start) body_start += 4;

        if (strncmp(req, "GET /health ", 12) == 0) {
            write_http_response(client_fd, 200, "ok", 2);
        } else if (strncmp(req, "POST /echo ", 11) == 0 && body_start) {
            int body_len = parse_content_length(req);
            if (body_len < 0) {
                body_len = (int)strlen(body_start);
            }
            if (body_len < 0) body_len = 0;
            write_http_response(client_fd, 200, body_start, (size_t)body_len);
        } else {
            write_http_response(client_fd, 404, "not found", 9);
        }

        close(client_fd);
    }

    close(listen_fd);
    return 0;
}

static bool wait_for_http_server_ready(int port) {
    for (int i = 0; i < 50; i++) {
        int fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) return false;

        struct sockaddr_in addr;
        memset(&addr, 0, sizeof(addr));
        addr.sin_family = AF_INET;
        addr.sin_port = htons((uint16_t)port);
        addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);

        int rc = connect(fd, (struct sockaddr*)&addr, sizeof(addr));
        close(fd);
        if (rc == 0) {
            return true;
        }
        usleep(20 * 1000);
    }
    return false;
}

static TestHttpServer start_test_http_server(int port) {
    TestHttpServer server;
    server.pid = -1;
    server.port = port;

    pid_t pid = fork();
    if (pid < 0) return server;

    if (pid == 0) {
        int code = run_test_http_server(port);
        _exit(code);
    }

    if (!wait_for_http_server_ready(port)) {
        kill(pid, SIGTERM);
        waitpid(pid, NULL, 0);
        return server;
    }

    server.pid = pid;
    return server;
}

static TestHttpServer start_test_https_server(int port) {
    TestHttpServer server;
    server.pid = -1;
    server.port = port;

    if (access(TEST_HTTPS_SERVER_SCRIPT, X_OK) != 0) return server;
    if (access(TEST_HTTPS_SERVER_CERT, R_OK) != 0) return server;
    if (access(TEST_HTTPS_SERVER_KEY, R_OK) != 0) return server;

    pid_t pid = fork();
    if (pid < 0) return server;

    if (pid == 0) {
        char port_str[32];
        snprintf(port_str, sizeof(port_str), "%d", port);
        execlp("python3",
               "python3",
               TEST_HTTPS_SERVER_SCRIPT,
               port_str,
               TEST_HTTPS_SERVER_CERT,
               TEST_HTTPS_SERVER_KEY,
               (char*)NULL);
        _exit(127);
    }

    if (!wait_for_http_server_ready(port)) {
        kill(pid, SIGTERM);
        waitpid(pid, NULL, 0);
        return server;
    }

    server.pid = pid;
    return server;
}

static void stop_test_http_server(TestHttpServer server) {
    if (server.pid <= 0) return;
    kill(server.pid, SIGTERM);
    waitpid(server.pid, NULL, 0);
}

static BuildRunResult build_and_run_source(const char* source) {
    BuildRunResult result;
    result.build.exit_code = -1;
    result.build.output = NULL;
    result.run.exit_code = -1;
    result.run.output = NULL;

    char* source_path = write_tmp_source(source);
    if (!source_path) {
        result.build.output = dup_cstr("failed to create temporary source");
        return result;
    }

    char* output_path = make_tmp_output_path();
    if (!output_path) {
        result.build.output = dup_cstr("failed to create temporary output path");
        unlink(source_path);
        free(source_path);
        return result;
    }

    char cmd[1536];
    snprintf(
        cmd,
        sizeof(cmd),
        "mise run runtime-lib >/dev/null 2>&1 && "
        "./bin/fern-c build -o %s %s 2>&1",
        output_path,
        source_path
    );
    result.build = run_cmd(cmd);
    if (result.build.exit_code == 0) {
        snprintf(cmd, sizeof(cmd), "%s 2>&1", output_path);
        result.run = run_cmd(cmd);
    }

    unlink(source_path);
    unlink(output_path);
    free(source_path);
    free(output_path);
    return result;
}

static BuildRunResult build_and_run_c_source(const char* source) {
    BuildRunResult result;
    result.build.exit_code = -1;
    result.build.output = NULL;
    result.run.exit_code = -1;
    result.run.output = NULL;

    char* source_path = write_tmp_c_source(source);
    if (!source_path) {
        result.build.output = dup_cstr("failed to create temporary C source");
        return result;
    }

    char* output_path = make_tmp_output_path();
    if (!output_path) {
        result.build.output = dup_cstr("failed to create temporary C output path");
        unlink(source_path);
        free(source_path);
        return result;
    }

    char cmd[1536];
    snprintf(
        cmd,
        sizeof(cmd),
        "mise run runtime-lib >/dev/null 2>&1 && "
        "cc -std=c11 -Wall -Wextra -Werror -Iruntime "
        "%s bin/libfern_runtime.a "
        "$(pkg-config --libs bdw-gc 2>/dev/null || echo -lgc) "
        "$(pkg-config --libs sqlite3 2>/dev/null || echo -lsqlite3) "
        "$(pkg-config --libs openssl 2>/dev/null || echo -lssl -lcrypto) "
        "-pthread "
        "-o %s 2>&1",
        source_path,
        output_path
    );
    result.build = run_cmd(cmd);
    if (result.build.exit_code == 0) {
        snprintf(cmd, sizeof(cmd), "%s 2>&1", output_path);
        result.run = run_cmd(cmd);
    }

    unlink(source_path);
    unlink(output_path);
    free(source_path);
    free(output_path);
    return result;
}

void test_runtime_json_parse_empty_returns_err_code(void) {
    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match json.parse(\"\"):\n"
        "        Ok(_) -> 10\n"
        "        Err(code) -> code\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 3);

    free_build_run_result(&result);
}

void test_runtime_json_stringify_empty_returns_ok_empty(void) {
    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match json.stringify(\"\"):\n"
        "        Ok(text) -> if String.eq(text, \"\"): 0 else: 11\n"
        "        Err(_) -> 12\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_http_get_empty_returns_io_error(void) {
    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match http.get(\"\"):\n"
        "        Ok(_) -> 10\n"
        "        Err(code) -> if code == 3: 0 else: 11\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_http_get_returns_ok_body(void) {
    TestHttpServer server = start_test_http_server(19081);
    ASSERT_TRUE(server.pid > 0);

    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match http.get(\"http://127.0.0.1:19081/health\"):\n"
        "        Ok(body) -> if String.eq(body, \"ok\"): 0 else: 10\n"
        "        Err(_) -> 11\n");

    stop_test_http_server(server);
    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_http_post_returns_ok_body(void) {
    TestHttpServer server = start_test_http_server(19082);
    ASSERT_TRUE(server.pid > 0);

    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match http.post(\"http://127.0.0.1:19082/echo\", \"ping\"):\n"
        "        Ok(body) -> if String.eq(body, \"ping\"): 0 else: 10\n"
        "        Err(_) -> 11\n");

    stop_test_http_server(server);
    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_http_get_https_returns_ok_body(void) {
    TestHttpServer server = start_test_https_server(19083);
    ASSERT_TRUE(server.pid > 0);

    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match http.get(\"https://127.0.0.1:19083/health\"):\n"
        "        Ok(body) -> if String.eq(body, \"ok\"): 0 else: 10\n"
        "        Err(_) -> 11\n");

    stop_test_http_server(server);
    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_http_post_https_returns_ok_body(void) {
    TestHttpServer server = start_test_https_server(19084);
    ASSERT_TRUE(server.pid > 0);

    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match http.post(\"https://127.0.0.1:19084/echo\", \"ping\"):\n"
        "        Ok(body) -> if String.eq(body, \"ping\"): 0 else: 10\n"
        "        Err(_) -> 11\n");

    stop_test_http_server(server);
    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_http_post_invalid_url_returns_io_error(void) {
    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match http.post(\"not-a-url\", \"payload\"):\n"
        "        Ok(_) -> 10\n"
        "        Err(code) -> if code == 3: 0 else: 11\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_sql_open_empty_returns_io_error(void) {
    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match sql.open(\"\"):\n"
        "        Ok(_) -> 10\n"
        "        Err(code) -> if code == 3: 0 else: 11\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_sql_open_and_execute_returns_ok(void) {
    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match sql.open(\"/tmp/fern_runtime_sql_surface.db\"):\n"
        "        Ok(handle) ->\n"
        "            match sql.execute(handle, \"DROP TABLE IF EXISTS users\"):\n"
        "                Ok(_) ->\n"
        "                    match sql.execute(handle, \"CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT)\"):\n"
        "                        Ok(_) ->\n"
        "                            match sql.execute(handle, \"INSERT INTO users(name) VALUES ('fern')\"):\n"
        "                                Ok(changed) -> if changed == 1: 0 else: 12\n"
        "                                Err(_) -> 13\n"
        "                        Err(_) -> 14\n"
        "                Err(_) -> 15\n"
        "        Err(_) -> 16\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_sql_execute_invalid_handle_returns_io_error(void) {
    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match sql.execute(424242, \"select 1\"):\n"
        "        Ok(_) -> 10\n"
        "        Err(code) -> if code == 3: 0 else: 11\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actors_start_returns_monotonic_ids(void) {
    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    let first = actors.start(\"worker\")\n"
        "    let second = actors.start(\"worker\")\n"
        "    if second > first: 0 else: 1\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actors_post_and_next_mailbox_contract(void) {
    BuildRunResult result = build_and_run_source(
        "fn post_status(pid: Int) -> Int:\n"
        "    match actors.post(pid, \"ping\"):\n"
        "        Ok(sent) -> sent\n"
        "        Err(_) -> -1\n"
        "\n"
        "fn post_status_2(pid: Int) -> Int:\n"
        "    match actors.post(pid, \"pong\"):\n"
        "        Ok(sent) -> sent\n"
        "        Err(_) -> -1\n"
        "\n"
        "fn first_msg_status(pid: Int) -> Int:\n"
        "    match actors.next(pid):\n"
        "        Ok(msg) -> if String.eq(msg, \"ping\"): 0 else: 41\n"
        "        Err(_) -> 42\n"
        "\n"
        "fn second_msg_status(pid: Int) -> Int:\n"
        "    match actors.next(pid):\n"
        "        Ok(msg) -> if String.eq(msg, \"pong\"): 0 else: 43\n"
        "        Err(_) -> 44\n"
        "\n"
        "fn third_msg_status(pid: Int) -> Int:\n"
        "    match actors.next(pid):\n"
        "        Ok(_) -> 45\n"
        "        Err(code) -> code\n"
        "\n"
        "fn main() -> Int:\n"
        "    let pid = actors.start(\"worker\")\n"
        "    let sent1 = post_status(pid)\n"
        "    let sent2 = post_status_2(pid)\n"
        "    let first = first_msg_status(pid)\n"
        "    let second = second_msg_status(pid)\n"
        "    let third = third_msg_status(pid)\n"
        "    if sent1 == 0 and sent2 == 0 and first == 0 and second == 0 and third == 3: 0 else: 1\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actor_scheduler_round_robin_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t a = fern_actor_spawn(\"a\");\n"
        "    int64_t b = fern_actor_spawn(\"b\");\n"
        "    if (a <= 0 || b <= a) return 60;\n"
        "\n"
        "    int64_t r1 = fern_actor_send(a, \"a1\");\n"
        "    int64_t r2 = fern_actor_send(a, \"a2\");\n"
        "    int64_t r3 = fern_actor_send(b, \"b1\");\n"
        "    if (!fern_result_is_ok(r1) || !fern_result_is_ok(r2) || !fern_result_is_ok(r3)) return 61;\n"
        "\n"
        "    if (fern_actor_mailbox_len(a) != 2) return 62;\n"
        "    if (fern_actor_mailbox_len(b) != 1) return 63;\n"
        "\n"
        "    int64_t first = fern_actor_scheduler_next();\n"
        "    int64_t second = fern_actor_scheduler_next();\n"
        "    int64_t third = fern_actor_scheduler_next();\n"
        "    int64_t fourth = fern_actor_scheduler_next();\n"
        "    if (first != a) return 64;\n"
        "    if (second != b) return 65;\n"
        "    if (third != a) return 66;\n"
        "    if (fourth != 0) return 67;\n"
        "\n"
        "    int64_t m1 = fern_actor_receive(a);\n"
        "    int64_t m2 = fern_actor_receive(b);\n"
        "    int64_t m3 = fern_actor_receive(a);\n"
        "    int64_t m4 = fern_actor_receive(a);\n"
        "    if (!fern_result_is_ok(m1)) return 68;\n"
        "    if (!fern_result_is_ok(m2)) return 69;\n"
        "    if (!fern_result_is_ok(m3)) return 70;\n"
        "    if (fern_result_is_ok(m4)) return 71;\n"
        "\n"
        "    const char* s1 = (const char*)(intptr_t)fern_result_unwrap(m1);\n"
        "    const char* s2 = (const char*)(intptr_t)fern_result_unwrap(m2);\n"
        "    const char* s3 = (const char*)(intptr_t)fern_result_unwrap(m3);\n"
        "    if (!fern_str_eq(s1, \"a1\")) return 72;\n"
        "    if (!fern_str_eq(s2, \"b1\")) return 73;\n"
        "    if (!fern_str_eq(s3, \"a2\")) return 74;\n"
        "\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actor_spawn_link_exit_notification_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t supervisor = fern_actor_spawn(\"supervisor\");\n"
        "    if (supervisor <= 0) return 80;\n"
        "\n"
        "    if (fern_actor_set_current(supervisor) != 0) return 86;\n"
        "\n"
        "    int64_t worker = fern_actor_spawn_link(\"worker\");\n"
        "    if (worker <= supervisor) return 81;\n"
        "\n"
        "    int64_t exit_status = fern_actor_exit(worker, \"boom\");\n"
        "    if (!fern_result_is_ok(exit_status)) return 82;\n"
        "\n"
        "    int64_t msg = fern_actor_next(supervisor);\n"
        "    if (!fern_result_is_ok(msg)) return 83;\n"
        "\n"
        "    const char* text = (const char*)(intptr_t)fern_result_unwrap(msg);\n"
        "    if (!fern_str_starts_with(text, \"Exit(\")) return 84;\n"
        "    if (!fern_str_contains(text, \"boom\")) return 85;\n"
        "    if (fern_actor_set_current(0) != 0) return 87;\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actor_spawn_link_requires_current_actor_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t supervisor = fern_actor_spawn(\"supervisor\");\n"
        "    if (supervisor <= 0) return 90;\n"
        "\n"
        "    if (fern_actor_set_current(0) != 0) return 91;\n"
        "    int64_t worker = fern_actor_spawn_link(\"worker\");\n"
        "    if (worker != 0) return 92;\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actor_monitor_and_restart_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t supervisor = fern_actor_spawn(\"supervisor\");\n"
        "    int64_t worker = fern_actor_spawn(\"worker\");\n"
        "    if (supervisor <= 0 || worker <= supervisor) return 100;\n"
        "\n"
        "    int64_t mon = fern_actor_monitor(supervisor, worker);\n"
        "    if (!fern_result_is_ok(mon)) return 101;\n"
        "\n"
        "    int64_t exited = fern_actor_exit(worker, \"boom\");\n"
        "    if (!fern_result_is_ok(exited)) return 102;\n"
        "\n"
        "    int64_t down = fern_actor_next(supervisor);\n"
        "    if (!fern_result_is_ok(down)) return 103;\n"
        "    const char* down_msg = (const char*)(intptr_t)fern_result_unwrap(down);\n"
        "    if (!fern_str_starts_with(down_msg, \"DOWN(\")) return 104;\n"
        "    if (!fern_str_contains(down_msg, \"boom\")) return 105;\n"
        "\n"
        "    int64_t restarted = fern_actor_restart(worker);\n"
        "    if (!fern_result_is_ok(restarted)) return 106;\n"
        "\n"
        "    int64_t new_worker = fern_result_unwrap(restarted);\n"
        "    if (new_worker <= worker) return 107;\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actor_demonitor_stops_down_notifications_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t supervisor = fern_actor_spawn(\"supervisor\");\n"
        "    int64_t worker = fern_actor_spawn(\"worker\");\n"
        "    if (supervisor <= 0 || worker <= supervisor) return 150;\n"
        "\n"
        "    int64_t mon = fern_actor_monitor(supervisor, worker);\n"
        "    if (!fern_result_is_ok(mon)) return 151;\n"
        "\n"
        "    int64_t dem = fern_actor_demonitor(supervisor, worker);\n"
        "    if (!fern_result_is_ok(dem)) return 152;\n"
        "\n"
        "    int64_t exited = fern_actor_exit(worker, \"boom\");\n"
        "    if (!fern_result_is_ok(exited)) return 153;\n"
        "\n"
        "    int64_t msg = fern_actor_next(supervisor);\n"
        "    if (fern_result_is_ok(msg)) return 154;\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actor_supervise_restart_intensity_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include <stdio.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t supervisor = fern_actor_spawn(\"supervisor\");\n"
        "    int64_t worker = fern_actor_spawn(\"worker\");\n"
        "    if (supervisor <= 0 || worker <= supervisor) return 110;\n"
        "\n"
        "    int64_t spec = fern_actor_supervise(supervisor, worker, 1, 60);\n"
        "    if (!fern_result_is_ok(spec)) return 111;\n"
        "\n"
        "    int64_t exited = fern_actor_exit(worker, \"boom\");\n"
        "    if (!fern_result_is_ok(exited)) return 112;\n"
        "\n"
        "    int64_t down = fern_actor_next(supervisor);\n"
        "    int64_t restart = fern_actor_next(supervisor);\n"
        "    if (!fern_result_is_ok(down) || !fern_result_is_ok(restart)) return 113;\n"
        "\n"
        "    const char* down_msg = (const char*)(intptr_t)fern_result_unwrap(down);\n"
        "    const char* restart_msg = (const char*)(intptr_t)fern_result_unwrap(restart);\n"
        "    if (!fern_str_starts_with(down_msg, \"DOWN(\")) return 114;\n"
        "    if (!fern_str_starts_with(restart_msg, \"RESTART(\")) return 115;\n"
        "\n"
        "    long long old_pid = 0;\n"
        "    long long new_pid = 0;\n"
        "    if (sscanf(restart_msg, \"RESTART(%lld,%lld)\", &old_pid, &new_pid) != 2) return 116;\n"
        "    if (old_pid != worker) return 117;\n"
        "    if (new_pid <= old_pid) return 118;\n"
        "\n"
        "    int64_t exited_again = fern_actor_exit((int64_t)new_pid, \"boom2\");\n"
        "    if (fern_result_is_ok(exited_again)) return 119;\n"
        "\n"
        "    int64_t down2 = fern_actor_next(supervisor);\n"
        "    int64_t escalate = fern_actor_next(supervisor);\n"
        "    if (!fern_result_is_ok(down2) || !fern_result_is_ok(escalate)) return 120;\n"
        "\n"
        "    const char* esc_msg = (const char*)(intptr_t)fern_result_unwrap(escalate);\n"
        "    if (!fern_str_starts_with(esc_msg, \"ESCALATE(\")) return 121;\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actor_supervise_normal_exit_does_not_restart_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t supervisor = fern_actor_spawn(\"supervisor\");\n"
        "    int64_t worker = fern_actor_spawn(\"worker\");\n"
        "    if (supervisor <= 0 || worker <= supervisor) return 160;\n"
        "\n"
        "    int64_t spec = fern_actor_supervise(supervisor, worker, 3, 60);\n"
        "    if (!fern_result_is_ok(spec)) return 161;\n"
        "\n"
        "    int64_t exited = fern_actor_exit(worker, \"normal\");\n"
        "    if (!fern_result_is_ok(exited)) return 162;\n"
        "    if (fern_result_unwrap(exited) != 0) return 163;\n"
        "\n"
        "    int64_t down = fern_actor_next(supervisor);\n"
        "    if (!fern_result_is_ok(down)) return 164;\n"
        "    const char* down_msg = (const char*)(intptr_t)fern_result_unwrap(down);\n"
        "    if (!fern_str_starts_with(down_msg, \"DOWN(\")) return 165;\n"
        "    if (!fern_str_contains(down_msg, \"normal\")) return 166;\n"
        "\n"
        "    int64_t msg2 = fern_actor_next(supervisor);\n"
        "    if (fern_result_is_ok(msg2)) return 167;\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actor_supervision_uses_deterministic_clock_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    if (fern_actor_clock_set(100) != 0) return 170;\n"
        "    if (fern_actor_clock_now() != 100) return 171;\n"
        "\n"
        "    int64_t supervisor = fern_actor_spawn(\"supervisor\");\n"
        "    int64_t worker = fern_actor_spawn(\"worker\");\n"
        "    if (supervisor <= 0 || worker <= supervisor) return 172;\n"
        "\n"
        "    int64_t spec = fern_actor_supervise(supervisor, worker, 1, 5);\n"
        "    if (!fern_result_is_ok(spec)) return 173;\n"
        "\n"
        "    int64_t first = fern_actor_exit(worker, \"boom\");\n"
        "    if (!fern_result_is_ok(first)) return 174;\n"
        "    int64_t w2 = fern_result_unwrap(first);\n"
        "    if (w2 <= worker) return 175;\n"
        "\n"
        "    int64_t second = fern_actor_exit(w2, \"boom2\");\n"
        "    if (fern_result_is_ok(second)) return 176;\n"
        "\n"
        "    if (fern_actor_clock_advance(5) != 0) return 177;\n"
        "    if (fern_actor_clock_now() != 105) return 178;\n"
        "\n"
        "    int64_t restarted = fern_actor_restart(w2);\n"
        "    if (!fern_result_is_ok(restarted)) return 179;\n"
        "    int64_t w3 = fern_result_unwrap(restarted);\n"
        "    if (w3 <= w2) return 180;\n"
        "\n"
        "    int64_t third = fern_actor_exit(w3, \"boom3\");\n"
        "    if (!fern_result_is_ok(third)) return 181;\n"
        "    int64_t w4 = fern_result_unwrap(third);\n"
        "    if (w4 <= w3) return 182;\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actor_supervise_one_for_all_restarts_all_children_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include <stdio.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t supervisor = fern_actor_spawn(\"supervisor\");\n"
        "    int64_t a = fern_actor_spawn(\"a\");\n"
        "    int64_t b = fern_actor_spawn(\"b\");\n"
        "    if (supervisor <= 0 || a <= supervisor || b <= a) return 190;\n"
        "\n"
        "    if (!fern_result_is_ok(fern_actor_supervise_one_for_all(supervisor, a, 3, 60))) return 191;\n"
        "    if (!fern_result_is_ok(fern_actor_supervise_one_for_all(supervisor, b, 3, 60))) return 192;\n"
        "\n"
        "    int64_t exited = fern_actor_exit(a, \"boom\");\n"
        "    if (!fern_result_is_ok(exited)) return 193;\n"
        "\n"
        "    int64_t m1 = fern_actor_next(supervisor);\n"
        "    int64_t m2 = fern_actor_next(supervisor);\n"
        "    int64_t m3 = fern_actor_next(supervisor);\n"
        "    int64_t m4 = fern_actor_next(supervisor);\n"
        "    if (!fern_result_is_ok(m1) || !fern_result_is_ok(m2) || !fern_result_is_ok(m3) || !fern_result_is_ok(m4)) return 194;\n"
        "\n"
        "    const char* s1 = (const char*)(intptr_t)fern_result_unwrap(m1);\n"
        "    const char* s2 = (const char*)(intptr_t)fern_result_unwrap(m2);\n"
        "    const char* s3 = (const char*)(intptr_t)fern_result_unwrap(m3);\n"
        "    const char* s4 = (const char*)(intptr_t)fern_result_unwrap(m4);\n"
        "    if (!fern_str_starts_with(s1, \"DOWN(\")) return 195;\n"
        "    if (!fern_str_starts_with(s2, \"DOWN(\")) return 196;\n"
        "    if (!fern_str_starts_with(s3, \"RESTART(\")) return 197;\n"
        "    if (!fern_str_starts_with(s4, \"RESTART(\")) return 198;\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actor_supervise_rest_for_one_restarts_suffix_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t supervisor = fern_actor_spawn(\"supervisor\");\n"
        "    int64_t a = fern_actor_spawn(\"a\");\n"
        "    int64_t b = fern_actor_spawn(\"b\");\n"
        "    int64_t c = fern_actor_spawn(\"c\");\n"
        "    if (supervisor <= 0 || a <= supervisor || b <= a || c <= b) return 210;\n"
        "\n"
        "    if (!fern_result_is_ok(fern_actor_supervise_rest_for_one(supervisor, a, 3, 60))) return 211;\n"
        "    if (!fern_result_is_ok(fern_actor_supervise_rest_for_one(supervisor, b, 3, 60))) return 212;\n"
        "    if (!fern_result_is_ok(fern_actor_supervise_rest_for_one(supervisor, c, 3, 60))) return 213;\n"
        "\n"
        "    int64_t exited = fern_actor_exit(b, \"boom\");\n"
        "    if (!fern_result_is_ok(exited)) return 214;\n"
        "\n"
        "    int64_t m1 = fern_actor_next(supervisor);\n"
        "    int64_t m2 = fern_actor_next(supervisor);\n"
        "    int64_t m3 = fern_actor_next(supervisor);\n"
        "    int64_t m4 = fern_actor_next(supervisor);\n"
        "    if (!fern_result_is_ok(m1) || !fern_result_is_ok(m2) || !fern_result_is_ok(m3) || !fern_result_is_ok(m4)) return 215;\n"
        "\n"
        "    const char* s1 = (const char*)(intptr_t)fern_result_unwrap(m1);\n"
        "    const char* s2 = (const char*)(intptr_t)fern_result_unwrap(m2);\n"
        "    const char* s3 = (const char*)(intptr_t)fern_result_unwrap(m3);\n"
        "    const char* s4 = (const char*)(intptr_t)fern_result_unwrap(m4);\n"
        "    if (!fern_str_starts_with(s1, \"DOWN(\")) return 216;\n"
        "    if (!fern_str_starts_with(s2, \"DOWN(\")) return 217;\n"
        "    if (!fern_str_starts_with(s3, \"RESTART(\")) return 218;\n"
        "    if (!fern_str_starts_with(s4, \"RESTART(\")) return 219;\n"
        "\n"
        "    int64_t ping = fern_actor_send(a, \"ping\");\n"
        "    if (!fern_result_is_ok(ping)) return 220;\n"
        "    int64_t recv = fern_actor_receive(a);\n"
        "    if (!fern_result_is_ok(recv)) return 221;\n"
        "\n"
        "    int64_t extra = fern_actor_next(supervisor);\n"
        "    if (fern_result_is_ok(extra)) return 222;\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_actor_exit_marks_actor_dead_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t worker = fern_actor_spawn(\"worker\");\n"
        "    if (worker <= 0) return 130;\n"
        "\n"
        "    int64_t sent = fern_actor_send(worker, \"before-exit\");\n"
        "    if (!fern_result_is_ok(sent)) return 131;\n"
        "\n"
        "    int64_t exited = fern_actor_exit(worker, \"shutdown\");\n"
        "    if (!fern_result_is_ok(exited)) return 132;\n"
        "\n"
        "    if (fern_actor_mailbox_len(worker) != -1) return 133;\n"
        "    if (fern_result_is_ok(fern_actor_send(worker, \"after-exit\"))) return 134;\n"
        "    if (fern_result_is_ok(fern_actor_receive(worker))) return 135;\n"
        "    if (fern_actor_scheduler_next() != 0) return 136;\n"
        "    int64_t restarted = fern_actor_restart(worker);\n"
        "    if (!fern_result_is_ok(restarted)) return 137;\n"
        "    int64_t next_worker = fern_result_unwrap(restarted);\n"
        "    if (next_worker <= worker) return 138;\n"
        "    int64_t ok_send = fern_actor_send(next_worker, \"hello\");\n"
        "    if (!fern_result_is_ok(ok_send)) return 139;\n"
        "    int64_t recv = fern_actor_receive(next_worker);\n"
        "    if (!fern_result_is_ok(recv)) return 140;\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_memory_alloc_dup_drop_contract(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t* values = (int64_t*)fern_alloc(sizeof(int64_t) * 2);\n"
        "    if (values == NULL) return 10;\n"
        "\n"
        "    values[0] = 41;\n"
        "    values[1] = 0;\n"
        "\n"
        "    if (fern_dup(NULL) != NULL) return 11;\n"
        "    fern_drop(NULL);\n"
        "\n"
        "    int64_t* alias = (int64_t*)fern_dup(values);\n"
        "    if (alias != values) return 12;\n"
        "    alias[1] = 1;\n"
        "    if (values[0] + values[1] != 42) return 13;\n"
        "\n"
        "    fern_drop(alias);\n"
        "    fern_drop(values);\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_rc_header_and_core_type_ops(void) {
    BuildRunResult result = build_and_run_c_source(
        "#include <stdint.h>\n"
        "#include \"fern_runtime.h\"\n"
        "\n"
        "int fern_main(void) {\n"
        "    int64_t* payload = (int64_t*)fern_rc_alloc(sizeof(int64_t), FERN_RC_TYPE_TUPLE);\n"
        "    if (payload == NULL) return 20;\n"
        "    if (fern_rc_refcount(payload) != 1) return 21;\n"
        "    if (fern_rc_type_tag(payload) != FERN_RC_TYPE_TUPLE) return 22;\n"
        "\n"
        "    fern_rc_set_flags(payload, FERN_RC_FLAG_UNIQUE);\n"
        "    if (fern_rc_flags(payload) != FERN_RC_FLAG_UNIQUE) return 23;\n"
        "\n"
        "    if (fern_rc_dup(NULL) != NULL) return 24;\n"
        "    fern_rc_drop(NULL);\n"
        "\n"
        "    int64_t* alias = (int64_t*)fern_rc_dup(payload);\n"
        "    if (alias != payload) return 25;\n"
        "    if (fern_rc_refcount(payload) != 2) return 26;\n"
        "\n"
        "    fern_rc_drop(alias);\n"
        "    if (fern_rc_refcount(payload) != 1) return 27;\n"
        "    fern_rc_drop(payload);\n"
        "    if (fern_rc_refcount(payload) != 0) return 28;\n"
        "\n"
        "    int64_t result_ptr = fern_result_ok(42);\n"
        "    if (fern_rc_type_tag((void*)(intptr_t)result_ptr) != FERN_RC_TYPE_RESULT) return 29;\n"
        "\n"
        "    FernList* list = fern_list_new();\n"
        "    if (list == NULL) return 30;\n"
        "    if (fern_rc_type_tag(list) != FERN_RC_TYPE_LIST) return 31;\n"
        "    if (fern_rc_refcount(list) != 1) return 32;\n"
        "\n"
        "    return 0;\n"
        "}\n");

    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);

    free_build_run_result(&result);
}

void test_runtime_rejects_unimplemented_actor_execution(void) {
    const char* sources[] = {
        "fn worker() -> Int: 0\nfn main() -> Int: spawn(worker)\n",
        "fn worker() -> Int: 0\nfn main() -> Int: spawn_link(worker)\n",
        "fn main() -> Int:\n    receive:\n        _ -> 0\n"
    };
    for (size_t i = 0; i < sizeof(sources) / sizeof(sources[0]); i++) {
        BuildRunResult result = build_and_run_source(sources[i]);
        ASSERT_NE(result.build.exit_code, 0);
        ASSERT_NOT_NULL(result.build.output);
        ASSERT_TRUE(strstr(result.build.output, "actor execution is not implemented") != NULL);
        ASSERT_TRUE(strstr(result.build.output, "actors.start/post/next") != NULL);
        free_build_run_result(&result);
        char* source_path = write_tmp_source(sources[i]);
        ASSERT_NOT_NULL(source_path);
        const char* commands[] = {"check", "run", "emit"};
        int failures = 0;
        for (size_t j = 0; j < sizeof(commands) / sizeof(commands[0]); j++) {
            char cmd[1024];
            snprintf(cmd, sizeof(cmd), "./bin/fern-c %s %s 2>&1", commands[j], source_path);
            CmdResult command = run_cmd(cmd);
            if (j == 0) {
                if (command.exit_code != 0) failures++;
            } else if (command.exit_code == 0 || command.output == NULL ||
                strstr(command.output, "actor execution is not implemented") == NULL) {
                failures++;
            }
            free_cmd_result(&command);
        }
        unlink(source_path);
        free(source_path);
        ASSERT_EQ(failures, 0);
    }
}

void test_runtime_send_primitive_preserves_error_result(void) {
    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    match send(2147483647, \"hello\"):\n"
        "        Ok(_) -> 1\n"
        "        Err(_) -> 0\n");
    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);
    free_build_run_result(&result);
}

void test_runtime_send_primitive_preserves_success_result(void) {
    BuildRunResult result = build_and_run_source(
        "fn main() -> Int:\n"
        "    let pid = actors.start(\"worker\")\n"
        "    match send(pid, \"hello\"):\n"
        "        Ok(status) ->\n"
        "            if status == 0:\n"
        "                match actors.next(pid):\n"
        "                    Ok(message) -> if String.eq(message, \"hello\"): 0 else: 2\n"
        "                    Err(_) -> 3\n"
        "            else: 4\n"
        "        Err(_) -> 5\n");
    ASSERT_EQ(result.build.exit_code, 0);
    ASSERT_EQ(result.run.exit_code, 0);
    free_build_run_result(&result);
}

/* Replay fresh-process lifecycle cases and seeded failures against the runtime archive. */
void test_runtime_actor_seeded_lifecycle_invariants(void) {
    char* output_path = make_tmp_output_path();
    ASSERT_NOT_NULL(output_path);
    char cmd[2048];
    snprintf(cmd, sizeof(cmd),
        "cc -std=c11 -Wall -Wextra -Werror -Iruntime -Iinclude "
        "tests/fixtures/runtime_actor_scenarios.c lib/fernsim.c lib/arena.c "
        "bin/libfern_runtime.a "
        "$(pkg-config --libs bdw-gc 2>/dev/null || echo -lgc) "
        "$(pkg-config --libs sqlite3 2>/dev/null || echo -lsqlite3) "
        "$(pkg-config --libs openssl 2>/dev/null || echo -lssl -lcrypto) "
        "-pthread -o %s 2>&1", output_path);
    CmdResult build = run_cmd(cmd);
    if (build.exit_code != 0 && build.output != NULL) {
        fprintf(stderr, "%s", build.output);
    }
    int build_status = build.exit_code;
    free_cmd_result(&build);
    if (build_status != 0) {
        unlink(output_path);
        free(output_path);
        ASSERT_EQ(build_status, 0);
    }
    const char* scenarios[] = {
        "time-zero", "single-replacement", "forest", "invalid-pid",
        "terminated-sibling", "simulation"
    };
    int failures = 0;
    for (size_t i = 0; i < sizeof(scenarios) / sizeof(scenarios[0]); i++) {
        snprintf(cmd, sizeof(cmd), "FERN_ACTOR_SCENARIO=%s %s 2>&1", scenarios[i], output_path);
        CmdResult run = run_cmd(cmd);
        if (run.exit_code != 0) {
            fprintf(stderr, "actor scenario %s failed: %s\n", scenarios[i],
                run.output != NULL ? run.output : "no output");
            failures++;
        }
        free_cmd_result(&run);
    }
    unlink(output_path);
    free(output_path);
    ASSERT_EQ(failures, 0);
}

void run_runtime_surface_tests(void) {
    printf("\n=== Runtime Surface Tests ===\n");
    TEST_RUN(test_runtime_json_parse_empty_returns_err_code);
    TEST_RUN(test_runtime_json_stringify_empty_returns_ok_empty);
    TEST_RUN(test_runtime_http_get_empty_returns_io_error);
    TEST_RUN(test_runtime_http_get_returns_ok_body);
    TEST_RUN(test_runtime_http_post_returns_ok_body);
    TEST_RUN(test_runtime_http_get_https_returns_ok_body);
    TEST_RUN(test_runtime_http_post_https_returns_ok_body);
    TEST_RUN(test_runtime_http_post_invalid_url_returns_io_error);
    TEST_RUN(test_runtime_sql_open_empty_returns_io_error);
    TEST_RUN(test_runtime_sql_open_and_execute_returns_ok);
    TEST_RUN(test_runtime_sql_execute_invalid_handle_returns_io_error);
    TEST_RUN(test_runtime_actors_start_returns_monotonic_ids);
    TEST_RUN(test_runtime_actors_post_and_next_mailbox_contract);
    TEST_RUN(test_runtime_actor_scheduler_round_robin_contract);
    TEST_RUN(test_runtime_actor_spawn_link_exit_notification_contract);
    TEST_RUN(test_runtime_actor_spawn_link_requires_current_actor_contract);
    TEST_RUN(test_runtime_actor_monitor_and_restart_contract);
    TEST_RUN(test_runtime_actor_demonitor_stops_down_notifications_contract);
    TEST_RUN(test_runtime_actor_supervise_restart_intensity_contract);
    TEST_RUN(test_runtime_actor_supervise_normal_exit_does_not_restart_contract);
    TEST_RUN(test_runtime_actor_supervision_uses_deterministic_clock_contract);
    TEST_RUN(test_runtime_actor_supervise_one_for_all_restarts_all_children_contract);
    TEST_RUN(test_runtime_actor_supervise_rest_for_one_restarts_suffix_contract);
    TEST_RUN(test_runtime_actor_exit_marks_actor_dead_contract);
    TEST_RUN(test_runtime_actor_seeded_lifecycle_invariants);
    TEST_RUN(test_runtime_rejects_unimplemented_actor_execution);
    TEST_RUN(test_runtime_send_primitive_preserves_error_result);
    TEST_RUN(test_runtime_send_primitive_preserves_success_result);
    TEST_RUN(test_runtime_memory_alloc_dup_drop_contract);
    TEST_RUN(test_runtime_rc_header_and_core_type_ops);
}
