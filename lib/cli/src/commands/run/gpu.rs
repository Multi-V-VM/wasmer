use std::sync::Arc;

use anyhow::{Context, Error, bail};
use wasmer::{
    ExternType, Function, FunctionEnv, FunctionEnvMut, Imports, Instance, Memory, Module,
    RuntimeError, Store, Value,
};
use wasmer_types::ModuleHash;
use wasmer_wasix::{Runtime, WasiError};

use super::Run;

const CUDA_SUCCESS: i32 = 0;
const CUDA_ERROR_INVALID_VALUE: i32 = 1;
const CUDA_MEMCPY_HOST_TO_HOST: i32 = 0;
const CUDA_MEMCPY_HOST_TO_DEVICE: i32 = 1;
const CUDA_MEMCPY_DEVICE_TO_HOST: i32 = 2;
const CUDA_MEMCPY_DEVICE_TO_DEVICE: i32 = 3;
const CUBLAS_OP_N: i32 = 0;

type AppleSgemmFn = unsafe extern "C" fn(
    *const f32,
    *const f32,
    *const f32,
    *mut f32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    f32,
    f32,
) -> i32;

#[derive(Debug)]
struct CudaBridge {
    memory: Option<Memory>,
    allocations: std::collections::HashMap<u32, Vec<u8>>,
    next_device_ptr: u32,
    next_cublas_handle: u32,
}

impl CudaBridge {
    fn new() -> Self {
        Self {
            memory: None,
            allocations: std::collections::HashMap::new(),
            next_device_ptr: 0x10000,
            next_cublas_handle: 0xc0010000,
        }
    }

    fn allocate_device_ptr(&mut self, size: usize) -> Option<u32> {
        let ptr = self.next_device_ptr;
        let step = ((size.max(1) + 15) & !15).max(16);
        let step = u32::try_from(step).ok()?;
        self.next_device_ptr = self.next_device_ptr.checked_add(step)?;
        self.allocations.insert(ptr, vec![0; size]);
        Some(ptr)
    }

    fn allocate_cublas_handle(&mut self) -> Option<u32> {
        let handle = self.next_cublas_handle;
        self.next_cublas_handle = self.next_cublas_handle.checked_add(1)?;
        Some(handle)
    }
}

pub(super) fn module_has_cuda_imports(module: &Module) -> bool {
    module.imports().any(|import| {
        import.module() == "env"
            && matches!(
                import.name(),
                "cudaMalloc"
                    | "cudaFree"
                    | "cudaMemcpy"
                    | "cudaDeviceSynchronize"
                    | "cublasCreate_v2"
                    | "cublasDestroy_v2"
                    | "cublasSgemm_v2"
            )
    })
}

pub(super) fn execute_wasi_module(
    run: &Run,
    program_name: String,
    module: Module,
    module_hash: ModuleHash,
    runtime: Arc<dyn Runtime + Send + Sync>,
) -> Result<(), Error> {
    if module_has_imported_memory(&module) {
        bail!("CUDA bridge currently supports modules that export their linear memory");
    }

    let mut store = runtime.new_store();
    let mut builder = run
        .wasi
        .prepare(&module, program_name, run.args.clone(), runtime)
        .context("Unable to prepare the WASI environment for CUDA execution")?;
    builder.set_module_hash(module_hash);

    let mut wasi_env = builder
        .finalize(&mut store)
        .context("Unable to finalize the WASI environment for CUDA execution")?;
    let mut imports = wasi_env
        .import_object_for_all_wasi_versions(&mut store, &module)
        .context("Unable to create WASI imports for CUDA execution")?;

    let cuda_env = FunctionEnv::new(&mut store, CudaBridge::new());
    define_cuda_imports(&mut store, &mut imports, &cuda_env);

    let instance = Instance::new(&mut store, &module, &imports)
        .context("Unable to instantiate CUDA-enabled WASI module")?;
    let memory = instance
        .exports
        .get_memory("memory")
        .context("CUDA-enabled WASI module must export memory")?
        .clone();

    cuda_env.as_mut(&mut store).memory = Some(memory);
    wasi_env
        .initialize(&mut store, instance.clone())
        .context("Unable to initialize CUDA-enabled WASI module")?;

    match run.invoke.as_deref() {
        Some(entry) => invoke_explicit_entry(&instance, &mut store, entry, &run.args),
        None => invoke_start(&instance, &mut store),
    }
}

fn module_has_imported_memory(module: &Module) -> bool {
    module
        .imports()
        .any(|import| matches!(import.ty(), ExternType::Memory(_)))
}

fn define_cuda_imports(store: &mut Store, imports: &mut Imports, env: &FunctionEnv<CudaBridge>) {
    imports.define(
        "env",
        "cudaMalloc",
        Function::new_typed_with_env(store, env, cuda_malloc),
    );
    imports.define(
        "env",
        "cudaFree",
        Function::new_typed_with_env(store, env, cuda_free),
    );
    imports.define(
        "env",
        "cudaMemcpy",
        Function::new_typed_with_env(store, env, cuda_memcpy),
    );
    imports.define(
        "env",
        "cudaDeviceSynchronize",
        Function::new_typed_with_env(store, env, cuda_device_synchronize),
    );
    imports.define(
        "env",
        "cublasCreate_v2",
        Function::new_typed_with_env(store, env, cublas_create_v2),
    );
    imports.define(
        "env",
        "cublasDestroy_v2",
        Function::new_typed_with_env(store, env, cublas_destroy_v2),
    );
    imports.define(
        "env",
        "cublasSgemm_v2",
        Function::new_typed_with_env(store, env, cublas_sgemm_v2),
    );
}

fn invoke_start(instance: &Instance, store: &mut Store) -> Result<(), Error> {
    let start = instance
        .exports
        .get_function("_start")
        .context("The module doesn't export a \"_start\" function")?;

    handle_runtime_result(start.call(store, &[]))
}

fn invoke_explicit_entry(
    instance: &Instance,
    store: &mut Store,
    entry: &str,
    args: &[String],
) -> Result<(), Error> {
    let function = instance
        .exports
        .get_function(entry)
        .with_context(|| format!("The module doesn't export a function named \"{entry}\""))?;
    let result = super::invoke_function(instance, store, function, args)?;

    match result {
        Ok(return_values) => {
            println!(
                "{}",
                return_values
                    .iter()
                    .map(|val| val.to_string())
                    .collect::<Vec<String>>()
                    .join(" ")
            );
            Ok(())
        }
        Err(err) => handle_runtime_error(err),
    }
}

fn handle_runtime_result(result: Result<Box<[Value]>, RuntimeError>) -> Result<(), Error> {
    match result {
        Ok(_) => Ok(()),
        Err(err) => handle_runtime_error(err),
    }
}

fn handle_runtime_error(err: RuntimeError) -> Result<(), Error> {
    if let Some(exit_code) = runtime_exit_code(&err) {
        if exit_code.raw() == 0 {
            return Ok(());
        }
        return Err(WasiError::Exit(exit_code).into());
    }

    Err(err.into())
}

fn runtime_exit_code(
    err: &(dyn std::error::Error + 'static),
) -> Option<wasmer_wasix::types::wasi::ExitCode> {
    if let Some(exit_code) = super::get_exit_code(err) {
        return Some(exit_code);
    }

    let mut source = err.source();
    while let Some(err) = source {
        if let Some(exit_code) = super::get_exit_code(err) {
            return Some(exit_code);
        }
        source = err.source();
    }

    None
}

fn read_guest_memory(
    ctx: &mut FunctionEnvMut<CudaBridge>,
    ptr: i32,
    len: usize,
) -> Option<Vec<u8>> {
    if ptr < 0 {
        return None;
    }
    let memory = ctx.data().memory.clone()?;
    let view = memory.view(&*ctx);
    let mut bytes = vec![0; len];
    view.read(ptr as u64, &mut bytes).ok()?;
    Some(bytes)
}

fn write_guest_memory(ctx: &mut FunctionEnvMut<CudaBridge>, ptr: i32, bytes: &[u8]) -> Option<()> {
    if ptr < 0 {
        return None;
    }
    let memory = ctx.data().memory.clone()?;
    let view = memory.view(&*ctx);
    view.write(ptr as u64, bytes).ok()?;
    Some(())
}

fn write_guest_u32(ctx: &mut FunctionEnvMut<CudaBridge>, ptr: i32, value: u32) -> Option<()> {
    write_guest_memory(ctx, ptr, &value.to_le_bytes())
}

fn read_guest_f32(ctx: &mut FunctionEnvMut<CudaBridge>, ptr: i32) -> Option<f32> {
    let bytes = read_guest_memory(ctx, ptr, 4)?;
    Some(f32::from_le_bytes(bytes.try_into().ok()?))
}

fn device_allocation_bytes(ctx: &FunctionEnvMut<CudaBridge>, ptr: i32) -> Option<Vec<u8>> {
    if ptr < 0 {
        return None;
    }
    ctx.data().allocations.get(&(ptr as u32)).cloned()
}

fn device_allocation_f32s(ctx: &FunctionEnvMut<CudaBridge>, ptr: i32) -> Option<Vec<f32>> {
    let bytes = device_allocation_bytes(ctx, ptr)?;
    if bytes.len() % 4 != 0 {
        return None;
    }

    let mut values = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        values.push(f32::from_le_bytes(chunk.try_into().ok()?));
    }
    Some(values)
}

fn f32s_to_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[cfg(unix)]
fn resolve_apple_sgemm(symbol: &[u8]) -> Option<AppleSgemmFn> {
    let resolved = unsafe { libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr() as *const _) };
    if resolved.is_null() {
        return None;
    }

    Some(unsafe { std::mem::transmute::<*mut libc::c_void, AppleSgemmFn>(resolved) })
}

#[cfg(not(unix))]
fn resolve_apple_sgemm(_symbol: &[u8]) -> Option<AppleSgemmFn> {
    None
}

fn try_apple_sgemm(
    m: usize,
    n: usize,
    k: usize,
    lda: usize,
    ldb: usize,
    ldc: usize,
    alpha: f32,
    a: &[f32],
    b: &[f32],
    beta: f32,
    c: &[f32],
) -> Option<Vec<f32>> {
    if std::env::var("WASM_CUDA_ACCEL").ok().as_deref() != Some("1") {
        return None;
    }

    let m_i32 = i32::try_from(m).ok()?;
    let n_i32 = i32::try_from(n).ok()?;
    let k_i32 = i32::try_from(k).ok()?;
    let lda_i32 = i32::try_from(lda).ok()?;
    let ldb_i32 = i32::try_from(ldb).ok()?;
    let ldc_i32 = i32::try_from(ldc).ok()?;

    let backend = std::env::var("WASM_CUDA_BACKEND").unwrap_or_else(|_| "metal".to_string());
    let candidates: &[&[u8]] = match backend.as_str() {
        "ane" => &[b"codifyone_ane_sgemm\0", b"codifyone_metal_sgemm\0"],
        "metal" => &[b"codifyone_metal_sgemm\0"],
        _ => return None,
    };

    for candidate in candidates {
        let Some(sgemm) = resolve_apple_sgemm(candidate) else {
            continue;
        };

        let mut output = c.to_vec();
        let rc = unsafe {
            sgemm(
                a.as_ptr(),
                b.as_ptr(),
                c.as_ptr(),
                output.as_mut_ptr(),
                m_i32,
                n_i32,
                k_i32,
                lda_i32,
                ldb_i32,
                ldc_i32,
                alpha,
                beta,
            )
        };

        if rc == CUDA_SUCCESS {
            return Some(output);
        }
    }

    None
}

fn checked_positive_i32(value: i32) -> Option<usize> {
    if value < 0 {
        return None;
    }
    usize::try_from(value).ok()
}

fn cuda_malloc(mut ctx: FunctionEnvMut<CudaBridge>, dev_ptr: i32, size: i32) -> i32 {
    let size = match checked_positive_i32(size) {
        Some(size) => size,
        None => return CUDA_ERROR_INVALID_VALUE,
    };

    let ptr = match ctx.data_mut().allocate_device_ptr(size) {
        Some(ptr) => ptr,
        None => return CUDA_ERROR_INVALID_VALUE,
    };

    if write_guest_u32(&mut ctx, dev_ptr, ptr).is_none() {
        ctx.data_mut().allocations.remove(&ptr);
        return CUDA_ERROR_INVALID_VALUE;
    }

    CUDA_SUCCESS
}

fn cuda_free(mut ctx: FunctionEnvMut<CudaBridge>, dev_ptr: i32) -> i32 {
    if dev_ptr == 0 {
        return CUDA_SUCCESS;
    }
    if dev_ptr < 0 {
        return CUDA_ERROR_INVALID_VALUE;
    }

    ctx.data_mut().allocations.remove(&(dev_ptr as u32));
    CUDA_SUCCESS
}

fn cuda_memcpy(
    mut ctx: FunctionEnvMut<CudaBridge>,
    dst: i32,
    src: i32,
    size: i32,
    kind: i32,
) -> i32 {
    let size = match checked_positive_i32(size) {
        Some(size) => size,
        None => return CUDA_ERROR_INVALID_VALUE,
    };

    match kind {
        CUDA_MEMCPY_HOST_TO_HOST => {
            let bytes = match read_guest_memory(&mut ctx, src, size) {
                Some(bytes) => bytes,
                None => return CUDA_ERROR_INVALID_VALUE,
            };
            if write_guest_memory(&mut ctx, dst, &bytes).is_none() {
                return CUDA_ERROR_INVALID_VALUE;
            }
        }
        CUDA_MEMCPY_HOST_TO_DEVICE => {
            let bytes = match read_guest_memory(&mut ctx, src, size) {
                Some(bytes) => bytes,
                None => return CUDA_ERROR_INVALID_VALUE,
            };
            let Some(allocation) = ctx.data_mut().allocations.get_mut(&(dst as u32)) else {
                return CUDA_ERROR_INVALID_VALUE;
            };
            if allocation.len() < size {
                return CUDA_ERROR_INVALID_VALUE;
            }
            allocation[..size].copy_from_slice(&bytes);
        }
        CUDA_MEMCPY_DEVICE_TO_HOST => {
            let Some(bytes) = device_allocation_bytes(&ctx, src) else {
                return CUDA_ERROR_INVALID_VALUE;
            };
            if bytes.len() < size {
                return CUDA_ERROR_INVALID_VALUE;
            }
            if write_guest_memory(&mut ctx, dst, &bytes[..size]).is_none() {
                return CUDA_ERROR_INVALID_VALUE;
            }
        }
        CUDA_MEMCPY_DEVICE_TO_DEVICE => {
            let Some(bytes) = device_allocation_bytes(&ctx, src) else {
                return CUDA_ERROR_INVALID_VALUE;
            };
            let Some(allocation) = ctx.data_mut().allocations.get_mut(&(dst as u32)) else {
                return CUDA_ERROR_INVALID_VALUE;
            };
            if bytes.len() < size || allocation.len() < size {
                return CUDA_ERROR_INVALID_VALUE;
            }
            allocation[..size].copy_from_slice(&bytes[..size]);
        }
        _ => return CUDA_ERROR_INVALID_VALUE,
    }

    CUDA_SUCCESS
}

fn cuda_device_synchronize(_ctx: FunctionEnvMut<CudaBridge>) -> i32 {
    CUDA_SUCCESS
}

fn cublas_create_v2(mut ctx: FunctionEnvMut<CudaBridge>, handle_ptr: i32) -> i32 {
    let handle = match ctx.data_mut().allocate_cublas_handle() {
        Some(handle) => handle,
        None => return CUDA_ERROR_INVALID_VALUE,
    };

    if write_guest_u32(&mut ctx, handle_ptr, handle).is_none() {
        return CUDA_ERROR_INVALID_VALUE;
    }

    CUDA_SUCCESS
}

fn cublas_destroy_v2(_ctx: FunctionEnvMut<CudaBridge>, _handle: i32) -> i32 {
    CUDA_SUCCESS
}

#[allow(clippy::too_many_arguments)]
fn cublas_sgemm_v2(
    mut ctx: FunctionEnvMut<CudaBridge>,
    handle: i32,
    transa: i32,
    transb: i32,
    m: i32,
    n: i32,
    k: i32,
    alpha_ptr: i32,
    a_ptr: i32,
    lda: i32,
    b_ptr: i32,
    ldb: i32,
    beta_ptr: i32,
    c_ptr: i32,
    ldc: i32,
) -> i32 {
    if handle == 0 || transa != CUBLAS_OP_N || transb != CUBLAS_OP_N {
        return CUDA_ERROR_INVALID_VALUE;
    }

    let (m, n, k, lda, ldb, ldc) = match (
        checked_positive_i32(m),
        checked_positive_i32(n),
        checked_positive_i32(k),
        checked_positive_i32(lda),
        checked_positive_i32(ldb),
        checked_positive_i32(ldc),
    ) {
        (Some(m), Some(n), Some(k), Some(lda), Some(ldb), Some(ldc)) => (m, n, k, lda, ldb, ldc),
        _ => return CUDA_ERROR_INVALID_VALUE,
    };

    if lda < m || ldb < k || ldc < m {
        return CUDA_ERROR_INVALID_VALUE;
    }

    let alpha = match read_guest_f32(&mut ctx, alpha_ptr) {
        Some(alpha) => alpha,
        None => return CUDA_ERROR_INVALID_VALUE,
    };
    let beta = match read_guest_f32(&mut ctx, beta_ptr) {
        Some(beta) => beta,
        None => return CUDA_ERROR_INVALID_VALUE,
    };
    let a = match device_allocation_f32s(&ctx, a_ptr) {
        Some(a) => a,
        None => return CUDA_ERROR_INVALID_VALUE,
    };
    let b = match device_allocation_f32s(&ctx, b_ptr) {
        Some(b) => b,
        None => return CUDA_ERROR_INVALID_VALUE,
    };
    let mut c = match device_allocation_f32s(&ctx, c_ptr) {
        Some(c) => c,
        None => return CUDA_ERROR_INVALID_VALUE,
    };

    if let Some(accelerated) = try_apple_sgemm(m, n, k, lda, ldb, ldc, alpha, &a, &b, beta, &c) {
        c = accelerated;
    } else {
        for col in 0..n {
            for row in 0..m {
                let mut sum = 0.0f32;
                for q in 0..k {
                    let Some(a_index) = row.checked_add(q.saturating_mul(lda)) else {
                        return CUDA_ERROR_INVALID_VALUE;
                    };
                    let Some(b_index) = q.checked_add(col.saturating_mul(ldb)) else {
                        return CUDA_ERROR_INVALID_VALUE;
                    };
                    if a_index >= a.len() || b_index >= b.len() {
                        return CUDA_ERROR_INVALID_VALUE;
                    }
                    sum += a[a_index] * b[b_index];
                }

                let Some(c_index) = row.checked_add(col.saturating_mul(ldc)) else {
                    return CUDA_ERROR_INVALID_VALUE;
                };
                if c_index >= c.len() {
                    return CUDA_ERROR_INVALID_VALUE;
                }
                c[c_index] = alpha.mul_add(sum, beta * c[c_index]);
            }
        }
    }

    let bytes = f32s_to_bytes(&c);
    let Some(allocation) = ctx.data_mut().allocations.get_mut(&(c_ptr as u32)) else {
        return CUDA_ERROR_INVALID_VALUE;
    };
    if allocation.len() < bytes.len() {
        return CUDA_ERROR_INVALID_VALUE;
    }
    allocation[..bytes.len()].copy_from_slice(&bytes);

    CUDA_SUCCESS
}
