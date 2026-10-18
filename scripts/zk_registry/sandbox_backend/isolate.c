/* Trusted PID-1 observer and ABI-adaptive, Landrun-CLI-compatible Landlock launcher. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/landlock.h>
#include <linux/capability.h>
#include <linux/securebits.h>
#include <net/if.h>
#include <signal.h>
#include <sched.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/prctl.h>
#include <sys/random.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define FS_IOCTL (1ULL << 15)
#define FS_UNIX (1ULL << 16)
#define RESTRICT_TSYNC (1U << 3)
#define FILE_RIGHTS ((1ULL << 0) | (1ULL << 1) | (1ULL << 2) | (1ULL << 14) | FS_IOCTL | FS_UNIX)
#define READ_RIGHTS ((1ULL << 0) | (1ULL << 2) | (1ULL << 3) | FS_UNIX)
extern char **environ;
static int setup_fd = -1;

/* execvpe silently retries ENOEXEC with /bin/sh. Preserve the actual exec errno. */
static void exec_no_shell(char **argv) {
    if (strchr(argv[0], '/')) {
        execve(argv[0], argv, environ);
        return;
    }
    const char *path = getenv("PATH");
    char *copy = strdup(path ? path : "/usr/bin:/bin");
    if (!copy) return;
    int denied = 0;
    char *remaining = copy;
    char *directory;
    while ((directory = strsep(&remaining, ":")) != NULL) {
        char *candidate;
        if (asprintf(&candidate, "%s/%s", *directory ? directory : ".", argv[0]) < 0) {
            free(copy);
            errno = ENOMEM;
            return;
        }
        execve(candidate, argv, environ);
        int error = errno;
        free(candidate);
        if (error == EACCES) denied = 1;
        else if (error != ENOENT && error != ENOTDIR) {
            free(copy);
            errno = error;
            return;
        }
    }
    free(copy);
    errno = denied ? EACCES : ENOENT;
}

static void event(int fd, const char *format, ...) {
    char buffer[4096];
    va_list arguments;
    va_start(arguments, format);
    int length = vsnprintf(buffer, sizeof(buffer), format, arguments);
    va_end(arguments);
    if (length < 0 || (size_t)length >= sizeof(buffer)
            || write(fd, buffer, (size_t)length) != length) {
        fprintf(stderr, "observer-event-write-failed\n");
        exit(125); /* A missing final event is an outer structured availability failure. */
    }
}

static void unavailable(const char *stage) {
    if (setup_fd >= 0) event(setup_fd, "{\"setup_error\":\"%s\",\"errno\":%d}\n", stage, errno);
    fprintf(stderr, "BOOLE_SANDBOX_UNAVAILABLE:%s:%s\n", stage, strerror(errno));
    exit(125);
}

static int abi(void) {
    return (int)syscall(__NR_landlock_create_ruleset, NULL, 0, LANDLOCK_CREATE_RULESET_VERSION);
}

static void write_control(int fd, const char *text) {
    if (lseek(fd, 0, SEEK_SET) < 0 || write(fd, text, strlen(text)) != (ssize_t)strlen(text))
        unavailable("cgroup-write");
}

static size_t read_control(int fd, char *buffer, size_t length) {
    if (lseek(fd, 0, SEEK_SET) < 0) unavailable("cgroup-seek");
    ssize_t count = read(fd, buffer, length - 1);
    if (count < 0) unavailable("cgroup-read");
    buffer[count] = 0;
    return (size_t)count;
}

static long counter(int fd, const char *name) {
    char buffer[4096];
    read_control(fd, buffer, sizeof(buffer));
    char *found = strstr(buffer, name);
    return found ? strtol(found + strlen(name), NULL, 10) : 0;
}

static void text_file(const char *path, char *buffer, size_t size) {
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) unavailable(path);
    ssize_t count = read(fd, buffer, size - 1);
    if (count < 0) unavailable(path);
    buffer[count] = 0;
    buffer[strcspn(buffer, "\n")] = 0;
    close(fd);
}

static void namespace_name(const char *kind, char *buffer, size_t size) {
    char path[80];
    snprintf(path, sizeof(path), "/proc/self/ns/%s", kind);
    ssize_t count = readlink(path, buffer, size - 1);
    if (count < 0) unavailable("namespace-readlink");
    buffer[count] = 0;
}

static void rule(int ruleset, const char *path, uint64_t access) {
    int fd = open(path, O_PATH | O_CLOEXEC);
    struct stat metadata;
    if (fd < 0 || fstat(fd, &metadata) < 0) unavailable("landlock-path");
    if (!S_ISDIR(metadata.st_mode)) access &= FILE_RIGHTS;
    struct landlock_path_beneath_attr attr = {.allowed_access = access, .parent_fd = fd};
    if (syscall(__NR_landlock_add_rule, ruleset, LANDLOCK_RULE_PATH_BENEATH, &attr, 0) < 0)
        unavailable("landlock-rule");
    close(fd);
}

/* This runs after namespaces/seccomp/Landlock and never executes a work-root file on the host. */
static void exec_probe(const char *directory, int version, uint64_t fs) {
    struct stat metadata;
    if (lstat(directory, &metadata) < 0 || S_ISLNK(metadata.st_mode)) unavailable("work-root-exec-probe");
    if (!S_ISDIR(metadata.st_mode)) return;
    int root = open(directory, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    struct statvfs filesystem;
    if (root < 0 || fstatvfs(root, &filesystem) < 0) unavailable("work-root-exec-probe");
    if (filesystem.f_flag & ST_NOEXEC) {
        errno = EACCES;
        unavailable("work-root-exec-probe");
    }
    unsigned long long nonce;
    if (getrandom(&nonce, sizeof(nonce), 0) != (ssize_t)sizeof(nonce)) unavailable("work-root-exec-probe");
    char name[64];
    snprintf(name, sizeof(name), ".boole-exec-probe-%016llx", nonce);
    int output = openat(root, name, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0700);
    if (output < 0) unavailable("work-root-exec-probe");
    int input = open("/usr/bin/true", O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    int failed = input < 0 || fchmod(output, 0700) < 0;
    char buffer[16384];
    while (!failed) {
        ssize_t count = read(input, buffer, sizeof(buffer));
        if (count == 0) break;
        if (count < 0) {
            if (errno == EINTR) continue;
            failed = 1;
            break;
        }
        for (ssize_t offset = 0; offset < count;) {
            ssize_t written = write(output, buffer + offset, (size_t)(count - offset));
            if (written < 0 && errno == EINTR) continue;
            if (written <= 0) { failed = 1; break; }
            offset += written;
        }
    }
    if (input >= 0) close(input);
    if (close(output) < 0) failed = 1;
    char *program = NULL;
    if (!failed && asprintf(&program, "%s/%s", directory, name) < 0) failed = 1;
    if (!failed) {
        pid_t child = fork();
        if (child == 0) {
            /* A replaced probe cannot write trusted Comparator config/status either. */
            if (setup_fd >= 0) close(setup_fd);
            setup_fd = -1;
            close(root);
            int null = open("/dev/null", O_WRONLY | O_CLOEXEC);
            if (null < 0 || dup2(null, STDOUT_FILENO) < 0 || dup2(null, STDERR_FILENO) < 0) _exit(125);
            if (null > STDERR_FILENO) close(null);
            struct { uint64_t fs, net, scoped; } attr = {
                .fs = fs, .net = 3, .scoped = version >= 6 ? 3 : 0};
            int readonly = (int)syscall(__NR_landlock_create_ruleset, &attr, version >= 6 ? 24 : 16, 0);
            if (readonly < 0) _exit(125);
            rule(readonly, "/", fs & READ_RIGHTS);
            if (syscall(__NR_landlock_restrict_self, readonly, 0) < 0) _exit(125);
            close(readonly);
            char *arguments[] = {program, NULL};
            execve(program, arguments, environ);
            _exit(125);
        }
        if (child < 0) failed = 1;
        else {
            int status = 0;
            pid_t waited;
            do { waited = waitpid(child, &status, 0); } while (waited < 0 && errno == EINTR);
            if (waited != child || !WIFEXITED(status) || WEXITSTATUS(status) != 0) failed = 1;
        }
    }
    if (unlinkat(root, name, 0) < 0) failed = 1;
    close(root);
    free(program);
    if (failed) unavailable("work-root-exec-probe");
}

static int landlock(int argc, char **argv) {
    int version = abi();
    if (version < 4) unavailable("landlock-abi");
    uint64_t fs = (1ULL << 15) - 1;
    if (version >= 5) fs |= FS_IOCTL;
    if (version >= 9) fs |= FS_UNIX;
    struct { uint64_t fs, net, scoped; } attr = {.fs = fs, .net = 3, .scoped = version >= 6 ? 3 : 0};
    int ruleset = (int)syscall(__NR_landlock_create_ruleset, &attr, version >= 6 ? 24 : 16, 0);
    if (ruleset < 0) unavailable("landlock-create");
    const char **probe_roots = calloc((size_t)argc + 1, sizeof(char *));
    if (!probe_roots) unavailable("work-root-exec-probe");
    int probe_count = 0, enable_probes = 0;
    int index = 0;
    for (; index < argc; ++index) {
        if (!strcmp(argv[index], "--")) { ++index; break; }
        if (!strcmp(argv[index], "--exec-probes")) { enable_probes = 1; continue; }
        if (!strcmp(argv[index], "--add-exec") || !strcmp(argv[index], "-add-exec")
                || !strcmp(argv[index], "--ldd") || !strcmp(argv[index], "-ldd")
                || !strcmp(argv[index], "--best-effort")) continue; /* Compatibility only; ABI >=4 stays mandatory. */
        if (index + 1 >= argc) unavailable("landlock-arguments");
        const char *flag = argv[index++], *value = argv[index];
        if (!strcmp(flag, "--ro") || !strcmp(flag, "--rox")) rule(ruleset, value, fs & READ_RIGHTS);
        else if (!strcmp(flag, "--rw")) {
            rule(ruleset, value, fs & (READ_RIGHTS | FILE_RIGHTS));
            probe_roots[probe_count++] = value;
        } else if (!strcmp(flag, "--rwx")) {
            rule(ruleset, value, fs);
            probe_roots[probe_count++] = value;
        }
        else if (!strcmp(flag, "--env")) {
            char *equal = strchr(value, '=');
            if (!equal || equal == value) unavailable("landlock-env");
            *equal = 0;
            if (setenv(value, equal + 1, 1) < 0) unavailable("landlock-env");
        } else unavailable("landlock-arguments");
    }
    if (index >= argc) unavailable("landlock-command");
    /* --add-exec grants only this executable, never its parent directory. */
    if (argv[index][0] == '/') rule(ruleset, argv[index], fs & READ_RIGHTS);
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) < 0) unavailable("no-new-privileges");
    if (syscall(__NR_landlock_restrict_self, ruleset, version >= 8 ? RESTRICT_TSYNC : 0) < 0)
        unavailable("landlock-restrict");
    close(ruleset);
    if (enable_probes) for (int probe = 0; probe < probe_count; ++probe) exec_probe(probe_roots[probe], version, fs);
    free(probe_roots);
    exec_no_shell(argv + index);
    unavailable("landlock-exec");
    return 125;
}

static int init(int argc, char **argv) {
    if (getpid() != 1 || argc < 11 || strcmp(argv[9], "--")) unavailable("pid-1-arguments");
    int fds[8];
    for (int i = 0; i < 8; ++i) fds[i] = atoi(argv[i]);
    char profile[256], status[4096], userns[80], netns[80], pidns[80], mountns[80], ipcns[80];
    text_file("/proc/self/attr/current", profile, sizeof(profile));
    int file = open("/proc/self/status", O_RDONLY | O_CLOEXEC);
    if (file < 0) unavailable("seccomp-state");
    read_control(file, status, sizeof(status));
    close(file);
    char *mode = strstr(status, "Seccomp:");
    int seccomp = mode ? atoi(mode + strlen("Seccomp:")) : -1;
    namespace_name("user", userns, sizeof(userns));
    namespace_name("net", netns, sizeof(netns));
    namespace_name("pid", pidns, sizeof(pidns));
    namespace_name("mnt", mountns, sizeof(mountns));
    namespace_name("ipc", ipcns, sizeof(ipcns));
    int sock = socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    struct ifreq request = {0};
    strcpy(request.ifr_name, "lo");
    if (sock < 0 || ioctl(sock, SIOCGIFFLAGS, &request) < 0) unavailable("loopback-flags");
    int loopback_up = !!(request.ifr_flags & IFF_UP);
    if (atoi(argv[8]) && loopback_up) unavailable("loopback-still-up");
    close(sock);
    if (prctl(PR_SET_SECUREBITS, SECBIT_NOROOT | SECBIT_NOROOT_LOCKED
              | SECBIT_NO_SETUID_FIXUP | SECBIT_NO_SETUID_FIXUP_LOCKED, 0, 0, 0) < 0)
        unavailable("root-capability-lock");
    for (int capability = 0; capability < 64; ++capability)
        if (prctl(PR_CAPBSET_DROP, capability, 0, 0, 0) < 0 && errno != EINVAL)
            unavailable("bounding-capability-drop");
    struct __user_cap_header_struct header = {.version = _LINUX_CAPABILITY_VERSION_3, .pid = 0};
    struct __user_cap_data_struct capabilities[2] = {{0}, {0}};
    if (syscall(SYS_capset, &header, capabilities) < 0
            || prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0) < 0)
        unavailable("capability-drop");
    file = open("/proc/self/status", O_RDONLY | O_CLOEXEC);
    if (file < 0) unavailable("capability-readback");
    read_control(file, status, sizeof(status));
    close(file);
    unsigned long long effective = strtoull(strstr(status, "CapEff:") + 7, NULL, 16);
    unsigned long long ambient = strtoull(strstr(status, "CapAmb:") + 7, NULL, 16);
    unsigned long long bounding = strtoull(strstr(status, "CapBnd:") + 7, NULL, 16);
    if (prctl(PR_SET_DUMPABLE, 0, 0, 0, 0) < 0 || prctl(PR_GET_DUMPABLE, 0, 0, 0, 0) != 0)
        unavailable("private-observer-descriptors");
    cpu_set_t available, selected;
    CPU_ZERO(&selected);
    if (sched_getaffinity(0, sizeof(available), &available) < 0) unavailable("cpu-affinity-read");
    int cpu_count = 0;
    for (int cpu = 0; cpu < CPU_SETSIZE && cpu_count < 4; ++cpu)
        if (CPU_ISSET(cpu, &available)) { CPU_SET(cpu, &selected); ++cpu_count; }
    if (cpu_count != 4 || sched_setaffinity(0, sizeof(selected), &selected) < 0)
        unavailable("four-cpu-affinity");
    int setup_pipe[2];
    if (pipe2(setup_pipe, O_CLOEXEC | O_NONBLOCK) < 0) unavailable("setup-status-pipe");
    /* FD 3 becomes the setup channel: preserve a control handle before forking. */
    if (fds[0] == 3) {
        fds[0] = fcntl(fds[0], F_DUPFD_CLOEXEC, 4);
        if (fds[0] < 0) unavailable("cgroup-descriptor-copy");
    }
    pid_t main_child = fork();
    if (main_child < 0) unavailable("main-fork");
    if (!main_child) {
        int copied = dup3(setup_pipe[1], 3, O_CLOEXEC);
        setup_fd = copied < 0 ? setup_pipe[1] : 3;
        if (copied < 0) unavailable("setup-status-descriptor");
        char pid[32];
        snprintf(pid, sizeof(pid), "%d", getpid());
        write_control(fds[0], pid);
        if (prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0) < 0) unavailable("child-parent-death");
        if (syscall(SYS_close_range, 4u, ~0u, 0) < 0) unavailable("child-close-descriptors");
        if (argc < 13 || strcmp(argv[11], "landlock")) unavailable("main-command-shape");
        return landlock(argc - 12, argv + 12);
    }
    close(setup_pipe[1]);
    int main_status = 0, orphan_count = 0, reaped_status;
    pid_t reaped;
    for (;;) {
        reaped = waitpid(-1, &reaped_status, 0);
        if (reaped < 0 && errno == EINTR) continue;
        if (reaped < 0) unavailable("main-wait");
        if (reaped == main_child) { main_status = reaped_status; break; }
        ++orphan_count;
        if (orphan_count <= 256) event(fds[7], "{\"event\":\"orphan\",\"pid\":%d}\n", reaped);
    }
    write_control(fds[1], "1");
    struct timespec pause = {.tv_nsec = 1000000}, started, now;
    if (clock_gettime(CLOCK_MONOTONIC, &started) < 0) unavailable("freeze-clock");
    int frozen = 0;
    for (;;) {
        if (counter(fds[2], "frozen ") == 1) { frozen = 1; break; }
        if (clock_gettime(CLOCK_MONOTONIC, &now) < 0) unavailable("freeze-clock");
        if ((now.tv_sec - started.tv_sec) * 1000000000LL + now.tv_nsec - started.tv_nsec >= 2000000000LL)
            break;
        nanosleep(&pause, NULL);
        if (pause.tv_nsec < 20000000) pause.tv_nsec = pause.tv_nsec * 2 < 20000000 ? pause.tv_nsec * 2 : 20000000;
    }
    if (!frozen) unavailable("cgroup-freeze");
    char members[16384];
    read_control(fds[3], members, sizeof(members));
    int remaining = 0;
    for (char *line = strtok(members, "\n"); line; line = strtok(NULL, "\n")) {
        long member = strtol(line, NULL, 10);
        if (member > 0) {
            ++remaining;
            event(fds[7], "{\"event\":\"frozen-member\",\"pid\":%ld}\n", member);
        }
    }
    while ((reaped = waitpid(-1, &reaped_status, WNOHANG)) > 0)
        if (reaped != main_child) {
            ++orphan_count;
            if (orphan_count <= 256) event(fds[7], "{\"event\":\"orphan\",\"pid\":%d}\n", reaped);
        }
    long oom = counter(fds[4], "oom_kill "), pids = counter(fds[5], "max ");
    if (remaining) write_control(fds[6], "1");
    write_control(fds[1], "0");
    while ((reaped = waitpid(-1, &reaped_status, 0)) > 0 || (reaped < 0 && errno == EINTR)) {
        if (reaped > 0 && reaped != main_child) {
            ++orphan_count;
            if (orphan_count <= 256) event(fds[7], "{\"event\":\"orphan\",\"pid\":%d}\n", reaped);
        }
    }
    int code = WIFEXITED(main_status) ? WEXITSTATUS(main_status) : 128 + WTERMSIG(main_status);
    char setup[512] = {0};
    ssize_t setup_bytes = read(setup_pipe[0], setup, sizeof(setup) - 1);
    if (setup_bytes < 0) unavailable("setup-status-read");
    setup[strcspn(setup, "\n")] = 0;
    close(setup_pipe[0]);
    event(fds[7], "{\"event\":\"final\",\"main_exit\":%d,\"orphans_reaped\":%d,\"frozen_members\":%d,"
            "\"orphan_events_recorded\":%d,\"setup_failed\":%s,\"setup\":%s,\"cpu_affinity_count\":%d,"
            "\"oom_kill_events\":%ld,\"pids_max_events\":%ld,\"seccomp_mode\":%d,\"pid_1_dumpable\":%d,"
            "\"apparmor_label\":\"%s\",\"loopback_up\":%s,\"landlock_abi\":%d,"
            "\"caps_effective\":%llu,\"caps_ambient\":%llu,\"caps_bounding\":%llu,"
            "\"namespaces\":{\"user\":\"%s\",\"net\":\"%s\",\"pid\":\"%s\","
            "\"mnt\":\"%s\",\"ipc\":\"%s\"}}\n",
            code, orphan_count, remaining, orphan_count < 256 ? orphan_count : 256,
            setup_bytes ? "true" : "false", setup_bytes ? setup : "null", cpu_count, oom, pids, seccomp,
            prctl(PR_GET_DUMPABLE, 0, 0, 0, 0), profile,
            loopback_up ? "true" : "false", abi(), effective, ambient, bounding,
            userns, netns, pidns, mountns, ipcns);
    close(fds[7]);
    return code;
}

static int inner(int argc, char **argv) {
    const char *path = getenv("COMPARATOR_BOUNDARY_STATUS");
    if (!path || path[0] != '/') unavailable("inner-status-path");
    int status_fd = open(path, O_WRONLY | O_APPEND | O_NOFOLLOW | O_CLOEXEC);
    struct stat metadata;
    if (status_fd < 0 || fstat(status_fd, &metadata) < 0 || !S_ISREG(metadata.st_mode))
        unavailable("inner-status-open");
    setup_fd = status_fd;
    if (prctl(PR_SET_DUMPABLE, 0, 0, 0, 0) < 0) unavailable("inner-private-descriptors");
    int channel[2];
    if (pipe2(channel, O_CLOEXEC) < 0) unavailable("inner-status-pipe");
    pid_t child = fork();
    if (child < 0) unavailable("inner-fork");
    if (!child) {
        close(status_fd);
        close(channel[0]);
        setup_fd = channel[1];
        if (prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0) < 0) unavailable("inner-parent-death");
        return landlock(argc, argv);
    }
    close(channel[1]);
    char error[512] = {0};
    ssize_t count;
    do { count = read(channel[0], error, sizeof(error) - 1); } while (count < 0 && errno == EINTR);
    if (count < 0) unavailable("inner-status-read");
    close(channel[0]);
    int status;
    while (waitpid(child, &status, 0) < 0) if (errno != EINTR) unavailable("inner-wait");
    int code = WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
    if (count) {
        error[strcspn(error, "\n")] = 0;
        event(status_fd, "%s\n", error);
    } else {
        /* EOF on the CLOEXEC channel attests exec; the child cannot access this FD or path. */
        event(status_fd, "{\"inner_exec\":\"ok\",\"exit\":%d}\n", code);
    }
    close(status_fd);
    return code;
}

int main(int argc, char **argv) {
    if (argc == 2 && !strcmp(argv[1], "abi")) { printf("%d\n", abi()); return 0; }
    if (argc > 2 && !strcmp(argv[1], "landlock")) return landlock(argc - 2, argv + 2);
    if (argc > 2 && !strcmp(argv[1], "inner-landlock")) {
        return inner(argc - 2, argv + 2);
    }
    if (argc > 2 && !strcmp(argv[1], "init")) return init(argc - 2, argv + 2);
    if (argc > 2 && !strcmp(argv[1], "network")) {
        int socket_fd = socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0);
        struct ifreq request = {0};
        strcpy(request.ifr_name, "lo");
        if (socket_fd < 0 || ioctl(socket_fd, SIOCGIFFLAGS, &request) < 0) unavailable("network-flags");
        request.ifr_flags &= ~IFF_UP;
        if (ioctl(socket_fd, SIOCSIFFLAGS, &request) < 0) unavailable("network-down");
        if (ioctl(socket_fd, SIOCGIFFLAGS, &request) < 0 || (request.ifr_flags & IFF_UP))
            unavailable("network-readback");
        close(socket_fd);
        exec_no_shell(argv + 2);
        unavailable("inner-bwrap-exec");
    }
    if (argc > 4 && !strcmp(argv[1], "join") && !strcmp(argv[3], "--")) {
        char pid[32];
        snprintf(pid, sizeof(pid), "%d", getpid());
        write_control(atoi(argv[2]), pid);
        close(atoi(argv[2]));
        exec_no_shell(argv + 4);
    }
    unavailable("launcher-mode");
    return 125;
}
