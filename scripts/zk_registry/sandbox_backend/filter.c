/* Build architecture-specific filters with the pinned Ubuntu libseccomp. */
#include <errno.h>
#include <fcntl.h>
#include <seccomp.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 3 && argc != 4) return 2;
    uint32_t arch = seccomp_arch_resolve_name(argv[1]);
    if (!arch) return 2;
    /* Generation is architecture-neutral; actual kernel support is required by the live preflight. */
    if (seccomp_api_set(3) < 0) return 2; /* KILL_PROCESS is mandatory, including cross-generation under QEMU. */
    scmp_filter_ctx ctx = seccomp_init(SCMP_ACT_ALLOW);
    if (!ctx) return 2;
    if (seccomp_attr_set(ctx, SCMP_FLTATR_ACT_BADARCH, SCMP_ACT_KILL_PROCESS) < 0) return 2;
    if (arch != seccomp_arch_native()) {
        if (seccomp_arch_remove(ctx, seccomp_arch_native()) < 0 || seccomp_arch_add(ctx, arch) < 0) return 2;
    }
    const char *denied[] = {
        "ptrace", "mount", "umount2", "keyctl", "add_key", "request_key", "bpf", "perf_event_open",
        "io_uring_setup", "io_uring_enter", "io_uring_register", "fsopen", "fsconfig", "fsmount",
        "fspick", "open_tree", "move_mount", "userfaultfd", "socketcall", "setns", "unshare",
        "pidfd_getfd", "process_vm_readv", "process_vm_writev"
    };
    for (unsigned i = 0; i < sizeof(denied) / sizeof(denied[0]); ++i) {
        int number = seccomp_syscall_resolve_name(denied[i]);
        if (number < 0) continue; /* No such native syscall; other architectures are killed. */
        if (seccomp_rule_add(ctx, SCMP_ACT_ERRNO(EPERM), number, 0) < 0) return 2;
    }
    int socket_nr = seccomp_syscall_resolve_name("socket");
    /* Permit only canonical AF_UNIX; high-bit/truncation variants are also denied. */
    if (seccomp_rule_add(ctx, SCMP_ACT_ERRNO(EPERM), socket_nr, 1, SCMP_A0(SCMP_CMP_NE, 1, 0)) < 0) return 2;
    int clone3_nr = seccomp_syscall_resolve_name("clone3");
    if (clone3_nr >= 0 && seccomp_rule_add(ctx, SCMP_ACT_ERRNO(ENOSYS), clone3_nr, 0) < 0) return 2;
    int file = open(argv[2], O_WRONLY | O_CREAT | O_EXCL, 0644);
    if (file < 0 || seccomp_export_bpf(ctx, file) < 0) return 2;
    close(file);
    if (argc == 4) {
        file = open(argv[3], O_WRONLY | O_CREAT | O_EXCL, 0644);
        if (file < 0 || seccomp_export_pfc(ctx, file) < 0) return 2;
        close(file);
    }
    seccomp_release(ctx);
    return 0;
}
