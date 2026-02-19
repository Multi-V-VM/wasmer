/**
 * MVVM State Tracking Implementation for Wasmer
 *
 * Implements the MVVM checkpoint/restore hooks that MVVM's WAMR fork calls
 * during execution. These functions track file descriptors, sockets,
 * synchronization operations, and thread mappings so the Rust layer can
 * perform checkpoint/restore.
 *
 * Based on the MVVM project by Aibo Hu, Yiwei Yang, Brian Zhao, Andi Quinn
 * UC Santa Cruz Sluglab.
 *
 * SPDX-License-Identifier: (LGPL-2.1 OR BSD-2-Clause)
 */

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <signal.h>
#include <pthread.h>

/* Forward declarations for WAMR types we reference */
typedef unsigned int uint32;
typedef unsigned short uint16;
typedef unsigned char uint8;
typedef unsigned long long uint64;
typedef long long int64;
typedef pthread_t korp_tid;
typedef pthread_mutex_t korp_mutex;
typedef pthread_cond_t korp_cond;

/* Minimal WAMR exec env - matches the fields we access */
typedef struct WASMExecEnv {
    /* We only access handle, is_restore, cur_frame, module_inst */
    uint8 _opaque[4096];
} WASMExecEnv;

typedef WASMExecEnv *wasm_exec_env_t;
typedef void *WASMModuleInstanceCommon;

/* ==========================================================================
 * MVVM Data Structures
 * ========================================================================== */

/* Match the SocketAddrPool from wamr_export.h */
struct SocketAddrPool {
    uint8 ip4[4];
    uint16 ip6[8];
    bool is_4;
    uint16 port;
};

/* File descriptor operations */
enum fd_op {
    MVVM_FOPEN = 0,
    MVVM_FWRITE = 1,
    MVVM_FREAD = 2,
    MVVM_FSEEK = 3
};

/* Synchronization operation types */
enum sync_op {
    SYNC_OP_MUTEX_LOCK = 0,
    SYNC_OP_MUTEX_UNLOCK,
    SYNC_OP_COND_WAIT,
    SYNC_OP_COND_SIGNAL,
    SYNC_OP_COND_BROADCAST,
    SYNC_OP_ATOMIC_WAIT,
    SYNC_OP_ATOMIC_NOTIFY,
};

struct sync_op_t {
    uint64 tid;
    uint32 ref;
    enum sync_op sync_op;
    uint64 expected;
    bool wait64;
};

/* ---------- Socket recv/send data recording ---------- */

#define MVVM_MAX_RECV_ENTRIES 4096
#define MVVM_MAX_SOCKETS 256
#define MVVM_MAX_FDS 1024
#define MVVM_MAX_SYNC_OPS 65536
#define MVVM_MAX_TID_MAP 256
#define MVVM_MAX_PATH 512

/* WASI address types - matches WAMR's __wasi_addr_t */
typedef struct {
    uint8 n0, n1, n2, n3;
} __wasi_addr_ip4_t;

typedef struct {
    __wasi_addr_ip4_t addr;
    uint16 port;
} __wasi_addr_ip4_port_t;

typedef struct {
    uint16 n0, n1, n2, n3, h0, h1, h2, h3;
} __wasi_addr_ip6_t;

typedef struct {
    __wasi_addr_ip6_t addr;
    uint16 port;
} __wasi_addr_ip6_port_t;

typedef enum { IPv4 = 0, IPv6 = 1 } __wasi_addr_type_t;

typedef struct {
    __wasi_addr_type_t kind;
    union {
        __wasi_addr_ip4_port_t ip4;
        __wasi_addr_ip6_port_t ip6;
    } addr;
} __wasi_addr_t;

/* Recorded recv-from data for replay */
typedef struct {
    uint32 sock;
    uint8 *data;
    size_t data_len;
    uint16 ri_flags;
    struct SocketAddrPool src_addr;
} mvvm_recv_entry_t;

/* Per-socket metadata for checkpoint/restore */
typedef struct {
    int fd;
    int domain;
    int type;
    int protocol;
    struct SocketAddrPool address;
    bool is_tcp;
    bool is_server;
    /* Socket open data */
    uint32 poolfd;
    int open_af;
    int open_socktype;
    uint32 open_sockfd;
    /* Send-to data (last recorded) */
    uint8 *send_data;
    size_t send_data_len;
    uint16 si_flags;
    struct SocketAddrPool send_dest_addr;
    /* Recv-from data ring buffer for replay */
    mvvm_recv_entry_t recv_entries[MVVM_MAX_RECV_ENTRIES];
    int recv_count;
    int replay_index;
    bool in_use;
} mvvm_socket_meta_t;

/* Per-fd metadata */
typedef struct {
    int fd;
    char path[MVVM_MAX_PATH];
    int flags;
    int offset;
    enum fd_op last_op;
    bool in_use;
} mvvm_fd_meta_t;

/* Thread ID mapping entry */
typedef struct {
    uint64 old_tid;
    uint64 new_tid;
    bool in_use;
} mvvm_tid_map_entry_t;

/* Parent-child thread mapping */
typedef struct {
    uint64 child_tid;
    uint64 parent_tid;
    bool in_use;
} mvvm_parent_child_entry_t;

/* ---------- Global MVVM State ---------- */

typedef struct {
    /* Socket tracking */
    mvvm_socket_meta_t sockets[MVVM_MAX_SOCKETS];
    int socket_count;
    int new_sock_map[MVVM_MAX_SOCKETS][2]; /* [old_fd, new_fd] pairs */
    int new_sock_map_count;
    bool is_tcp;

    /* File descriptor tracking */
    mvvm_fd_meta_t fds[MVVM_MAX_FDS];
    int fd_count;

    /* Synchronization operation log */
    struct sync_op_t *sync_ops;
    int sync_ops_count;
    int sync_ops_capacity;

    /* Thread ID mappings */
    mvvm_tid_map_entry_t tid_map[MVVM_MAX_TID_MAP];
    int tid_map_count;

    /* Parent-child thread relationships */
    mvvm_parent_child_entry_t parent_child[MVVM_MAX_TID_MAP];
    int parent_child_count;

    /* Lightweight checkpoint tracking */
    uint64 lwcp_tids[MVVM_MAX_TID_MAP];
    int lwcp_counts[MVVM_MAX_TID_MAP];
    int lwcp_count;
    int ready;

    /* Lock for thread-safe access */
    pthread_mutex_t mtx;
    bool initialized;
} mvvm_state_t;

static mvvm_state_t g_mvvm;

static void mvvm_ensure_init(void) {
    if (!g_mvvm.initialized) {
        pthread_mutex_init(&g_mvvm.mtx, NULL);
        g_mvvm.sync_ops_capacity = 1024;
        g_mvvm.sync_ops = (struct sync_op_t *)calloc(
            g_mvvm.sync_ops_capacity, sizeof(struct sync_op_t));
        g_mvvm.initialized = true;
    }
}

/* ---------- Socket lookup helpers ---------- */

static mvvm_socket_meta_t *find_socket(int fd) {
    for (int i = 0; i < MVVM_MAX_SOCKETS; i++) {
        if (g_mvvm.sockets[i].in_use && g_mvvm.sockets[i].fd == fd)
            return &g_mvvm.sockets[i];
    }
    return NULL;
}

static mvvm_socket_meta_t *alloc_socket(int fd) {
    for (int i = 0; i < MVVM_MAX_SOCKETS; i++) {
        if (!g_mvvm.sockets[i].in_use) {
            memset(&g_mvvm.sockets[i], 0, sizeof(mvvm_socket_meta_t));
            g_mvvm.sockets[i].fd = fd;
            g_mvvm.sockets[i].in_use = true;
            g_mvvm.socket_count++;
            return &g_mvvm.sockets[i];
        }
    }
    return NULL;
}

/* ---------- FD lookup helpers ---------- */

static mvvm_fd_meta_t *find_fd(int fd) {
    for (int i = 0; i < MVVM_MAX_FDS; i++) {
        if (g_mvvm.fds[i].in_use && g_mvvm.fds[i].fd == fd)
            return &g_mvvm.fds[i];
    }
    return NULL;
}

static mvvm_fd_meta_t *alloc_fd(int fd) {
    for (int i = 0; i < MVVM_MAX_FDS; i++) {
        if (!g_mvvm.fds[i].in_use) {
            memset(&g_mvvm.fds[i], 0, sizeof(mvvm_fd_meta_t));
            g_mvvm.fds[i].fd = fd;
            g_mvvm.fds[i].in_use = true;
            g_mvvm.fd_count++;
            return &g_mvvm.fds[i];
        }
    }
    return NULL;
}

/* ==========================================================================
 * Exported globals required by WAMR's MVVM fork
 * ========================================================================== */

int stop_func_index = -1;
int stop_func_threshold = -1;
int cur_func_count = 0;
bool is_debug = false;
size_t snapshot_threshold = 0;
bool checkpoint = false;

korp_cond syncop_cv;
korp_mutex syncop_mutex;

/* ==========================================================================
 * Socket Tracking Functions
 * ========================================================================== */

/**
 * Get the mapped socket fd for restore.
 * During restore, old fds are mapped to new fds.
 */
int get_sock_fd(int fd) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);
    for (int i = 0; i < g_mvvm.new_sock_map_count; i++) {
        if (g_mvvm.new_sock_map[i][0] == fd) {
            int result = g_mvvm.new_sock_map[i][1];
            pthread_mutex_unlock(&g_mvvm.mtx);
            return result;
        }
    }
    pthread_mutex_unlock(&g_mvvm.mtx);
    return fd;
}

/**
 * Initialize gateway connection for socket migration.
 * In the embedded Wasmer context, this is a no-op since migration
 * is driven by the Rust layer. The address is recorded for later use.
 */
void init_gateway(struct SocketAddrPool *address) {
    (void)address;
    /* Gateway initialization is handled by the Rust layer */
}

/**
 * Record a newly opened socket for checkpoint/restore tracking.
 */
void insert_socket(int fd, int domain, int type, int protocol) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    mvvm_socket_meta_t *sock = find_socket(fd);
    if (sock) {
        fprintf(stderr, "mvvm: insert_socket: fd %d already tracked\n", fd);
        pthread_mutex_unlock(&g_mvvm.mtx);
        return;
    }

    sock = alloc_socket(fd);
    if (!sock) {
        fprintf(stderr, "mvvm: insert_socket: socket table full\n");
        pthread_mutex_unlock(&g_mvvm.mtx);
        return;
    }

    sock->domain = domain;
    sock->type = type;
    sock->protocol = protocol;
    if (protocol == 1)
        sock->is_server = true;
    /* If protocol > 1, inherit address from that socket (used for accept) */
    if (protocol > 1) {
        mvvm_socket_meta_t *parent = find_socket(protocol);
        if (parent)
            sock->address = parent->address;
    }

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Record socket open parameters for replay during restore.
 */
void insert_sock_open_data(uint32_t poolfd, int af, int socktype, uint32_t sockfd) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    mvvm_socket_meta_t *sock = find_socket((int)sockfd);
    if (sock) {
        sock->poolfd = poolfd;
        sock->open_af = af;
        sock->open_socktype = socktype;
        sock->open_sockfd = sockfd;
    } else {
        fprintf(stderr, "mvvm: insert_sock_open_data: fd %u not found\n", sockfd);
    }

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Record received data from a socket for replay during restore.
 */
void insert_sock_recv_from_data(uint32_t sock, uint8 *ri_data, uint32 ri_data_len,
                                uint16_t ri_flags, __wasi_addr_t *src_addr) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    mvvm_socket_meta_t *s = find_socket((int)sock);
    if (!s) {
        fprintf(stderr, "mvvm: insert_sock_recv_from_data: fd %u not found\n", sock);
        pthread_mutex_unlock(&g_mvvm.mtx);
        return;
    }

    if (s->recv_count >= MVVM_MAX_RECV_ENTRIES) {
        fprintf(stderr, "mvvm: insert_sock_recv_from_data: recv buffer full for fd %u\n", sock);
        pthread_mutex_unlock(&g_mvvm.mtx);
        return;
    }

    mvvm_recv_entry_t *entry = &s->recv_entries[s->recv_count];
    entry->sock = sock;
    entry->data = (uint8 *)malloc(ri_data_len);
    if (entry->data) {
        memcpy(entry->data, ri_data, ri_data_len);
        entry->data_len = ri_data_len;
    } else {
        entry->data_len = 0;
    }
    entry->ri_flags = ri_flags;

    /* Convert __wasi_addr_t to SocketAddrPool */
    if (src_addr) {
        if (src_addr->kind == IPv4) {
            entry->src_addr.is_4 = true;
            entry->src_addr.ip4[0] = src_addr->addr.ip4.addr.n0;
            entry->src_addr.ip4[1] = src_addr->addr.ip4.addr.n1;
            entry->src_addr.ip4[2] = src_addr->addr.ip4.addr.n2;
            entry->src_addr.ip4[3] = src_addr->addr.ip4.addr.n3;
            entry->src_addr.port = src_addr->addr.ip4.port;
        } else {
            entry->src_addr.is_4 = false;
            entry->src_addr.ip6[0] = src_addr->addr.ip6.addr.n0;
            entry->src_addr.ip6[1] = src_addr->addr.ip6.addr.n1;
            entry->src_addr.ip6[2] = src_addr->addr.ip6.addr.n2;
            entry->src_addr.ip6[3] = src_addr->addr.ip6.addr.n3;
            entry->src_addr.ip6[4] = src_addr->addr.ip6.addr.h0;
            entry->src_addr.ip6[5] = src_addr->addr.ip6.addr.h1;
            entry->src_addr.ip6[6] = src_addr->addr.ip6.addr.h2;
            entry->src_addr.ip6[7] = src_addr->addr.ip6.addr.h3;
            entry->src_addr.port = src_addr->addr.ip6.port;
        }
    }

    s->recv_count++;
    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Record sent data to a socket for checkpoint.
 */
void insert_sock_send_to_data(uint32_t sock, uint8 *si_data, uint32 si_data_len,
                              uint16_t si_flags, __wasi_addr_t *dest_addr) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    mvvm_socket_meta_t *s = find_socket((int)sock);
    if (!s) {
        fprintf(stderr, "mvvm: insert_sock_send_to_data: fd %u not found\n", sock);
        pthread_mutex_unlock(&g_mvvm.mtx);
        return;
    }

    /* Free previous send data */
    free(s->send_data);
    s->send_data = (uint8 *)malloc(si_data_len);
    if (s->send_data) {
        memcpy(s->send_data, si_data, si_data_len);
        s->send_data_len = si_data_len;
    } else {
        s->send_data_len = 0;
    }
    s->si_flags = si_flags;

    /* Convert __wasi_addr_t to SocketAddrPool */
    if (dest_addr) {
        if (dest_addr->kind == IPv4) {
            s->send_dest_addr.is_4 = true;
            s->send_dest_addr.ip4[0] = dest_addr->addr.ip4.addr.n0;
            s->send_dest_addr.ip4[1] = dest_addr->addr.ip4.addr.n1;
            s->send_dest_addr.ip4[2] = dest_addr->addr.ip4.addr.n2;
            s->send_dest_addr.ip4[3] = dest_addr->addr.ip4.addr.n3;
            s->send_dest_addr.port = dest_addr->addr.ip4.port;
        } else {
            s->send_dest_addr.is_4 = false;
            s->send_dest_addr.ip6[0] = dest_addr->addr.ip6.addr.n0;
            s->send_dest_addr.ip6[1] = dest_addr->addr.ip6.addr.n1;
            s->send_dest_addr.ip6[2] = dest_addr->addr.ip6.addr.n2;
            s->send_dest_addr.ip6[3] = dest_addr->addr.ip6.addr.n3;
            s->send_dest_addr.ip6[4] = dest_addr->addr.ip6.addr.h0;
            s->send_dest_addr.ip6[5] = dest_addr->addr.ip6.addr.h1;
            s->send_dest_addr.ip6[6] = dest_addr->addr.ip6.addr.h2;
            s->send_dest_addr.ip6[7] = dest_addr->addr.ip6.addr.h3;
            s->send_dest_addr.port = dest_addr->addr.ip6.port;
        }
    }

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Replay previously recorded recv data during restore.
 * Drains entries in FIFO order from the socket's recv buffer.
 */
void replay_sock_recv_from_data(uint32_t sock, uint8 **ri_data,
                                unsigned long *recv_size, __wasi_addr_t *src_addr) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    mvvm_socket_meta_t *s = find_socket((int)sock);
    if (!s || s->recv_count == 0 || s->replay_index >= s->recv_count) {
        *recv_size = 0;
        pthread_mutex_unlock(&g_mvvm.mtx);
        return;
    }

    mvvm_recv_entry_t *entry = &s->recv_entries[s->replay_index];
    s->replay_index++;

    if (entry->data && entry->data_len > 0) {
        memcpy(*ri_data, entry->data, entry->data_len);
        *recv_size = entry->data_len;
    } else {
        *recv_size = 0;
    }

    /* Restore source address */
    if (src_addr) {
        if (entry->src_addr.is_4) {
            src_addr->kind = IPv4;
            src_addr->addr.ip4.addr.n0 = entry->src_addr.ip4[0];
            src_addr->addr.ip4.addr.n1 = entry->src_addr.ip4[1];
            src_addr->addr.ip4.addr.n2 = entry->src_addr.ip4[2];
            src_addr->addr.ip4.addr.n3 = entry->src_addr.ip4[3];
            src_addr->addr.ip4.port = entry->src_addr.port;
        } else {
            src_addr->kind = IPv6;
            src_addr->addr.ip6.addr.n0 = entry->src_addr.ip6[0];
            src_addr->addr.ip6.addr.n1 = entry->src_addr.ip6[1];
            src_addr->addr.ip6.addr.n2 = entry->src_addr.ip6[2];
            src_addr->addr.ip6.addr.n3 = entry->src_addr.ip6[3];
            src_addr->addr.ip6.addr.h0 = entry->src_addr.ip6[4];
            src_addr->addr.ip6.addr.h1 = entry->src_addr.ip6[5];
            src_addr->addr.ip6.addr.h2 = entry->src_addr.ip6[6];
            src_addr->addr.ip6.addr.h3 = entry->src_addr.ip6[7];
            src_addr->addr.ip6.port = entry->src_addr.port;
        }
    }

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Mark the current connection type as TCP.
 */
void set_tcp(void) {
    mvvm_ensure_init();
    g_mvvm.is_tcp = true;
}

/**
 * Update the address associated with a tracked socket.
 */
void update_socket_fd_address(int fd, struct SocketAddrPool *address) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    mvvm_socket_meta_t *sock = find_socket(fd);
    if (sock && address) {
        sock->address = *address;
    }

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/* ==========================================================================
 * File Descriptor Tracking
 * ========================================================================== */

/**
 * Record a file descriptor operation for checkpoint/restore.
 */
void insert_fd(int fd, const char *path, int flags, int offset, enum fd_op op) {
    if (fd <= 2) return; /* Skip stdin/stdout/stderr */

    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    mvvm_fd_meta_t *meta = find_fd(fd);
    if (!meta) {
        meta = alloc_fd(fd);
        if (!meta) {
            pthread_mutex_unlock(&g_mvvm.mtx);
            return;
        }
    }

    if (path && path[0] != '\0') {
        strncpy(meta->path, path, MVVM_MAX_PATH - 1);
        meta->path[MVVM_MAX_PATH - 1] = '\0';
    }
    meta->flags = flags;
    meta->offset = offset;
    meta->last_op = op;

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Remove a file descriptor from tracking.
 */
void remove_fd(int fd) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    mvvm_fd_meta_t *meta = find_fd(fd);
    if (meta) {
        meta->in_use = false;
        g_mvvm.fd_count--;
    } else {
        /* Also check socket map */
        mvvm_socket_meta_t *sock = find_socket(fd);
        if (sock) {
            /* Free recv buffer data */
            for (int i = 0; i < sock->recv_count; i++) {
                free(sock->recv_entries[i].data);
            }
            free(sock->send_data);
            sock->in_use = false;
            g_mvvm.socket_count--;
        }
    }

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Rename/move a file descriptor entry (e.g., after path_rename).
 */
void rename_fd(int old_fd, char const *old_path, int new_fd, char const *new_path) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    mvvm_fd_meta_t *meta = find_fd(old_fd);
    if (meta) {
        meta->fd = new_fd;
        if (new_path && new_path[0] != '\0') {
            strncpy(meta->path, new_path, MVVM_MAX_PATH - 1);
            meta->path[MVVM_MAX_PATH - 1] = '\0';
        }
    }

    (void)old_path;
    pthread_mutex_unlock(&g_mvvm.mtx);
}

/* ==========================================================================
 * Synchronization Operation Tracking
 * ========================================================================== */

static void sync_ops_push(struct sync_op_t *op) {
    if (g_mvvm.sync_ops_count >= g_mvvm.sync_ops_capacity) {
        int new_cap = g_mvvm.sync_ops_capacity * 2;
        struct sync_op_t *new_buf = (struct sync_op_t *)realloc(
            g_mvvm.sync_ops, new_cap * sizeof(struct sync_op_t));
        if (!new_buf) return;
        g_mvvm.sync_ops = new_buf;
        g_mvvm.sync_ops_capacity = new_cap;
    }
    g_mvvm.sync_ops[g_mvvm.sync_ops_count++] = *op;
}

/**
 * Record a synchronization operation (mutex lock/unlock, cond wait/signal).
 */
void insert_sync_op(wasm_exec_env_t exec_env, const uint32 *mutex, enum sync_op locking) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    struct sync_op_t op;
    memset(&op, 0, sizeof(op));
    op.tid = (uint64)(uintptr_t)exec_env; /* Use exec_env as tid proxy */
    op.ref = *mutex;
    op.sync_op = locking;
    sync_ops_push(&op);

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Record an atomic wait operation for deterministic replay.
 */
void insert_sync_op_atomic_wait(wasm_exec_env_t exec_env, const uint32 *mutex,
                                uint64 expected, bool wait64) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    struct sync_op_t op;
    memset(&op, 0, sizeof(op));
    op.tid = (uint64)(uintptr_t)exec_env;
    op.ref = *mutex;
    op.sync_op = SYNC_OP_ATOMIC_WAIT;
    op.expected = expected;
    op.wait64 = wait64;
    sync_ops_push(&op);

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Record an atomic notify operation.
 */
void insert_sync_op_atomic_notify(wasm_exec_env_t exec_env, const uint32 *mutex,
                                  uint32 count) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    struct sync_op_t op;
    memset(&op, 0, sizeof(op));
    op.tid = (uint64)(uintptr_t)exec_env;
    op.ref = *mutex;
    op.sync_op = SYNC_OP_ATOMIC_NOTIFY;
    op.expected = count;
    sync_ops_push(&op);

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Record an atomic wake - removes waiting sync ops for this mutex.
 */
void insert_sync_op_atomic_wake(wasm_exec_env_t exec_env, const uint32 *mutex) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    uint32 ref = *mutex;

    /* Remove sync ops matching this ref (same as MVVM's std::remove_if) */
    int write_idx = 0;
    for (int i = 0; i < g_mvvm.sync_ops_count; i++) {
        if (g_mvvm.sync_ops[i].ref != ref) {
            if (write_idx != i)
                g_mvvm.sync_ops[write_idx] = g_mvvm.sync_ops[i];
            write_idx++;
        }
    }
    g_mvvm.sync_ops_count = write_idx;

    (void)exec_env;
    pthread_mutex_unlock(&g_mvvm.mtx);
}

/* ==========================================================================
 * Thread Management
 * ========================================================================== */

/**
 * Map old thread ID to new thread ID during migration/restore.
 */
void wamr_handle_map(uint64_t old_tid, uint64_t new_tid) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    /* Update existing entry or add new one */
    for (int i = 0; i < g_mvvm.tid_map_count; i++) {
        if (g_mvvm.tid_map[i].old_tid == old_tid) {
            g_mvvm.tid_map[i].new_tid = new_tid;
            pthread_mutex_unlock(&g_mvvm.mtx);
            return;
        }
    }

    if (g_mvvm.tid_map_count < MVVM_MAX_TID_MAP) {
        g_mvvm.tid_map[g_mvvm.tid_map_count].old_tid = old_tid;
        g_mvvm.tid_map[g_mvvm.tid_map_count].new_tid = new_tid;
        g_mvvm.tid_map[g_mvvm.tid_map_count].in_use = true;
        g_mvvm.tid_map_count++;
    }

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Look up the new thread ID for a given old thread ID.
 */
korp_tid wamr_get_new_korp_tid(korp_tid tid) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    uint64_t tid_val = (uint64_t)tid;
    for (int i = 0; i < g_mvvm.tid_map_count; i++) {
        if (g_mvvm.tid_map[i].old_tid == tid_val) {
            korp_tid result = (korp_tid)g_mvvm.tid_map[i].new_tid;
            pthread_mutex_unlock(&g_mvvm.mtx);
            return result;
        }
    }

    pthread_mutex_unlock(&g_mvvm.mtx);
    return tid; /* Return original if no mapping found */
}

/**
 * Look up thread ID mapping (same as wamr_get_new_korp_tid).
 */
korp_tid wamr_get_korp_tid(korp_tid tid) {
    return wamr_get_new_korp_tid(tid);
}

/**
 * Record parent-child thread relationship.
 */
void insert_parent_child(uint64_t parent_tid, uint64_t child_tid) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    if (g_mvvm.parent_child_count < MVVM_MAX_TID_MAP) {
        g_mvvm.parent_child[g_mvvm.parent_child_count].parent_tid = parent_tid;
        g_mvvm.parent_child[g_mvvm.parent_child_count].child_tid = child_tid;
        g_mvvm.parent_child[g_mvvm.parent_child_count].in_use = true;
        g_mvvm.parent_child_count++;
    }

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Record thread start arguments for replay.
 */
void insert_tid_start_arg(uint64_t tid, size_t arg1, size_t arg2) {
    (void)tid;
    (void)arg1;
    (void)arg2;
    /* Thread start args are tracked but not needed until full restore */
}

/* ==========================================================================
 * Checkpoint / Restore Core
 * ========================================================================== */

/**
 * Serialize execution state to file.
 * Called from the interpreter loop at checkpoint points.
 * The actual serialization is delegated to the Rust layer via callback.
 */
void serialize_to_file(WASMExecEnv *exec_env) {
    (void)exec_env;
    /*
     * In the Wasmer integration, serialization is driven by the Rust layer.
     * When this is called, it means the interpreter hit a checkpoint point.
     * The Rust layer should poll for checkpoint requests and call the
     * appropriate serialization routines.
     *
     * TODO: Implement callback to Rust checkpoint handler
     */
    if (checkpoint) {
        fprintf(stderr, "mvvm: serialize_to_file called (checkpoint pending)\n");
    }
}

/**
 * Enter a lightweight checkpoint region.
 * Increments the checkpoint counter for this thread, indicating
 * it's at a safe point for checkpointing.
 */
void lightweight_checkpoint(WASMExecEnv *exec_env) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    uint64 tid = (uint64)(uintptr_t)exec_env;

    /* Find or allocate lwcp slot for this tid */
    int slot = -1;
    for (int i = 0; i < g_mvvm.lwcp_count; i++) {
        if (g_mvvm.lwcp_tids[i] == tid) {
            slot = i;
            break;
        }
    }
    if (slot < 0 && g_mvvm.lwcp_count < MVVM_MAX_TID_MAP) {
        slot = g_mvvm.lwcp_count++;
        g_mvvm.lwcp_tids[slot] = tid;
        g_mvvm.lwcp_counts[slot] = 0;
    }

    if (slot >= 0) {
        g_mvvm.lwcp_counts[slot]++;
        g_mvvm.ready++;
    }

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Exit a lightweight checkpoint region.
 * Decrements the checkpoint counter. If the counter was reset to 0
 * by the checkpoint thread (meaning we were serialized), block
 * until the checkpoint completes.
 */
void lightweight_uncheckpoint(WASMExecEnv *exec_env) {
    mvvm_ensure_init();
    pthread_mutex_lock(&g_mvvm.mtx);

    uint64 tid = (uint64)(uintptr_t)exec_env;

    for (int i = 0; i < g_mvvm.lwcp_count; i++) {
        if (g_mvvm.lwcp_tids[i] == tid) {
            if (g_mvvm.lwcp_counts[i] == 0) {
                /* Counter was reset by checkpoint - we were serialized.
                 * In the full MVVM implementation, this blocks forever
                 * (the process is being migrated). For now, just return. */
                pthread_mutex_unlock(&g_mvvm.mtx);
                return;
            }
            g_mvvm.lwcp_counts[i]--;
            g_mvvm.ready--;
            break;
        }
    }

    pthread_mutex_unlock(&g_mvvm.mtx);
}

/**
 * Restore execution environment from checkpoint.
 * Called during thread creation in restore mode.
 */
void restore_env(void) {
    /* Restoration is driven by the Rust layer */
}

/**
 * Wait for migration checkpoint to complete.
 * Called by child threads during restore.
 */
void wamr_wait(wasm_exec_env_t exec_env) {
    (void)exec_env;
    /* In the embedded context, waiting is coordinated by the Rust layer */
}

/* ==========================================================================
 * Signal Handlers
 * ========================================================================== */

static void mvvm_sigtrap_handler(int sig) {
    (void)sig;
    checkpoint = true;
}

static void mvvm_sigint_handler(int sig) {
    (void)sig;
    checkpoint = true;
}

/**
 * Register SIGTRAP handler for checkpoint triggers.
 */
void register_sigtrap(void) {
#if !defined(_WIN32)
    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sigemptyset(&sa.sa_mask);
    sa.sa_handler = mvvm_sigtrap_handler;
    sa.sa_flags = SA_RESTART;
    sigaction(SIGTRAP, &sa, NULL);
#endif
}

/**
 * Register SIGINT handler.
 */
void register_sigint(void) {
#if !defined(_WIN32)
    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sigemptyset(&sa.sa_mask);
    sa.sa_handler = mvvm_sigint_handler;
    sa.sa_flags = SA_RESTART;
    sigaction(SIGINT, &sa, NULL);
#endif
}

void sigtrap_handler(int sig) { mvvm_sigtrap_handler(sig); }
void sigint_handler(int sig) { mvvm_sigint_handler(sig); }

/* ==========================================================================
 * Utility Functions
 * ========================================================================== */

/**
 * Restart execution after migration (multi-threaded).
 */
void restart_execution(uint32 targs) {
    (void)targs;
}

/**
 * Check if atomic operations are at a checkpointable state.
 */
bool is_atomic_checkpointable(void) {
    return true;
}

void print_memory(WASMExecEnv *exec_env) { (void)exec_env; }
void print_exec_env_debug_info(WASMExecEnv *exec_env) { (void)exec_env; }
