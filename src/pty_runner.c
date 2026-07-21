/*
 * pty_runner.c — запускает sideloader-cli внутри псевдотерминала (PTY),
 * автоматически подставляет email/пароль в интерактивные запросы и
 * пробрасывает 2FA-код через файл-маячок (beacon), который читает GUI.
 *
 * Причина существования: sideloader читает пароль через getpass(3), а
 * getpass() открывает /dev/tty напрямую и игнорирует stdin-пайп. Поэтому
 * обычный подход "пишем в stdin дочернего процесса" не работает — нужен
 * настоящий терминал, который даёт PTY.
 *
 * Компилируется через build.rs (crate `cc`) и линкуется в бинарь Rust.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <signal.h>
#include <sys/wait.h>
#include <sys/select.h>
#include <sys/stat.h>
#include <pty.h>      /* openpty (glibc) */
#include <utmp.h>
#include <termios.h>
#include <ctype.h>

typedef void (*line_cb_t)(const char *line, void *ctx);

#define LINEBUF_MAX 4096
#define WAIT_2FA_MARKER "__AVERSTOR_WAIT_2FA__"

/* приводим строку к нижнему регистру во временный буфер для поиска ключевых слов */
static void to_lower_copy(const char *src, char *dst, size_t dst_size) {
    size_t i = 0;
    for (; src[i] != '\0' && i + 1 < dst_size; i++) {
        dst[i] = (char)tolower((unsigned char)src[i]);
    }
    dst[i] = '\0';
}

/* Убирает ANSI escape-последовательности вида ESC[...m из строки */
static void strip_ansi(const char *src, char *dst, size_t dst_size) {
    size_t d = 0;
    for (size_t i = 0; src[i] != '\0' && d + 1 < dst_size; i++) {
        if ((unsigned char)src[i] == 0x1b && src[i + 1] == '[') {
            i += 2;
            while (src[i] != '\0' && !isalpha((unsigned char)src[i])) i++;
        } else {
            dst[d++] = src[i];
        }
    }
    dst[d] = '\0';
}

static int write_all(int fd, const char *buf, size_t len) {
    size_t off = 0;
    while (off < len) {
        ssize_t n = write(fd, buf + off, len - off);
        if (n < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        off += (size_t)n;
    }
    return 0;
}

static int send_line(int master_fd, const char *text) {
    if (write_all(master_fd, text, strlen(text)) < 0) return -1;
    return write_all(master_fd, "\n", 1);
}

/* Ждём появления непустого beacon-файла с кодом 2FA (пишет его Rust-поток).
 * Опрос раз в 300мс, таймаут ~5 минут — после этого сдаёмся и возвращаем -1. */
static int wait_for_2fa_code(const char *beacon_path, char *out, size_t out_size) {
    const int poll_ms = 300;
    const int timeout_ms = 5 * 60 * 1000;
    int waited = 0;

    while (waited < timeout_ms) {
        struct stat st;
        if (stat(beacon_path, &st) == 0 && st.st_size > 0) {
            FILE *f = fopen(beacon_path, "r");
            if (f) {
                size_t n = fread(out, 1, out_size - 1, f);
                fclose(f);
                out[n] = '\0';
                /* обрезаем перевод строки/пробелы по краям */
                while (n > 0 && (out[n - 1] == '\n' || out[n - 1] == '\r' || out[n - 1] == ' ')) {
                    out[--n] = '\0';
                }
                if (n > 0) {
                    unlink(beacon_path);
                    return 0;
                }
            }
        }
        usleep(poll_ms * 1000);
        waited += poll_ms;
    }
    return -1;
}

int pty_run(
    const char *email,
    const char *password,
    const char *beacon_path,
    const char *bin_path,
    const char *const *args,
    line_cb_t line_cb,
    void *ctx
) {
    int master_fd = -1;
    pid_t pid;
    struct winsize ws;
    memset(&ws, 0, sizeof(ws));
    ws.ws_row = 40;
    ws.ws_col = 120;

    pid = forkpty(&master_fd, NULL, NULL, &ws);
    if (pid < 0) {
        return -1;
    }

    if (pid == 0) {
        /* ── дочерний процесс: запускаем sideloader внутри слейв-стороны PTY ── */
        execv(bin_path, (char *const *)args);
        /* если execv вернулся — это ошибка */
        _exit(127);
    }

    /* ── родитель ── */
    int email_sent = 0;
    int password_sent = 0;
    int waiting_2fa = 0;

    char linebuf[LINEBUF_MAX];
    size_t linelen = 0;
    char lowbuf[LINEBUF_MAX];

    for (;;) {
        fd_set rfds;
        FD_ZERO(&rfds);
        FD_SET(master_fd, &rfds);
        struct timeval tv = { .tv_sec = 0, .tv_usec = 200000 }; /* 200мс */

        int sel = select(master_fd + 1, &rfds, NULL, NULL, &tv);
        if (sel < 0) {
            if (errno == EINTR) continue;
            break;
        }

        /* Проверяем, не завершился ли уже дочерний процесс */
        int status = 0;
        pid_t w = waitpid(pid, &status, WNOHANG);
        if (w == pid) {
            /* дочитываем то, что осталось в буфере PTY, если есть */
            char tmp[LINEBUF_MAX];
            ssize_t n;
            while ((n = read(master_fd, tmp, sizeof(tmp) - 1)) > 0) {
                tmp[n] = '\0';
                line_cb(tmp, ctx);
            }
            close(master_fd);
            if (WIFEXITED(status)) return WEXITSTATUS(status);
            return -1;
        }

        if (sel == 0) continue; /* таймаут select, идём проверять процесс заново */

        char chunk[1024];
        ssize_t n = read(master_fd, chunk, sizeof(chunk));
        if (n <= 0) {
            if (n < 0 && errno == EINTR) continue;
            continue; /* PTY может временно не иметь данных, процесс ещё жив */
        }

        for (ssize_t i = 0; i < n; i++) {
            char c = chunk[i];
            if (c == '\n' || c == '\r') {
                if (linelen == 0) continue;
                linebuf[linelen] = '\0';

                to_lower_copy(linebuf, lowbuf, sizeof(lowbuf));

                /* Сообщаем в GUI обычную строку вывода */
                line_cb(linebuf, ctx);

                /* Запрос email/Apple ID */
                if (!email_sent && (strstr(lowbuf, "apple id") || strstr(lowbuf, "email"))) {
                    send_line(master_fd, email);
                    email_sent = 1;
                }
                /* Запрос пароля */
                else if (!password_sent && strstr(lowbuf, "password")) {
                    send_line(master_fd, password);
                    password_sent = 1;
                }
                /* Запрос 2FA-кода */
                else if (!waiting_2fa &&
                         (strstr(lowbuf, "2fa") || strstr(lowbuf, "verification code") ||
                          strstr(lowbuf, "two-factor") || strstr(lowbuf, "code:"))) {
                    waiting_2fa = 1;
                    line_cb(WAIT_2FA_MARKER, ctx);

                    char code[64];
                    if (wait_for_2fa_code(beacon_path, code, sizeof(code)) == 0) {
                        send_line(master_fd, code);
                    } else {
                        line_cb("[timeout waiting for 2FA code]", ctx);
                    }
                    waiting_2fa = 0;
                }

                linelen = 0;
            } else {
                if (linelen + 1 < sizeof(linebuf)) {
                    linebuf[linelen++] = c;
                }
            }
        }

        /* ── Проверяем частичный буфер (промпт без \n) ──────────────────
         * getpass() выводит "Password: " без перевода строки и ждёт ввода.
         * select() снова не сработает пока мы не ответим, поэтому проверяем
         * накопленный буфер после каждого чтения из PTY. */
        if (linelen > 0) {
            char stripped_p[LINEBUF_MAX];
            char partial[LINEBUF_MAX];
            linebuf[linelen] = '\0';
            strip_ansi(linebuf, stripped_p, sizeof(stripped_p));
            to_lower_copy(stripped_p, partial, sizeof(partial));

            if (!email_sent && (strstr(partial, "apple id") || strstr(partial, "email") ||
                                strstr(partial, "informations") || strstr(partial, "account"))) {
                send_line(master_fd, email);
                email_sent = 1;
                linelen = 0;
            } else if (!password_sent && strstr(partial, "password")) {
                send_line(master_fd, password);
                password_sent = 1;
                linelen = 0;
            }
        }
    }

    /* сюда доходим только если select сломался — подчищаем процесс */
    kill(pid, SIGKILL);
    int status = 0;
    waitpid(pid, &status, 0);
    close(master_fd);
    return -1;
}
