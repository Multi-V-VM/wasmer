/**
 * MVVM Stub Implementations
 *
 * These stubs provide placeholder implementations for MVVM functions
 * that are required by MVVM's modified WAMR fork but are not yet
 * linked from the full MVVM library.
 *
 * TODO: Replace these stubs with actual MVVM implementations by
 * building and linking the full MVVM library.
 */

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>

/* Forward declarations for WAMR types */
typedef void* wasm_exec_env_t;
typedef void* WASMExecEnv;
typedef pthread_t korp_tid;
typedef pthread_mutex_t korp_mutex;
typedef pthread_cond_t korp_cond;

/* Sync operation types */
enum sync_op {
    SYNC_OP_MUTEX_LOCK = 0,
    SYNC_OP_MUTEX_UNLOCK,
    SYNC_OP_COND_WAIT,
    SYNC_OP_COND_SIGNAL,
    SYNC_OP_COND_BROADCAST,
    SYNC_OP_ATOMIC_WAIT,
    SYNC_OP_ATOMIC_NOTIFY,
};

/* Global variables used by MVVM's WAMR fork */
int stop_func_index = -1;
int stop_func_threshold = -1;
int cur_func_count = 0;
bool is_debug = false;
size_t snapshot_threshold = 0;
bool checkpoint = false;

/* Synchronization primitives for checkpoint coordination */
korp_cond syncop_cv;
korp_mutex syncop_mutex;

/**
 * Serialize execution state to file.
 * Stub: Currently does nothing.
 */
void serialize_to_file(WASMExecEnv *exec_env) {
    (void)exec_env;
    /* TODO: Implement actual serialization when MVVM is fully integrated */
}

/**
 * Create a lightweight checkpoint.
 * Stub: Currently does nothing.
 */
void lightweight_checkpoint(WASMExecEnv *exec_env) {
    (void)exec_env;
}

/**
 * Restore from a lightweight checkpoint.
 * Stub: Currently does nothing.
 */
void lightweight_uncheckpoint(WASMExecEnv *exec_env) {
    (void)exec_env;
}

/**
 * Insert a synchronization operation record.
 * Used for deterministic replay during migration.
 */
void insert_sync_op(wasm_exec_env_t exec_env, const uint32_t *mutex, enum sync_op locking) {
    (void)exec_env;
    (void)mutex;
    (void)locking;
}

/**
 * Insert atomic wait synchronization record.
 */
void insert_sync_op_atomic_wait(wasm_exec_env_t exec_env, const uint32_t *mutex, uint64_t expected, bool wait64) {
    (void)exec_env;
    (void)mutex;
    (void)expected;
    (void)wait64;
}

/**
 * Insert atomic notify synchronization record.
 */
void insert_sync_op_atomic_notify(wasm_exec_env_t exec_env, const uint32_t *mutex, uint32_t count) {
    (void)exec_env;
    (void)mutex;
    (void)count;
}

/**
 * Insert atomic wake synchronization record.
 */
void insert_sync_op_atomic_wake(wasm_exec_env_t exec_env, const uint32_t *mutex) {
    (void)exec_env;
    (void)mutex;
}

/**
 * Map old thread ID to new thread ID during restore.
 */
void wamr_handle_map(uint64_t old_tid, uint64_t new_tid) {
    (void)old_tid;
    (void)new_tid;
}

/**
 * Get a new thread ID mapping.
 */
korp_tid wamr_get_new_korp_tid(korp_tid tid) {
    return tid;
}

/**
 * Get the current thread ID mapping.
 */
korp_tid wamr_get_korp_tid(korp_tid tid) {
    return tid;
}

/**
 * Wait for migration checkpoint.
 */
void wamr_wait(wasm_exec_env_t exec_env) {
    (void)exec_env;
}

/**
 * Register SIGTRAP handler for debugging.
 */
void register_sigtrap(void) {
    /* No-op in stub implementation */
}

/**
 * Register SIGINT handler.
 */
void register_sigint(void) {
    /* No-op in stub implementation */
}

/**
 * SIGTRAP handler.
 */
void sigtrap_handler(int sig) {
    (void)sig;
}

/**
 * SIGINT handler.
 */
void sigint_handler(int sig) {
    (void)sig;
}

/**
 * Restore execution environment from checkpoint.
 */
void restore_env(void) {
    /* No-op in stub implementation */
}

/**
 * Insert parent-child thread relationship for tracking.
 */
void insert_parent_child(uint64_t parent, uint64_t child) {
    (void)parent;
    (void)child;
}

/**
 * Insert thread start argument.
 */
void insert_tid_start_arg(uint64_t tid, size_t arg1, size_t arg2) {
    (void)tid;
    (void)arg1;
    (void)arg2;
}

/**
 * Restart execution after migration.
 */
void restart_execution(uint32_t targs) {
    (void)targs;
}

/**
 * Check if atomic operations are checkpointable.
 */
bool is_atomic_checkpointable(void) {
    return true;
}

/**
 * Insert file descriptor operation record.
 */
void insert_fd(int fd, const char *path, int flags, int mode, int op) {
    (void)fd;
    (void)path;
    (void)flags;
    (void)mode;
    (void)op;
}

/**
 * Remove file descriptor record.
 */
void remove_fd(int fd) {
    (void)fd;
}

/**
 * Rename file descriptor.
 */
void rename_fd(int old_fd, const char *old_path, int new_fd, const char *new_path) {
    (void)old_fd;
    (void)old_path;
    (void)new_fd;
    (void)new_path;
}

/**
 * Print memory for debugging.
 */
void print_memory(WASMExecEnv *exec_env) {
    (void)exec_env;
}

/**
 * Print execution environment debug info.
 */
void print_exec_env_debug_info(WASMExecEnv *exec_env) {
    (void)exec_env;
}

/* =============================================================================
 * MVVM Checkpoint API Stubs
 *
 * These functions provide the checkpoint/restore API that is used by the
 * Rust bindings. In a full MVVM integration, these would be implemented
 * by the MVVM library itself.
 * ============================================================================= */

/* Opaque types for MVVM checkpoint API */
typedef struct mvvm_checkpoint_context_t {
    void *instance;
    int granularity;
} mvvm_checkpoint_context_t;

typedef struct mvvm_exec_env_t {
    void *_private;
} mvvm_exec_env_t;

typedef struct mvvm_frame_t {
    void *_private;
} mvvm_frame_t;

typedef struct mvvm_checkpoint_data_t {
    uint32_t version;
    size_t data_size;
    uint8_t *data;
} mvvm_checkpoint_data_t;

typedef enum mvvm_result_t {
    MVVM_OK = 0,
    MVVM_ERROR = 1,
    MVVM_INVALID_ARG = 2,
    MVVM_OUT_OF_MEMORY = 3,
    MVVM_CHECKPOINT_NOT_SUPPORTED = 4,
    MVVM_RESTORE_INCOMPATIBLE = 5,
    MVVM_MODULE_HASH_MISMATCH = 6,
} mvvm_result_t;

/**
 * Create a new checkpoint context for the given instance.
 * Stub: Returns a minimal context structure.
 */
mvvm_checkpoint_context_t* mvvm_checkpoint_context_new(void *instance) {
    mvvm_checkpoint_context_t *ctx = (mvvm_checkpoint_context_t *)malloc(sizeof(mvvm_checkpoint_context_t));
    if (ctx) {
        ctx->instance = instance;
        ctx->granularity = 0;
    }
    return ctx;
}

/**
 * Destroy a checkpoint context.
 */
void mvvm_checkpoint_context_delete(mvvm_checkpoint_context_t *ctx) {
    if (ctx) {
        free(ctx);
    }
}

/**
 * Create a checkpoint of the current execution state.
 * Stub: Returns CHECKPOINT_NOT_SUPPORTED as full implementation requires MVVM library.
 */
mvvm_result_t mvvm_create_checkpoint(
    mvvm_checkpoint_context_t *ctx,
    mvvm_checkpoint_data_t *data
) {
    (void)ctx;
    if (!data) {
        return MVVM_INVALID_ARG;
    }
    /* Stub implementation: checkpoint creation requires full MVVM library */
    data->version = 1;
    data->data_size = 0;
    data->data = NULL;
    return MVVM_CHECKPOINT_NOT_SUPPORTED;
}

/**
 * Restore execution state from a checkpoint.
 * Stub: Returns RESTORE_INCOMPATIBLE as full implementation requires MVVM library.
 */
mvvm_result_t mvvm_restore_checkpoint(
    mvvm_checkpoint_context_t *ctx,
    const mvvm_checkpoint_data_t *data
) {
    (void)ctx;
    (void)data;
    /* Stub implementation: restore requires full MVVM library */
    return MVVM_RESTORE_INCOMPATIBLE;
}

/**
 * Free checkpoint data allocated by mvvm_create_checkpoint.
 */
void mvvm_checkpoint_data_delete(mvvm_checkpoint_data_t *data) {
    if (data && data->data) {
        free(data->data);
        data->data = NULL;
        data->data_size = 0;
    }
}

/**
 * Get the execution environment from an instance.
 * Stub: Returns NULL.
 */
mvvm_exec_env_t* mvvm_instance_get_exec_env(void *instance) {
    (void)instance;
    return NULL;
}

/**
 * Get the current call frame from the execution environment.
 * Stub: Returns NULL.
 */
mvvm_frame_t* mvvm_exec_env_get_cur_frame(mvvm_exec_env_t *exec_env) {
    (void)exec_env;
    return NULL;
}

/**
 * Get the function index of a call frame.
 * Stub: Returns 0.
 */
uint32_t mvvm_frame_get_func_idx(mvvm_frame_t *frame) {
    (void)frame;
    return 0;
}

/**
 * Get the instruction pointer offset of a call frame.
 * Stub: Returns 0.
 */
uint32_t mvvm_frame_get_ip_offset(mvvm_frame_t *frame) {
    (void)frame;
    return 0;
}

/**
 * Get the stack pointer of a call frame.
 * Stub: Returns 0.
 */
uint32_t mvvm_frame_get_sp(mvvm_frame_t *frame) {
    (void)frame;
    return 0;
}

/**
 * Get the previous frame in the call stack.
 * Stub: Returns NULL.
 */
mvvm_frame_t* mvvm_frame_get_prev(mvvm_frame_t *frame) {
    (void)frame;
    return NULL;
}

/**
 * Get linear memory data pointer.
 * Stub: Returns NULL.
 */
uint8_t* mvvm_instance_get_memory_data(void *instance) {
    (void)instance;
    return NULL;
}

/**
 * Get linear memory size in bytes.
 * Stub: Returns 0.
 */
size_t mvvm_instance_get_memory_size(void *instance) {
    (void)instance;
    return 0;
}

/**
 * Get the module hash for validation.
 * Stub: Fills with zeros and returns OK.
 */
mvvm_result_t mvvm_instance_get_module_hash(void *instance, uint8_t hash[32]) {
    (void)instance;
    if (!hash) {
        return MVVM_INVALID_ARG;
    }
    memset(hash, 0, 32);
    return MVVM_OK;
}

/**
 * Set checkpoint granularity.
 * 0 = function level, 1 = loop level, 2 = instruction level
 */
void mvvm_set_checkpoint_granularity(mvvm_checkpoint_context_t *ctx, uint32_t level) {
    if (ctx) {
        ctx->granularity = level;
    }
}

/**
 * Check if the current execution position is checkpoint-safe.
 * Stub: Always returns true for testing purposes.
 * In a full implementation, this would check the actual execution state.
 */
bool mvvm_is_checkpoint_safe(mvvm_checkpoint_context_t *ctx) {
    (void)ctx;
    /* Stub: For testing, we say it's always safe.
     * In reality, this depends on the execution state. */
    return true;
}
