#[cfg(feature = "wamr")]
fn build_wamr() {
    use bindgen::callbacks::ParseCallbacks;
    const WAMR_ZIP: &str =
        "https://github.com/bytecodealliance/wasm-micro-runtime/archive/refs/tags/WAMR-2.2.0.zip";
    const ZIP_NAME: &str = "wasm-micro-runtime-WAMR-2.2.0";

    use cmake::Config;
    use std::{env, path::PathBuf};

    let crate_root = env::var("OUT_DIR").unwrap();

    // Read target os from cargo env
    // Transform from cargo value to valid wasm-micro-runtime os
    let target_os = match env::var("CARGO_CFG_TARGET_OS").unwrap().as_str() {
        "linux" => "linux",
        "windows" => "windows",
        "macos" => "darwin",
        "freebsd" => "freebsd",
        "android" => "android",
        "ios" => "ios",
        "ios-sim" => "ios",
        other => panic!("Unsupported CARGO_CFG_TARGET_OS: {other}"),
    };

    // Read target arch from cargo env
    // Transform from cargo value to valid wasm-micro-runtime WAMR_BUILD_TARGET
    let target_arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "x86" => "X86_32",
        "x86_64" => "X86_64",
        "arm" => "ARM",
        "aarch64" => "AARCH64",
        "mips" => "MIPS",
        "powerpc" => "POWERPC",
        "powerpc64" => "POWERPC64",
        other => panic!("Unsupported CARGO_CFG_TARGET_ARCH: {other}"),
    };

    // Cleanup tmp data from prior builds
    let wamr_dir = PathBuf::from(&crate_root).join("third_party/wamr");
    if !wamr_dir.exists() {
        let zip_dir = PathBuf::from(&crate_root).join("third_party");
        let _ = std::fs::remove_dir_all(&wamr_dir);
        let _ = std::fs::remove_dir_all(&zip_dir);

        // Fetch & extract wasm-micro-runtime source
        let zip_data = ureq::get(WAMR_ZIP)
            .call()
            .expect("failed to download wamr")
            .body_mut()
            .with_config()
            .limit(50 * 1024 * 1024) // 50MB
            .read_to_vec()
            .expect("failed to download wamr");
        std::fs::create_dir_all(&zip_dir)
            .expect("Failed to create temporary zip extraction directory");
        zip::read::ZipArchive::new(std::io::Cursor::new(zip_data))
            .expect("failed to open wamr zip file")
            .extract(&zip_dir)
            .expect("failed to extract wamr zip file");
        let _ = std::fs::remove_dir_all(&wamr_dir);
        std::fs::rename(zip_dir.join(ZIP_NAME), &wamr_dir)
            .unwrap_or_else(|e| panic!("failed to rename wamr dir: {zip_dir:?} due to: {e:?}"));
    } else {
        println!("cargo::rerun-if-changed={}", wamr_dir.display());
    }

    let wamr_platform_dir = wamr_dir.join("product-mini/platforms").join(target_os);
    let mut dst = Config::new(wamr_platform_dir.as_path());

    dst.always_configure(true)
        .generator("Ninja")
        .no_build_target(true)
        .define(
            "CMAKE_BUILD_TYPE",
            if cfg!(debug_assertions) {
                "RelWithDebInfo"
            } else {
                "Release"
            },
        )
        .define("CMAKE_POLICY_VERSION_MINIMUM", "3.5")
        .define("WAMR_BUILD_AOT", "0")
        //.define("WAMR_BUILD_TAIL_CALL", "1")
        //.define("WAMR_BUILD_DUMP_CALL_STACK", "1")
        // .define("WAMR_BUILD_CUSTOM_NAME_SECTION", "1")
        // .define("WAMR_BUILD_LOAD_CUSTOM_SECTION", "1")
        .define("WAMR_BUILD_BULK_MEMORY", "1")
        .define("WAMR_BUILD_REF_TYPES", "1")
        .define("WAMR_BUILD_SIMD", "1")
        .define("WAMR_BUILD_FAST_INTERP", "1")
        .define("WAMR_BUILD_LIB_PTHREAD", "1")
        .define("WAMR_BUILD_LIB_WASI_THREADS", "0")
        .define("WAMR_BUILD_LIBC_WASI", "0")
        .define("WAMR_BUILD_LIBC_BUILTIN", "0")
        .define("WAMR_BUILD_SHARED_MEMORY", "1")
        .define("WAMR_BUILD_MULTI_MODULE", "1")
        .define("WAMR_DISABLE_HW_BOUND_CHECK", "1")
        .define("WAMR_BUILD_TARGET", target_arch);

    if target_os == "windows" {
        dst.define("CMAKE_CXX_COMPILER", "cl.exe");
        dst.define("CMAKE_C_COMPILER", "cl.exe");
        dst.define("CMAKE_LINKER_TYPE", "MSVC");
        dst.define("WAMR_BUILD_PLATFORM", "windows");
        dst.define("WAMR_BUILD_LIBC_UVWASI", "0");
    }

    if target_os == "ios" || target_os == "ios-sim" {
        // XXX: Hacky
        //
        // Compiling wamr targeting `aarch64-apple-ios` results in
        //
        // ```
        //  clang: error: unsupported option '-mfloat-abi=' for target 'aarch64-apple-ios'
        // ```
        // So, here, we simply remove that setting.
        //
        // See: https://github.com/bytecodealliance/wasm-micro-runtime/pull/3889
        let mut lines = vec![];
        let cmake_file_path = wamr_platform_dir.join("CMakeLists.txt");
        for line in std::fs::read_to_string(&cmake_file_path).unwrap().lines() {
            if !line.contains("-mfloat-abi=hard") {
                lines.push(line.to_string())
            }
        }
        std::fs::write(cmake_file_path, lines.join("\n")).unwrap();
    }

    let dst = dst.build();

    // Check output of `cargo build --verbose`, should see something like:
    // -L native=/path/runng/target/debug/build/runng-sys-abc1234/out
    // That contains output from cmake

    // Rename the symbols created from wamr.
    static mut WAMR_RENAMED: Vec<(String, String)> = vec![];

    #[derive(Debug)]
    struct WamrRenamer {}
    impl ParseCallbacks for WamrRenamer {
        /// This function will run for every extern variable and function. The returned value determines
        /// the link name in the bindings.
        fn generated_link_name_override(
            &self,
            item_info: bindgen::callbacks::ItemInfo<'_>,
        ) -> Option<String> {
            if item_info.name.starts_with("wasm") {
                let new_name = format!("wamr_{}", item_info.name);
                // TODO: refactor to not use static mut
                #[allow(
                    static_mut_refs,
                    reason = "existing behaviour that was disallowed by edition 2024"
                )]
                unsafe {
                    WAMR_RENAMED.push((item_info.name.to_string(), new_name.clone()));
                }
                Some(new_name)
            } else {
                None
            }
        }
    }

    let mut builder = bindgen::Builder::default()
        .header(
            wamr_dir
                .join("core/iwasm/include/wasm_c_api.h")
                .to_str()
                .unwrap(),
        )
        .derive_default(true)
        .derive_debug(true)
        .parse_callbacks(Box::new(WamrRenamer {}));

    // Add iOS SDK include paths for bindgen
    if target_os == "ios" || target_os == "ios-sim" {
        let sdk_path = env::var("SDKROOT")
            .unwrap_or_else(|_| "/Applications/Xcode.app/Contents/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk".to_string());
        builder = builder
            .clang_arg(format!("-isysroot{}", sdk_path))
            .clang_arg(format!("--target={}", env::var("TARGET").unwrap()));
    }

    let bindings = builder
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bindings_path = out_path.join("wamr_bindings.rs");
    bindings
        .write_to_file(&bindings_path)
        .expect("Couldn't write bindings");

    let objcopy_names = ["llvm-objcopy", "objcopy", "gobjcopy"];

    let mut objcopy = None;
    for n in objcopy_names {
        if which::which(n).is_ok() {
            objcopy = Some(n);
            break;
        }
    }

    if objcopy.is_none() {
        panic!(
            "No program akin to `objcopy` found\nI searched for these programs in your path: {}",
            objcopy_names.join(", ")
        );
    }

    let objcopy = objcopy.unwrap();

    unsafe {
        // TODO: refactor to not use static mut
        #[allow(
            static_mut_refs,
            reason = "existing behaviour that was disallowed by edition 2024"
        )]
        let syms: Vec<String> = WAMR_RENAMED
            .iter()
            .map(|(old, new)|
                // A bit hacky: we need a way to figure out if we're going to target a Mach-O
                // library or an ELF one to take care of the "_" in front of symbols.
            {
                if cfg!(any(target_os = "macos", target_os = "ios")) {
                    format!("--redefine-sym=_{old}={new}")
                } else {
                    format!("--redefine-sym={old}={new}")
                }
            })
            .collect();

        // iOS builds produce a dylib in a different location
        let (input_lib, output_lib) = if target_os == "ios" {
            (
                dst.join("build").join("distribution").join("wasm").join("lib").join("libiwasm.dylib"),
                dst.join("build").join("libwamr.dylib"),
            )
        } else {
            (
                dst.join("build").join("libvmlib.a"),
                dst.join("build").join("libwamr.a"),
            )
        };

        let output = std::process::Command::new(objcopy)
            .args(syms)
            .arg(input_lib.display().to_string())
            .arg(output_lib.display().to_string())
            .output()
            .unwrap();

        if !output.status.success() {
            panic!(
                "{objcopy} failed with error code {}: {}",
                output.status,
                String::from_utf8(output.stderr).unwrap()
            );
        }
    }

    println!(
        "cargo:rustc-link-search=native={}",
        dst.join("build").display()
    );
    if target_os == "ios" || target_os == "ios-sim" {
        println!("cargo:rustc-link-lib=dylib=wamr");
    } else {
        println!("cargo:rustc-link-lib=static=wamr");
    }
}

#[cfg(feature = "mvvm")]
fn patch_mvvm_includes(mvvm_dir: &std::path::Path) {
    // MVVM's WAMR fork uses hardcoded relative includes like
    // "../../../../../../../include/wamr_export.h" which only work when
    // building from MVVM's root directory.
    //
    // Fix this by:
    // 1. Copying the MVVM include files to multiple WAMR directories
    // 2. Patching source files to remove the relative paths

    let mvvm_include_dir = mvvm_dir.join("include");

    // Directories where headers need to be accessible
    let target_dirs = [
        mvvm_dir.join("lib/wasm-micro-runtime/core/iwasm/include"),
        mvvm_dir.join("lib/wasm-micro-runtime/core/iwasm/interpreter"),
        mvvm_dir.join("lib/wasm-micro-runtime/core/iwasm/aot"),
        mvvm_dir.join("lib/wasm-micro-runtime/core/iwasm/common"),
        mvvm_dir.join("lib/wasm-micro-runtime/core/shared/platform/include"),
    ];

    // Copy MVVM headers to WAMR directories
    let headers_to_copy = ["wamr_export.h", "mvvm_export.h"];

    for header in headers_to_copy {
        let src = mvvm_include_dir.join(header);
        if src.exists() {
            for target_dir in &target_dirs {
                let dst = target_dir.join(header);
                if !dst.exists() {
                    if let Err(e) = std::fs::copy(&src, &dst) {
                        println!("cargo:warning=Failed to copy {} to {}: {}", header, target_dir.display(), e);
                    }
                }
            }
        }
    }

    // Patch source files to replace relative includes with simple includes
    // Search for all .c and .h files that might have the problematic include
    patch_relative_includes_recursive(mvvm_dir, "lib/wasm-micro-runtime");

    // Fix the static/non-static declaration mismatch (must be called once, not recursively)
    fix_wasm_interp_header(mvvm_dir);
}

#[cfg(feature = "mvvm")]
fn patch_relative_includes_recursive(base_dir: &std::path::Path, subdir: &str) {
    let dir = base_dir.join(subdir);
    if !dir.exists() {
        return;
    }

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Recurse into subdirectories
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let new_subdir = format!("{}/{}", subdir, name);
                patch_relative_includes_recursive(base_dir, &new_subdir);
            }
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "c" || ext == "h" {
                // Check and patch this file
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let mut patched = content.clone();

                    // Replace various relative path patterns
                    let patterns = [
                        "../../../../../../../include/wamr_export.h",
                        "../../../../../../include/wamr_export.h",
                        "../../../../../include/wamr_export.h",
                        "../../../../include/wamr_export.h",
                        "../../../include/wamr_export.h",
                    ];

                    for pattern in patterns {
                        patched = patched.replace(
                            &format!("#include \"{}\"", pattern),
                            "#include \"wamr_export.h\"",
                        );
                    }

                    if patched != content {
                        let _ = std::fs::write(&path, patched);
                    }
                }
            }
        }
    }
}

#[cfg(feature = "mvvm")]
fn fix_wasm_interp_header(mvvm_dir: &std::path::Path) {
    // Fix the static/non-static mismatch in wasm_interp.h
    // When MULTI_MODULE is enabled, wasm_interp_call_func_bytecode is static
    // in the source but declared non-static in the header
    let interp_header = mvvm_dir.join("lib/wasm-micro-runtime/core/iwasm/interpreter/wasm_interp.h");
    if interp_header.exists() {
        if let Ok(content) = std::fs::read_to_string(&interp_header) {
            // Skip if already patched (check for our marker)
            if content.contains("MVVM_PATCHED_STATIC_DECLARATION") {
                return;
            }
            // Use #if 0 instead of C comments to avoid nested comment issues
            let patched = content.replace(
                "void\nwasm_interp_call_func_bytecode(struct WASMModuleInstance *module,\n                               struct WASMExecEnv *exec_env,\n                               struct WASMFunctionInstance *cur_func,\n                               struct WASMInterpFrame *prev_frame);",
                "/* MVVM_PATCHED_STATIC_DECLARATION: disabled conflicting declaration */\n#if 0\nvoid\nwasm_interp_call_func_bytecode(struct WASMModuleInstance *module,\n                               struct WASMExecEnv *exec_env,\n                               struct WASMFunctionInstance *cur_func,\n                               struct WASMInterpFrame *prev_frame);\n#endif",
            );
            if patched != content {
                let _ = std::fs::write(&interp_header, patched);
            }
        }
    }
}

#[cfg(feature = "mvvm")]
fn build_mvvm() {
    use cmake::Config;
    use std::{env, path::PathBuf, process::Command};

    // MVVM is built on top of WAMR with checkpoint/restore support
    const MVVM_REPO: &str = "https://github.com/Multi-V-VM/MVVM.git";

    let crate_root = env::var("OUT_DIR").unwrap();

    // Read target os from cargo env
    let target_os = match env::var("CARGO_CFG_TARGET_OS").unwrap().as_str() {
        "linux" => "linux",
        "windows" => "windows",
        "macos" => "darwin",
        "freebsd" => "freebsd",
        "android" => "android",
        "ios" => "ios",
        "ios-sim" => "ios",
        other => panic!("MVVM unsupported CARGO_CFG_TARGET_OS: {other}"),
    };

    // Read target arch from cargo env
    let target_arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "x86_64" => "X86_64",
        "aarch64" => "AARCH64",
        "riscv64" => "RISCV64",
        other => panic!("MVVM unsupported CARGO_CFG_TARGET_ARCH: {other}"),
    };

    // Clone MVVM with submodules (required for wasm-micro-runtime, s2n-tls, etc.)
    let mvvm_dir = PathBuf::from(&crate_root).join("third_party/mvvm");
    if !mvvm_dir.exists() {
        let third_party_dir = PathBuf::from(&crate_root).join("third_party");
        std::fs::create_dir_all(&third_party_dir)
            .expect("Failed to create third_party directory");

        // Clone with recursive submodules
        let status = Command::new("git")
            .args([
                "clone",
                "--recursive",
                "--depth=1",
                MVVM_REPO,
                mvvm_dir.to_str().unwrap(),
            ])
            .status()
            .expect("Failed to run git clone for MVVM");

        if !status.success() {
            panic!("git clone --recursive failed for MVVM repository");
        }

        // Patch MVVM's WAMR fork to fix hardcoded relative includes
        // These files use "../../../../../../../include/wamr_export.h" which doesn't work
        // when building WAMR standalone. We replace with the proper include.
        patch_mvvm_includes(&mvvm_dir);
    } else {
        println!("cargo::rerun-if-changed={}", mvvm_dir.display());
    }

    // Always apply patches (they're idempotent) — needed even if mvvm_dir already existed
    patch_mvvm_includes(&mvvm_dir);

    // Build MVVM's WAMR fork directly (bypasses main MVVM CMakeLists which enables too many features)
    // The WAMR fork at lib/wasm-micro-runtime has checkpoint/restore support
    let wamr_platform_dir = mvvm_dir
        .join("lib/wasm-micro-runtime/product-mini/platforms")
        .join(target_os);
    let mut dst = Config::new(wamr_platform_dir.as_path());

    // We patch the iOS CMakeLists.txt to rename the 'iwasm' SHARED library
    // to a 'vmlib' STATIC library, so all platforms use the same target name.
    dst.build_target("vmlib");

    dst.always_configure(true)
        .generator("Ninja")
        .define(
            "CMAKE_BUILD_TYPE",
            if cfg!(debug_assertions) {
                "RelWithDebInfo"
            } else {
                "Release"
            },
        )
        .define("CMAKE_POLICY_VERSION_MINIMUM", "3.5")
        // Enable checkpoint/restore support - key for MVVM
        // MVVM requires AOT mode for checkpoint support
        .define("WAMR_BUILD_CHECKPOINT_RESTORE", "1")
        .define("WAMR_BUILD_DUMP_CALL_STACK", "1")
        .define("WAMR_BUILD_CUSTOM_NAME_SECTION", "1")
        // Enable both AOT and interpreter (MVVM checkpoint needs both)
        .define("WAMR_BUILD_AOT", "1")
        .define("WAMR_BUILD_INTERP", "1")
        .define("WAMR_BUILD_JIT", "0")
        .define("WAMR_BUILD_FAST_JIT", "0")
        // Standard WAMR features
        .define("WAMR_BUILD_BULK_MEMORY", "1")
        .define("WAMR_BUILD_REF_TYPES", "1")
        .define("WAMR_BUILD_SIMD", "1")
        .define("WAMR_BUILD_FAST_INTERP", "0")
        .define("WAMR_BUILD_LIB_PTHREAD", "1")
        .define("WAMR_BUILD_LIB_PTHREAD_SEMAPHORE", "0")
        .define("WAMR_BUILD_LIB_WASI_THREADS", "0")
        .define("WAMR_BUILD_LIBC_WASI", "0")
        .define("WAMR_BUILD_LIBC_BUILTIN", "0")
        .define("WAMR_BUILD_SHARED_MEMORY", "1")
        .define("WAMR_BUILD_MULTI_MODULE", "1")
        .define("WAMR_DISABLE_HW_BOUND_CHECK", "1")
        // Disable WASI-NN and other heavy dependencies
        .define("WAMR_BUILD_WASI_NN", "0")
        .define("WAMR_BUILD_WASI_NN_ENABLE_GPU", "0")
        .define("WAMR_BUILD_WASI_NN_EXTERNAL_DELEGATE", "0")
        .define("WAMR_BUILD_WASI_EPHEMERAL_NN", "0")
        // Disable other optional features
        .define("WAMR_BUILD_DEBUG_INTERP", "0")
        .define("WAMR_BUILD_DEBUG_AOT", "0")
        .define("WAMR_BUILD_MINI_LOADER", "0")
        .define("WAMR_BUILD_MEMORY_PROFILING", "0")
        .define("WAMR_BUILD_PERF_PROFILING", "0")
        .define("WAMR_BUILD_GC", "0")
        .define("WAMR_BUILD_STRINGREF", "0")
        .define("WAMR_BUILD_EXCE_HANDLING", "0")
        .define("WAMR_BUILD_TARGET", target_arch);

    if target_os == "windows" {
        dst.define("CMAKE_CXX_COMPILER", "cl.exe");
        dst.define("CMAKE_C_COMPILER", "cl.exe");
        dst.define("CMAKE_LINKER_TYPE", "MSVC");
        dst.define("WAMR_BUILD_PLATFORM", "windows");
    }

    if target_os == "ios" || target_os == "ios-sim" {
        // Remove -mfloat-abi=hard which is unsupported on aarch64-apple-ios
        // See: https://github.com/bytecodealliance/wasm-micro-runtime/pull/3889
        let mut lines = vec![];
        let cmake_file_path = wamr_platform_dir.join("CMakeLists.txt");
        for line in std::fs::read_to_string(&cmake_file_path).unwrap().lines() {
            if line.contains("-mfloat-abi=hard") {
                continue;
            }
            // The iOS CMakeLists builds a SHARED library named 'iwasm', but we
            // need a STATIC library named 'vmlib' to match what darwin/linux
            // produce (and what the rest of build_mvvm expects).
            if line.contains("add_library") && line.contains("iwasm") && line.contains("SHARED") {
                lines.push(line.replace("iwasm", "vmlib").replace("SHARED", "STATIC"));
            } else if line.contains("iwasm") && !line.contains("include") {
                // Replace other references to the iwasm target with vmlib,
                // but preserve include-copy commands that reference iwasm paths.
                lines.push(line.replace("iwasm", "vmlib"));
            } else {
                lines.push(line.to_string());
            }
        }
        std::fs::write(cmake_file_path, lines.join("\n")).unwrap();

        // pthread_jit_write_protect_np is not available on iOS.
        // Replace the call with a no-op on iOS builds.
        let posix_thread_path = mvvm_dir.join(
            "lib/wasm-micro-runtime/core/shared/platform/common/posix/posix_thread.c",
        );
        if posix_thread_path.exists() {
            let content = std::fs::read_to_string(&posix_thread_path).unwrap();
            let patched = content.replace(
                "#if (defined(__APPLE__) || defined(__MACH__)) && defined(__arm64__)\n    pthread_jit_write_protect_np(enabled);",
                "#if (defined(__APPLE__) || defined(__MACH__)) && defined(__arm64__) && !defined(WAMR_BUILD_TARGET_IOS)\n    pthread_jit_write_protect_np(enabled);",
            );
            if patched != content {
                std::fs::write(&posix_thread_path, patched).unwrap();
            }
        }

        // Define WAMR_BUILD_TARGET_IOS so the patched guard above takes effect
        dst.cflag("-DWAMR_BUILD_TARGET_IOS=1");
    }

    // Add MVVM's include directory and WAMR runtime directories to C flags
    // MVVM's WAMR fork has hardcoded relative includes to these paths
    let mvvm_include = mvvm_dir.join("include");
    let aot_runtime_dir = mvvm_dir.join("lib/wasm-micro-runtime/core/iwasm/aot");
    let wamr_include_dir = mvvm_dir.join("lib/wasm-micro-runtime/core/iwasm/include");
    let interp_dir = mvvm_dir.join("lib/wasm-micro-runtime/core/iwasm/interpreter");

    let stubs_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("src/backend/wamr/mvvm_stubs.c");

    let extra_includes = format!(
        "-I{} -I{} -I{} -I{}",
        mvvm_include.display(),
        aot_runtime_dir.display(),
        wamr_include_dir.display(),
        interp_dir.display()
    );
    dst.cflag(&extra_includes);

    let dst = dst.build();

    // Compile mvvm_stubs.c and merge it into libvmlib.a so all MVVM symbols
    // are resolved within a single archive. This avoids cross-library symbol
    // resolution issues (especially when cmake creates shared libraries).
    if stubs_path.exists() {
        cc::Build::new()
            .file(&stubs_path)
            .warnings(false)
            .compile("mvvm_stubs");

        let vmlib_path = dst.join("build").join("libvmlib.a");
        let stubs_lib_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("libmvvm_stubs.a");
        if vmlib_path.exists() && stubs_lib_path.exists() {
            // Merge stubs into vmlib using libtool (macOS/iOS) or ar (Linux)
            let merged_path = dst.join("build").join("libvmlib_merged.a");
            let merge_ok = if cfg!(any(target_os = "macos", target_os = "ios")) {
                Command::new("libtool")
                    .args(["-static", "-o"])
                    .arg(&merged_path)
                    .arg(&vmlib_path)
                    .arg(&stubs_lib_path)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            } else {
                // On Linux, create a thin MRI script to merge archives
                let mri_script = format!(
                    "CREATE {}\nADDLIB {}\nADDLIB {}\nSAVE\nEND\n",
                    merged_path.display(),
                    vmlib_path.display(),
                    stubs_lib_path.display()
                );
                let mri_path = dst.join("build").join("merge.mri");
                std::fs::write(&mri_path, &mri_script).unwrap();
                Command::new("ar")
                    .arg("-M")
                    .stdin(std::fs::File::open(&mri_path).unwrap())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            };
            if merge_ok {
                std::fs::rename(&merged_path, &vmlib_path).unwrap();
            } else {
                panic!("Failed to merge mvvm_stubs into vmlib archive");
            }
        }
    }

    // Generate bindings for MVVM's WAMR
    // MVVM replaces the regular WAMR backend, so we don't need symbol renaming.
    // The Rust code uses wasm_* function names directly, which match the library symbols.
    // This avoids requiring objcopy for symbol renaming (which may not be available
    // on all systems, especially for cross-compilation targets like iOS).

    // Generate bindings for WAMR C API from MVVM's WAMR fork
    // Output to wamr_bindings.rs for compatibility with the WAMR Rust module
    let wasm_c_api_header = mvvm_dir.join("lib/wasm-micro-runtime/core/iwasm/include/wasm_c_api.h");

    let mut builder = bindgen::Builder::default()
        .header(wasm_c_api_header.to_str().unwrap())
        .derive_default(true)
        .derive_debug(true);

    // Add iOS SDK include paths for bindgen
    if target_os == "ios" || target_os == "ios-sim" {
        let sdk_path = env::var("SDKROOT")
            .unwrap_or_else(|_| "/Applications/Xcode.app/Contents/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk".to_string());
        builder = builder
            .clang_arg(format!("-isysroot{}", sdk_path))
            .clang_arg(format!("--target={}", env::var("TARGET").unwrap()));
    }

    let bindings = builder
        .generate()
        .expect("Unable to generate WAMR bindings from MVVM");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    // Use wamr_bindings.rs for compatibility with the WAMR Rust module
    let bindings_path = out_path.join("wamr_bindings.rs");
    bindings
        .write_to_file(&bindings_path)
        .expect("Couldn't write MVVM bindings");

    // Link against vmlib directly (the WAMR static library built by cmake).
    // No symbol renaming needed: MVVM replaces the regular WAMR backend,
    // and the Rust bindings use the original wasm_* symbol names.
    println!(
        "cargo:rustc-link-search=native={}",
        dst.join("build").display()
    );
    println!("cargo:rustc-link-lib=static=vmlib");
}

#[cfg(feature = "v8")]
fn build_v8() {
    use bindgen::callbacks::ParseCallbacks;
    use std::{env, path::PathBuf};

    let url = match (
        env::var("CARGO_CFG_TARGET_OS").unwrap().as_str(),
        env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str(),
        env::var("CARGO_CFG_TARGET_ENV")
            .unwrap_or_default()
            .as_str(),
    ) {
        ("macos", "aarch64", _) => {
            "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.8/wee8-darwin-aarch64.tar.xz"
        }
        ("macos", "x86_64", _) => {
            "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.8/wee8-darwin-amd64.tar.xz"
        }
        ("linux", "x86_64", "gnu") => {
            "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.8/wee8-linux-amd64.tar.xz"
        }
        ("linux", "x86_64", "musl") => {
            "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.8/wee8-linux-musl-amd64.tar.xz"
        }
        ("android", "aarch64", _) => {
            "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.8/wee8-android-arm64.tar.xz"
        }
        // Not supported in 6.0.0-alpha1
        //("windows", "x86_64", _) => "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.7-custom1/wee8-windows-amd64.tar.xz",
        (os, arch, _) => panic!("target os + arch combination not supported: {os}, {arch}"),
    };

    let out_dir = env::var("OUT_DIR").unwrap();
    let crate_root = env::var("CARGO_MANIFEST_DIR").unwrap();
    let v8_header_path = PathBuf::from(&crate_root).join("third-party").join("wee8");

    let tar_data = ureq::get(url)
        .call()
        .expect("failed to download v8")
        .body_mut()
        .with_config()
        .limit(50 * 1024 * 1024) // 50MB
        .read_to_vec()
        .expect("failed to download v8 lib");

    let tar = xz::read::XzDecoder::new(tar_data.as_slice());
    let mut archive = tar::Archive::new(tar);

    for entry in archive.entries().unwrap() {
        eprintln!("entry: {:?}", entry.unwrap().path());
    }

    let tar = xz::read::XzDecoder::new(tar_data.as_slice());
    let mut archive = tar::Archive::new(tar);

    archive.unpack(out_dir.clone()).unwrap();
    println!("cargo:rustc-link-search=native={out_dir}");

    if cfg!(any(target_os = "linux",)) {
        println!("cargo:rustc-link-lib=stdc++");
    } else if cfg!(target_os = "windows") {
        println!("cargo:rustc-link-lib=winmm");
        println!("cargo:rustc-link-lib=dbghelp");
        println!("cargo:rustc-link-lib=shlwapi");
    } else {
        println!("cargo:rustc-link-lib=c++");
    }

    // Rename the symbols created from wee8.
    static mut WEE8_RENAMED: Vec<(String, String)> = vec![];

    #[derive(Debug)]
    struct Wee8Renamer {}
    impl ParseCallbacks for Wee8Renamer {
        /// This function will run for every extern variable and function. The returned value determines
        /// the link name in the bindings.
        fn generated_link_name_override(
            &self,
            item_info: bindgen::callbacks::ItemInfo<'_>,
        ) -> Option<String> {
            if item_info.name.starts_with("wasm") {
                let new_name = format!("wee8_{}", item_info.name);
                // TODO: refactor to not use static mut
                #[allow(
                    static_mut_refs,
                    reason = "existing behaviour that was disallowed by edition 2024"
                )]
                unsafe {
                    WEE8_RENAMED.push((item_info.name.to_string(), new_name.clone()));
                }
                Some(new_name)
            } else {
                None
            }
        }
    }

    let header_path = v8_header_path.join("wasm.h");
    let mut args = vec![];
    if cfg!(target_os = "macos") {
        args.push("-I/usr/local/include");
        args.push("-I/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/usr/include/c++/v1");
        args.push("-I/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/usr/include");
        args.push("-I/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/include");
        args.push("-I/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/System/Library/Frameworks");
    }
    let bindings = bindgen::Builder::default()
        .header(header_path.display().to_string())
        .clang_args(args)
        .derive_default(true)
        .derive_debug(true)
        .parse_callbacks(Box::new(Wee8Renamer {}))
        .generate()
        .expect("Unable to generate bindings for `v8`!");

    let out_path = PathBuf::from(out_dir);

    bindings
        .write_to_file(out_path.join("v8_bindings.rs"))
        .expect("Couldn't write bindings");

    let objcopy_names = ["llvm-objcopy", "objcopy", "gobjcopy"];

    let mut objcopy = None;
    for n in objcopy_names {
        if which::which(n).is_ok() {
            objcopy = Some(n);
            break;
        }
    }

    if objcopy.is_none() {
        panic!(
            "No program akin to `objcopy` found\nI searched for these programs in your path: {}",
            objcopy_names.join(", ")
        );
    }

    let objcopy = objcopy.unwrap();

    // TODO: refactor to not use static mut
    #[allow(
        static_mut_refs,
        reason = "existing behaviour that was disallowed by edition 2024"
    )]
    unsafe {
        let syms: Vec<String> = WEE8_RENAMED
            .iter()
            .map(|(old, new)|
                // A bit hacky: we need a way to figure out if we're going to target a Mach-O
                // library or an ELF one to take care of the "_" in front of symbols.
            {
                if cfg!(any(target_os = "macos", target_os = "ios")) {
                    format!("--redefine-sym=_{old}={new}")
                } else {
                    format!("--redefine-sym={old}={new}")
                }
            })
            .collect();
        let output = dbg!(
            std::process::Command::new(objcopy)
                .args(syms)
                .arg(out_path.join("obj").join("libwee8.a").display().to_string())
                .arg(out_path.join("libwee8prefixed.a").display().to_string())
        )
        .output()
        .unwrap();

        if !output.status.success() {
            panic!(
                "{objcopy} failed with error code {}: {}",
                output.status,
                String::from_utf8(output.stderr).unwrap()
            );
        }
    }

    println!("cargo:rustc-link-lib=static=wee8prefixed");
}

#[cfg(feature = "wasmi")]
fn build_wasmi() {
    use bindgen::callbacks::ParseCallbacks;
    use std::{env, path::PathBuf};

    #[derive(Debug)]
    struct WasmiRenamer {}

    impl ParseCallbacks for WasmiRenamer {
        /// This function will run for every extern variable and function. The returned value determines
        /// the link name in the bindings.
        fn generated_link_name_override(
            &self,
            item_info: bindgen::callbacks::ItemInfo<'_>,
        ) -> Option<String> {
            if item_info.name.starts_with("wasm") {
                let new_name = if cfg!(any(target_os = "macos", target_os =     "ios")) {
                    format!("_wasmi_{}", item_info.name)
                } else {
                    format!("wasmi_{}", item_info.name)
                };

                Some(new_name)
            } else {
                None
            }
        }
    }

    let bindings = bindgen::Builder::default()
        .header(
            PathBuf::from(std::env::var("DEP_WASMI_C_API_INCLUDE").unwrap())
                .join("wasm.h")
                .to_string_lossy(),
        )
        .derive_default(true)
        .derive_debug(true)
        .parse_callbacks(Box::new(WasmiRenamer {}))
        .generate()
        .expect("Unable to generate bindings for `wasmi`!");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bindings_path = out_path.join("wasmi_bindings.rs");
    bindings
        .write_to_file(&bindings_path)
        .expect("Couldn't write bindings");

    let original =
        std::fs::read_to_string(&bindings_path).expect("Failed to read generated wasmi bindings");
    let mut patched = String::with_capacity(original.len());
    for line in original.lines() {
        let trimmed = line.trim_start();
        let indent_len = line.len() - trimmed.len();
        let indent = &line[..indent_len];
        if trimmed.starts_with("extern \"")
            && trimmed.ends_with('{')
            && !trimmed.starts_with("unsafe ")
        {
            patched.push_str(indent);
            patched.push_str("unsafe ");
            patched.push_str(trimmed);
        } else {
            patched.push_str(line);
        }
        patched.push('\n');
    }
    std::fs::write(&bindings_path, patched)
        .expect("Failed to post-process wasmi bindings for Rust 2024");
}
#[allow(unused)]
fn main() {
    // MVVM includes its own modified WAMR fork, so don't build regular WAMR
    // when MVVM is enabled
    #[cfg(all(feature = "wamr", not(feature = "mvvm")))]
    build_wamr();

    #[cfg(feature = "mvvm")]
    build_mvvm();

    #[cfg(feature = "v8")]
    build_v8();

    #[cfg(feature = "wasmi")]
    build_wasmi();
}
