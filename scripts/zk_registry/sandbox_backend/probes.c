/* Small bounded negative payloads, always executed inside the production boundary. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/io_uring.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

static int denied(void) {
    int error = errno;
    printf("{\"blocked\":true,\"errno\":%d}\n", error);
    return error == EPERM || error == EACCES || error == ENETUNREACH || error == EHOSTUNREACH || error == ENOENT ? 0 : 2;
}

static int connect_socket(int fd, int family, const char *address, int port) {
    struct sockaddr_in v4 = {.sin_family = AF_INET, .sin_port = htons((unsigned short)port)};
    struct sockaddr_in6 v6 = {.sin6_family = AF_INET6, .sin6_port = htons((unsigned short)port)};
    if (family == AF_INET) {
        if (inet_pton(AF_INET, address, &v4.sin_addr) != 1) return 2;
        if (connect(fd, (struct sockaddr *)&v4, sizeof(v4)) < 0) return denied();
    } else {
        if (inet_pton(AF_INET6, address, &v6.sin6_addr) != 1) return 2;
        if (connect(fd, (struct sockaddr *)&v6, sizeof(v6)) < 0) return denied();
    }
    puts("{\"blocked\":false}");
    return 1;
}

static int network(const char *kind, const char *address, int port) {
    int family = strchr(address, ':') ? AF_INET6 : AF_INET;
    int fd = socket(!strcmp(kind, "raw") ? AF_PACKET : family,
                    !strcmp(kind, "udp") ? SOCK_DGRAM : (!strcmp(kind, "raw") ? SOCK_RAW : SOCK_STREAM), 0);
    if (fd < 0) return denied();
    if (!strcmp(kind, "raw")) { close(fd); puts("{\"blocked\":false}"); return 1; }
    if (!strcmp(kind, "udp")) {
        struct sockaddr_in remote = {.sin_family = AF_INET, .sin_port = htons((unsigned short)port)};
        if (inet_pton(AF_INET, address, &remote.sin_addr) != 1) return 2;
        ssize_t sent = sendto(fd, "AA-UDP", 6, 0, (struct sockaddr *)&remote, sizeof(remote));
        if (sent < 0) return denied();
        printf("{\"blocked\":false,\"sent_bytes\":%zd}\n", sent);
        return 1;
    }
    return connect_socket(fd, family, address, port);
}

static int uring(const char *address, int port) {
    struct io_uring_params parameters = {0};
    int ring_fd = (int)syscall(SYS_io_uring_setup, 2, &parameters);
    if (ring_fd < 0) return denied();
    size_t sq_size = parameters.sq_off.array + parameters.sq_entries * sizeof(unsigned);
    size_t cq_size = parameters.cq_off.cqes + parameters.cq_entries * sizeof(struct io_uring_cqe);
    if (parameters.features & IORING_FEAT_SINGLE_MMAP) sq_size = cq_size = sq_size > cq_size ? sq_size : cq_size;
    void *sq = mmap(NULL, sq_size, PROT_READ | PROT_WRITE, MAP_SHARED, ring_fd, IORING_OFF_SQ_RING);
    void *cq = parameters.features & IORING_FEAT_SINGLE_MMAP ? sq :
        mmap(NULL, cq_size, PROT_READ | PROT_WRITE, MAP_SHARED, ring_fd, IORING_OFF_CQ_RING);
    struct io_uring_sqe *entries = mmap(NULL, parameters.sq_entries * sizeof(*entries),
                                      PROT_READ | PROT_WRITE, MAP_SHARED, ring_fd, IORING_OFF_SQES);
    if (sq == MAP_FAILED || cq == MAP_FAILED || entries == MAP_FAILED) return 2;
    unsigned *tail = (unsigned *)((char *)sq + parameters.sq_off.tail);
    unsigned *mask = (unsigned *)((char *)sq + parameters.sq_off.ring_mask);
    unsigned *array = (unsigned *)((char *)sq + parameters.sq_off.array);
    entries[0] = (struct io_uring_sqe){.opcode = IORING_OP_SOCKET, .fd = AF_INET, .off = SOCK_STREAM};
    array[*tail & *mask] = 0;
    __atomic_store_n(tail, *tail + 1, __ATOMIC_RELEASE);
    if (syscall(SYS_io_uring_enter, ring_fd, 1, 1, IORING_ENTER_GETEVENTS, NULL, 0) < 0) return denied();
    unsigned head = *(unsigned *)((char *)cq + parameters.cq_off.head);
    unsigned cq_mask = *(unsigned *)((char *)cq + parameters.cq_off.ring_mask);
    struct io_uring_cqe *results = (struct io_uring_cqe *)((char *)cq + parameters.cq_off.cqes);
    int socket_fd = results[head & cq_mask].res;
    if (socket_fd < 0) { errno = -socket_fd; return denied(); }
    return connect_socket(socket_fd, AF_INET, address, port);
}

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    if (!strcmp(argv[1], "tcp") || !strcmp(argv[1], "udp") || !strcmp(argv[1], "raw"))
        return argc == 4 ? network(argv[1], argv[2], atoi(argv[3])) : 2;
    if (!strcmp(argv[1], "uring")) return argc == 4 ? uring(argv[2], atoi(argv[3])) : 2;
    if (!strcmp(argv[1], "write") || !strcmp(argv[1], "read")) {
        if (argc != 3) return 2;
        int fd = open(argv[2], !strcmp(argv[1], "write") ? O_WRONLY | O_CREAT : O_RDONLY, 0600);
        if (fd < 0) return denied();
        close(fd);
        puts("{\"blocked\":false}");
        return 1;
    }
    if (!strcmp(argv[1], "detached") || !strcmp(argv[1], "double-fork")) {
        pid_t child = fork();
        if (child < 0) return 2;
        if (!child) {
            if (setsid() < 0) _exit(2);
            if (!strcmp(argv[1], "double-fork")) {
                pid_t grandchild = fork();
                if (grandchild < 0) _exit(2);
                _exit(0);
            }
            close(1);
            close(2);
            sleep(30);
            _exit(0);
        }
        if (!strcmp(argv[1], "double-fork")) { waitpid(child, NULL, 0); usleep(30000); }
        return 0;
    }
    if (!strcmp(argv[1], "fork-bomb")) {
        for (int attempt = 0; attempt < 1024; ++attempt) {
            pid_t child = fork();
            if (child < 0) { puts("{\"pids_limit_observed\":true}"); return errno == EAGAIN ? 0 : 2; }
            if (!child) { close(1); close(2); sleep(30); _exit(0); }
        }
        return 1;
    }
    if (!strcmp(argv[1], "memory-hog")) {
        for (int attempt = 0; attempt < 4096; ++attempt) {
            volatile unsigned char *memory = malloc(1024 * 1024);
            if (!memory) return 2;
            for (int page = 0; page < 1024 * 1024; page += 4096) memory[page] = 1;
        }
        return 1;
    }
    return 2;
}
