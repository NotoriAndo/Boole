/* Trusted PID-1 observer and ABI-adaptive, Landrun-CLI-compatible Landlock launcher. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/landlock.h>
#include <linux/capability.h>
#include <linux/securebits.h>
#include <net/if.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
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

static void unavailable(const char *stage) {
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

static int landlock(int argc, char **argv) {
    int version = abi();
    if (version < 4) unavailable("landlock-abi");
    uint64_t fs = (1ULL << 15) - 1;
    if (version >= 5) fs |= FS_IOCTL;
    if (version >= 9) fs |= FS_UNIX;
    struct { uint64_t fs, net, scoped; } attr = {.fs = fs, .net = 3, .scoped = version >= 6 ? 3 : 0};
    int ruleset = (int)syscall(__NR_landlock_create_ruleset, &attr, version >= 6 ? 24 : 16, 0);
    if (ruleset < 0) unavailable("landlock-create");
    int index = 0;
    for (; index < argc; ++index) {
        if (!strcmp(argv[index], "--")) { ++index; break; }
        if (!strcmp(argv[index], "--add-exec") || !strcmp(argv[index], "-add-exec")
                || !strcmp(argv[index], "--ldd") || !strcmp(argv[index], "-ldd")
                || !strcmp(argv[index], "--best-effort")) continue; /* Compatibility only; ABI >=4 stays mandatory. */
        if (index + 1 >= argc) unavailable("landlock-arguments");
        const char *flag = argv[index++], *value = argv[index];
        if (!strcmp(flag, "--ro") || !strcmp(flag, "--rox")) rule(ruleset, value, fs & READ_RIGHTS);
        else if (!strcmp(flag, "--rw")) rule(ruleset, value, fs & (READ_RIGHTS | FILE_RIGHTS));
        else if (!strcmp(flag, "--rwx")) rule(ruleset, value, fs);
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
    execvpe(argv[index], argv + index, environ);
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
    pid_t main_child = fork();
    if (main_child < 0) unavailable("main-fork");
    if (!main_child) {
        char pid[32];
        snprintf(pid, sizeof(pid), "%d", getpid());
        write_control(fds[0], pid);
        if (prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0) < 0) unavailable("child-parent-death");
        if (syscall(SYS_close_range, 3u, ~0u, 0) < 0) unavailable("child-close-descriptors");
        execvpe(argv[10], argv + 10, environ);
        unavailable("main-exec");
    }
    int main_status = 0, orphan_count = 0, reaped_status;
    pid_t reaped;
    for (;;) {
        reaped = waitpid(-1, &reaped_status, 0);
        if (reaped < 0 && errno == EINTR) continue;
        if (reaped < 0) unavailable("main-wait");
        if (reaped == main_child) { main_status = reaped_status; break; }
        ++orphan_count;
        dprintf(fds[7], "{\"event\":\"orphan\",\"pid\":%d}\n", reaped);
    }
    write_control(fds[1], "1");
    struct timespec pause = {.tv_nsec = 1000000};
    int frozen = 0;
    for (int attempt = 0; attempt < 1000; ++attempt) {
        if (counter(fds[2], "frozen ") == 1) { frozen = 1; break; }
        nanosleep(&pause, NULL);
    }
    if (!frozen) unavailable("cgroup-freeze");
    char members[16384];
    read_control(fds[3], members, sizeof(members));
    int remaining = 0;
    for (char *line = strtok(members, "\n"); line; line = strtok(NULL, "\n")) {
        long member = strtol(line, NULL, 10);
        if (member > 0) {
            ++remaining;
            dprintf(fds[7], "{\"event\":\"frozen-member\",\"pid\":%ld}\n", member);
        }
    }
    while ((reaped = waitpid(-1, &reaped_status, WNOHANG)) > 0)
        if (reaped != main_child) {
            ++orphan_count;
            dprintf(fds[7], "{\"event\":\"orphan\",\"pid\":%d}\n", reaped);
        }
    long oom = counter(fds[4], "oom_kill "), pids = counter(fds[5], "max ");
    if (remaining) write_control(fds[6], "1");
    write_control(fds[1], "0");
    while ((reaped = waitpid(-1, &reaped_status, 0)) > 0 || (reaped < 0 && errno == EINTR)) {
        if (reaped > 0 && reaped != main_child) {
            ++orphan_count;
            dprintf(fds[7], "{\"event\":\"orphan\",\"pid\":%d}\n", reaped);
        }
    }
    int code = WIFEXITED(main_status) ? WEXITSTATUS(main_status) : 128 + WTERMSIG(main_status);
    dprintf(fds[7], "{\"event\":\"final\",\"main_exit\":%d,\"orphans_reaped\":%d,\"frozen_members\":%d,"
            "\"oom_kill_events\":%ld,\"pids_max_events\":%ld,\"seccomp_mode\":%d,\"pid_1_dumpable\":%d,"
            "\"apparmor_label\":\"%s\",\"loopback_up\":%s,\"landlock_abi\":%d,"
            "\"caps_effective\":%llu,\"caps_ambient\":%llu,\"caps_bounding\":%llu,"
            "\"namespaces\":{\"user\":\"%s\",\"net\":\"%s\",\"pid\":\"%s\","
            "\"mnt\":\"%s\",\"ipc\":\"%s\"}}\n",
            code, orphan_count, remaining, oom, pids, seccomp, prctl(PR_GET_DUMPABLE, 0, 0, 0, 0), profile,
            loopback_up ? "true" : "false", abi(), effective, ambient, bounding,
            userns, netns, pidns, mountns, ipcns);
    close(fds[7]);
    return code;
}

int main(int argc, char **argv) {
    if (argc == 2 && !strcmp(argv[1], "abi")) { printf("%d\n", abi()); return 0; }
    if (argc > 2 && !strcmp(argv[1], "landlock")) return landlock(argc - 2, argv + 2);
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
        execvpe(argv[2], argv + 2, environ);
        unavailable("inner-bwrap-exec");
    }
    if (argc > 4 && !strcmp(argv[1], "join") && !strcmp(argv[3], "--")) {
        char pid[32];
        snprintf(pid, sizeof(pid), "%d", getpid());
        write_control(atoi(argv[2]), pid);
        close(atoi(argv[2]));
        execvpe(argv[4], argv + 4, environ);
    }
    unavailable("launcher-mode");
    return 125;
}
