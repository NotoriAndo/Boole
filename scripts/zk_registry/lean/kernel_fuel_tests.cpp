/* Generated-input controls, linked only into gate_fuel_tests. */
#include <memory>
#include <vector>
#include "runtime/interrupt.h"
#include "runtime/exception.h"
#include "lean/lean.h"
extern "C" lean_obj_res gate_kernel_reset();

extern "C" LEAN_EXPORT lean_obj_res gate_test_heartbeat_limit() {
    gate_kernel_reset();
    lean::scope_max_heartbeat limit(1);
    try { lean::check_heartbeat(); lean::check_heartbeat(); }
    catch (lean::heartbeat_exception const &) {}
    return lean_box(0);
}

extern "C" LEAN_EXPORT lean_obj_res gate_test_depth_limit() {
    gate_kernel_reset();
    lean::scope_max_rec_depth limit(1);
    std::vector<std::unique_ptr<lean::scope_rec_depth>> scopes;
    try {
        for (unsigned i = 0; i < 17; ++i)
            scopes.emplace_back(std::make_unique<lean::scope_rec_depth>());
    } catch (lean::stack_space_exception const &) {}
    return lean_box(0);
}
