/// Utilities to convert between `wamr` and `wasmer` values
use crate::{
    BackendFunction, Function, Value,
    wamr::{
        bindings::{
            self, wasm_func_as_ref, wasm_val_t, wasm_val_t__bindgen_ty_1, wasm_valkind_t,
            wasm_valtype_kind, wasm_valtype_t,
        },
        function,
    },
};
use wasmer_types::Type;

pub trait IntoCApiValue {
    /// Consume [`self`] to produce a [`wasm_val_t`].
    fn into_cv(self) -> wasm_val_t;
}

impl IntoCApiValue for Value {
    fn into_cv(self) -> wasm_val_t {
        match self {
            // MVVM's WAMR fork has a simpler wasm_val_t without _paddings field
            #[cfg(feature = "mvvm")]
            Self::I32(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_I32 as _,
                of: wasm_val_t__bindgen_ty_1 { i32_: val },
            },
            #[cfg(not(feature = "mvvm"))]
            Self::I32(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_I32 as _,
                _paddings: Default::default(),
                of: wasm_val_t__bindgen_ty_1 { i32_: val },
            },
            #[cfg(feature = "mvvm")]
            Self::I64(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_I64 as _,
                of: wasm_val_t__bindgen_ty_1 { i64_: val },
            },
            #[cfg(not(feature = "mvvm"))]
            Self::I64(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_I64 as _,
                _paddings: Default::default(),
                of: wasm_val_t__bindgen_ty_1 { i64_: val },
            },
            #[cfg(feature = "mvvm")]
            Self::F32(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_F32 as _,
                of: wasm_val_t__bindgen_ty_1 { f32_: val },
            },
            #[cfg(not(feature = "mvvm"))]
            Self::F32(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_F32 as _,
                _paddings: Default::default(),
                of: wasm_val_t__bindgen_ty_1 { f32_: val },
            },
            #[cfg(feature = "mvvm")]
            Self::F64(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_F64 as _,
                of: wasm_val_t__bindgen_ty_1 { f64_: val },
            },
            #[cfg(not(feature = "mvvm"))]
            Self::F64(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_F64 as _,
                _paddings: Default::default(),
                of: wasm_val_t__bindgen_ty_1 { f64_: val },
            },
            #[cfg(feature = "mvvm")]
            Self::FuncRef(Some(val)) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_FUNCREF as _,
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(val.as_wamr().handle) },
                },
            },
            #[cfg(not(feature = "mvvm"))]
            Self::FuncRef(Some(val)) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_FUNCREF as _,
                _paddings: Default::default(),
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(val.as_wamr().handle) },
                },
            },
            #[cfg(feature = "mvvm")]
            Self::FuncRef(None) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_FUNCREF as _,
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(std::ptr::null_mut()) },
                },
            },
            #[cfg(not(feature = "mvvm"))]
            Self::FuncRef(None) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_FUNCREF as _,
                _paddings: Default::default(),
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(std::ptr::null_mut()) },
                },
            },
            Self::ExternRef(_) => panic!(
                "Creating host values from guest ExternRefs is not currently supported in wamr ."
            ),
            Self::ExceptionRef(_) => {
                panic!("Creating host values from guest V128s is not currently supported in wamr.")
            }
            Self::V128(_) => {
                panic!("Creating host values from guest V128s is not currently supported in wamr.")
            }
        }
    }
}

pub trait IntoWasmerValue {
    /// Consume [`self`] to produce a [`Value`].
    fn into_wv(self) -> Value;
}

impl IntoWasmerValue for wasm_val_t {
    fn into_wv(self) -> Value {
        match self.kind as _ {
            bindings::wasm_valkind_enum_WASM_I32 => Value::I32(unsafe { self.of.i32_ }),
            bindings::wasm_valkind_enum_WASM_I64 => Value::I64(unsafe { self.of.i64_ }),
            bindings::wasm_valkind_enum_WASM_F32 => Value::F32(unsafe { self.of.f32_ }),
            bindings::wasm_valkind_enum_WASM_F64 => Value::F64(unsafe { self.of.f64_ }),
            bindings::wasm_valkind_enum_WASM_FUNCREF => Value::FuncRef(Some(Function(
                BackendFunction::Wamr(crate::backend::wamr::function::Function {
                    handle: unsafe { self.of.ref_ as _ },
                }),
            ))),
            // MVVM uses ANYREF instead of EXTERNREF
            #[cfg(feature = "mvvm")]
            bindings::wasm_valkind_enum_WASM_ANYREF => {
                panic!("ExternRefs are not currently supported through wasm_c_api")
            }
            #[cfg(not(feature = "mvvm"))]
            bindings::wasm_valkind_enum_WASM_EXTERNREF => {
                panic!("ExternRefs are not currently supported through wasm_c_api")
            }
            _ => panic!("wamr kind {} has no matching type", self.kind),
        }
    }
}

pub trait IntoWasmerType {
    /// Consume [`self`] to produce a [`Type`].
    fn into_wt(self) -> Type;
}

impl IntoWasmerType for wasm_valkind_t {
    fn into_wt(self) -> Type {
        match self as _ {
            bindings::wasm_valkind_enum_WASM_I32 => Type::I32,
            bindings::wasm_valkind_enum_WASM_I64 => Type::I64,
            bindings::wasm_valkind_enum_WASM_F32 => Type::F32,
            bindings::wasm_valkind_enum_WASM_F64 => Type::F64,
            // MVVM doesn't have V128 support
            #[cfg(not(feature = "mvvm"))]
            bindings::wasm_valkind_enum_WASM_V128 => Type::V128,
            // MVVM uses ANYREF instead of EXTERNREF
            #[cfg(feature = "mvvm")]
            bindings::wasm_valkind_enum_WASM_ANYREF => Type::ExternRef,
            #[cfg(not(feature = "mvvm"))]
            bindings::wasm_valkind_enum_WASM_EXTERNREF => Type::ExternRef,
            bindings::wasm_valkind_enum_WASM_FUNCREF => Type::FuncRef,
            _ => unreachable!("wamr kind {self:?} has no matching wasmer_types::Type"),
        }
    }
}

pub trait IntoCApiType {
    /// Consume [`self`] to produce a [`wasm_valkind_t`].
    fn into_ct(self) -> wasm_valkind_t;
}

impl IntoCApiType for Type {
    fn into_ct(self) -> wasm_valkind_t {
        match self {
            Self::I32 => bindings::wasm_valkind_enum_WASM_I32 as _,
            Self::I64 => bindings::wasm_valkind_enum_WASM_I64 as _,
            Self::F32 => bindings::wasm_valkind_enum_WASM_F32 as _,
            Self::F64 => bindings::wasm_valkind_enum_WASM_F64 as _,
            Self::FuncRef => bindings::wasm_valkind_enum_WASM_FUNCREF as _,
            // MVVM uses ANYREF instead of EXTERNREF
            #[cfg(feature = "mvvm")]
            Self::ExternRef => bindings::wasm_valkind_enum_WASM_ANYREF as _,
            #[cfg(not(feature = "mvvm"))]
            Self::ExternRef => bindings::wasm_valkind_enum_WASM_EXTERNREF as _,
            // MVVM doesn't have V128 support
            #[cfg(feature = "mvvm")]
            Self::V128 => panic!("MVVM does not support V128/SIMD"),
            #[cfg(not(feature = "mvvm"))]
            Self::V128 => bindings::wasm_valkind_enum_WASM_V128 as _,
            Self::ExceptionRef => panic!("wamr currently does not support exnrefs"),
        }
    }
}

impl IntoWasmerType for wasm_valtype_t {
    fn into_wt(self) -> Type {
        let type_: wasm_valkind_t = unsafe { wasm_valtype_kind(&self as *const _) };
        type_.into_wt()
    }
}

impl IntoWasmerType for *const wasm_valtype_t {
    fn into_wt(self) -> Type {
        let type_: wasm_valkind_t = unsafe { wasm_valtype_kind(self as _) };
        type_.into_wt()
    }
}

impl IntoWasmerType for *mut wasm_valtype_t {
    fn into_wt(self) -> Type {
        let type_: wasm_valkind_t = unsafe { wasm_valtype_kind(self as _) };
        type_.into_wt()
    }
}
