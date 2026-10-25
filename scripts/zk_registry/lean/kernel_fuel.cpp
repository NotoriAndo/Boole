/*
Copyright (c) 2013 Microsoft Corporation. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.

Author: Leonardo de Moura

Modified for Boole DET3: logical budget-hit bits, per-declaration reset,
checkpoint/depth observations and dedicated-thread stack measurements.
Derived from Lean src/runtime/interrupt.cpp at commit
5045d0056413266e57c625dcd7c365b10e377c52; these modifications are not upstream.
*/
#include <limits>
#include <algorithm>
#include <pthread.h>
#include "runtime/thread.h"
#include "runtime/interrupt.h"
#include "runtime/exception.h"
#include "runtime/memory.h"
#include "lean/lean.h"

namespace lean {
LEAN_THREAD_VALUE(size_t, g_max_heartbeat, 0);
LEAN_THREAD_VALUE(size_t, g_heartbeat, 0);
LEAN_THREAD_VALUE(size_t, g_gate_peak_depth, 0);
LEAN_THREAD_VALUE(size_t, g_gate_budget_hit, 0);
LEAN_THREAD_VALUE(size_t, g_gate_stack_top, 0);
LEAN_THREAD_VALUE(size_t, g_gate_stack_size, 0);
LEAN_THREAD_VALUE(size_t, g_gate_peak_stack, 0);
static void gate_init_stack() {
    if (g_gate_stack_top) return;
#ifdef __APPLE__
    g_gate_stack_top = reinterpret_cast<size_t>(pthread_get_stackaddr_np(pthread_self()));
    g_gate_stack_size = pthread_get_stacksize_np(pthread_self());
#else
    pthread_attr_t attrs;
    if (pthread_getattr_np(pthread_self(), &attrs)) std::abort();
    void *base = nullptr; size_t size = 0;
    if (pthread_attr_getstack(&attrs, &base, &size)) std::abort();
    pthread_attr_destroy(&attrs);
    g_gate_stack_top = reinterpret_cast<size_t>(base) + size;
    g_gate_stack_size = size;
#endif
}
static void gate_sample_stack() {
    volatile char marker = 0;
    gate_init_stack();
    size_t current = reinterpret_cast<size_t>(&marker);
    if (current > g_gate_stack_top || g_gate_stack_top-current > g_gate_stack_size) std::abort();
    g_gate_peak_stack = std::max(g_gate_peak_stack, g_gate_stack_top-current);
}


extern "C" LEAN_EXPORT lean_obj_res lean_internal_get_default_max_heartbeat() {
#ifdef LEAN_DEFAULT_MAX_HEARTBEAT
    return lean_box(LEAN_DEFAULT_MAX_HEARTBEAT);
#else
    return lean_box(0);
#endif
}

void inc_heartbeat() { g_heartbeat++; }

void reset_heartbeat() { g_heartbeat = 0; }

void set_max_heartbeat(size_t max) { g_max_heartbeat = max; }

extern "C" LEAN_EXPORT lean_obj_res lean_internal_set_max_heartbeat(size_t max) {
    set_max_heartbeat(max);
    return lean_box(0);
}

size_t get_max_heartbeat() { return g_max_heartbeat; }

void set_max_heartbeat_thousands(unsigned max) { g_max_heartbeat = static_cast<size_t>(max) * 1000; }

scope_heartbeat::scope_heartbeat(size_t max):flet<size_t>(g_heartbeat, max) {}
LEAN_EXPORT scope_max_heartbeat::scope_max_heartbeat(size_t max):flet<size_t>(g_max_heartbeat, max) {}

// separate definition to allow breakpoint in debugger
void throw_heartbeat_exception() {
    throw heartbeat_exception();
}

void check_heartbeat() {
    inc_heartbeat();
    if (g_max_heartbeat > 0 && g_heartbeat > g_max_heartbeat) {
        g_gate_budget_hit |= 1;
        throw_heartbeat_exception();
    }
}

LEAN_THREAD_VALUE(size_t, g_max_rec_depth, 0);
LEAN_THREAD_VALUE(size_t, g_rec_depth, 0);

/* The kernel re-checks a fully elaborated term from scratch, without the caching, metavariable
   assignments, and reducibility shortcuts the elaborator uses while building it incrementally. As a
   result the kernel recurses substantially deeper than the elaborator did for the same term (stdlib
   `grind`/`simp` proofs check several thousand levels deep). We therefore let the kernel reach a
   generous multiple of the configured `maxRecDepth` before bailing out, so that code which fits
   within `maxRecDepth` during elaboration is not rejected by the kernel. */
static constexpr size_t g_kernel_rec_depth_factor = 16;

void set_max_rec_depth(size_t max) { g_max_rec_depth = max; }
size_t get_max_rec_depth() { return g_max_rec_depth; }

LEAN_EXPORT scope_max_rec_depth::scope_max_rec_depth(size_t max) :
    m_max(g_max_rec_depth, max), m_curr(g_rec_depth, 0) {}

LEAN_EXPORT scope_rec_depth::scope_rec_depth() {
    g_rec_depth++;
    if (g_rec_depth > g_gate_peak_depth) {
        g_gate_peak_depth = g_rec_depth;
        gate_sample_stack();
    }
    if (g_max_rec_depth > 0 && g_rec_depth > g_max_rec_depth * g_kernel_rec_depth_factor) {
        g_gate_budget_hit |= 2;
        g_rec_depth--;
        throw stack_space_exception("type checker");
    }
}

LEAN_EXPORT scope_rec_depth::~scope_rec_depth() { g_rec_depth--; }

LEAN_THREAD_VALUE(lean_object *, g_cancel_tk, nullptr);

LEAN_EXPORT scope_cancel_tk::scope_cancel_tk(lean_object * o):flet<lean_object *>(g_cancel_tk, o) {}

// `IO.CancelToken` is `structure { promise : IO.Promise Unit; setRef : IO.Ref Bool }`. We read
// the `Bool` flag (field 1) directly: cheaper than walking the promise's task state, and this
// is on the hot `Core.checkInterrupted` path. Must stay in sync with the field order in
// `Init/System/CancelToken.lean`.
static bool cancel_tk_is_set(lean_object * tk) {
    lean_object * setRef = lean_ctor_get(tk, 1);
    return lean_unbox(lean_to_ref(setRef)->m_value) != 0;
}

void check_interrupted() {
    if (g_cancel_tk) {
        // `g_cancel_tk` is owned by the enclosing `scope_cancel_tk`, so it stays alive for the
        // duration of this call without an explicit `inc_ref`.
        if (cancel_tk_is_set(g_cancel_tk) &&
            !std::uncaught_exceptions()) {
            throw interrupted();
        }
    }
}

void check_system(char const * component_name, bool do_check_interrupted) {
    check_stack(component_name);
    check_memory(component_name);
    if (do_check_interrupted) {
        check_interrupted();
        check_heartbeat();
    }
}

void sleep_for(unsigned ms, unsigned step_ms) {
    if (step_ms == 0)
        step_ms = 1;
    unsigned rounds = ms / step_ms;
    chrono::milliseconds c(step_ms);
    chrono::milliseconds r(ms % step_ms);
    for (unsigned i = 0; i < rounds; i++) {
        this_thread::sleep_for(c);
        check_interrupted();
    }
    this_thread::sleep_for(r);
    check_interrupted();
}
}

// This translation unit is linked before the pinned static Lean runtime.
extern "C" LEAN_EXPORT lean_obj_res gate_kernel_reset() {
    lean::reset_heartbeat();
    lean::g_gate_peak_depth = 0;
    lean::g_gate_budget_hit = 0;
    lean::g_gate_peak_stack = 0;
    lean::gate_sample_stack();
    return lean_box(0);
}
extern "C" LEAN_EXPORT lean_obj_res gate_kernel_checkpoints() {
    return lean_usize_to_nat(lean::g_heartbeat);
}

extern "C" LEAN_EXPORT lean_obj_res gate_kernel_depth() {
    return lean_usize_to_nat(lean::g_gate_peak_depth);
}
extern "C" LEAN_EXPORT lean_obj_res gate_kernel_stack_capacity() {
    lean::gate_init_stack();
    return lean_usize_to_nat(lean::g_gate_stack_size);
}
extern "C" LEAN_EXPORT lean_obj_res gate_kernel_peak_stack() {
    return lean_usize_to_nat(lean::g_gate_peak_stack);
}

extern "C" LEAN_EXPORT lean_obj_res gate_kernel_budget_hit() {
    return lean_usize_to_nat(lean::g_gate_budget_hit);
}
