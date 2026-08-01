//! In-process bindings to the game's exported Mono C API.
//!
//! We do not link against Mono — the game already has `mono-2.0-bdwgc.dll`
//! mapped, so we resolve the exports we need with `GetModuleHandle` +
//! `GetProcAddress` at runtime. Mono handles (`MonoDomain`, `MonoImage`,
//! `MonoClass`, `MonoMethod`, ...) are opaque pointers passed straight back to
//! Mono, exactly as CE's collector does — we never read their internal layout,
//! which is what keeps this version-agnostic across Unity/Mono releases.

use std::ffi::{CString, c_char, c_int, c_void};

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

/// Module names Unity ships its Mono runtime under, newest-first. Unity renamed
/// the embedded runtime to `mono-2.0-bdwgc.dll`; older builds used `mono.dll`.
const MONO_MODULE_NAMES: &[&[u8]] = &[
    b"mono-2.0-bdwgc.dll\0",
    b"mono.dll\0",
    b"monosgen-2.0.dll\0",
];

// Opaque Mono pointers. Treated as black boxes — never dereferenced.
type MonoDomain = *mut c_void;
type MonoAssembly = *mut c_void;
type MonoImage = *mut c_void;
type MonoClass = *mut c_void;
type MonoMethod = *mut c_void;
type MonoMethodDesc = *mut c_void;
type MonoJitInfo = *mut c_void;

type FnGetRootDomain = unsafe extern "C" fn() -> MonoDomain;
type FnThreadAttach = unsafe extern "C" fn(MonoDomain) -> *mut c_void;
type FnAssemblyForeach = unsafe extern "C" fn(GFunc, *mut c_void);
type FnAssemblyGetImage = unsafe extern "C" fn(MonoAssembly) -> MonoImage;
type FnImageGetName = unsafe extern "C" fn(MonoImage) -> *const c_char;
type FnClassFromName = unsafe extern "C" fn(MonoImage, *const c_char, *const c_char) -> MonoClass;
type FnClassGetMethodFromName = unsafe extern "C" fn(MonoClass, *const c_char, c_int) -> MonoMethod;
type FnMethodDescNew = unsafe extern "C" fn(*const c_char, c_int) -> MonoMethodDesc;
type FnMethodDescSearchInImage = unsafe extern "C" fn(MonoMethodDesc, MonoImage) -> MonoMethod;
type FnMethodDescFree = unsafe extern "C" fn(MonoMethodDesc);
type FnCompileMethod = unsafe extern "C" fn(MonoMethod) -> *mut c_void;
type FnMethodGetClass = unsafe extern "C" fn(MonoMethod) -> MonoClass;
type FnClassIsGeneric = unsafe extern "C" fn(MonoClass) -> c_int;
type FnJitInfoTableFind = unsafe extern "C" fn(MonoDomain, *mut c_void) -> MonoJitInfo;
type FnJitInfoGetMethod = unsafe extern "C" fn(MonoJitInfo) -> MonoMethod;
type FnJitInfoGetCodeStart = unsafe extern "C" fn(MonoJitInfo) -> *mut c_void;
type FnJitInfoGetCodeSize = unsafe extern "C" fn(MonoJitInfo) -> c_int;

/// The `GFunc` Mono passes each assembly to during `mono_assembly_foreach`.
type GFunc = unsafe extern "C" fn(*mut c_void, *mut c_void);

/// Resolved Mono entry points. Required functions are non-optional; functions
/// Mono doesn't always export (`mono_class_is_generic`) are `Option`.
pub struct MonoApi {
    handle: HMODULE,
    get_root_domain: FnGetRootDomain,
    thread_attach: FnThreadAttach,
    assembly_foreach: FnAssemblyForeach,
    assembly_get_image: FnAssemblyGetImage,
    image_get_name: FnImageGetName,
    class_from_name: FnClassFromName,
    class_get_method_from_name: FnClassGetMethodFromName,
    method_desc_new: FnMethodDescNew,
    method_desc_search_in_image: FnMethodDescSearchInImage,
    method_desc_free: FnMethodDescFree,
    compile_method: FnCompileMethod,
    method_get_class: FnMethodGetClass,
    class_is_generic: Option<FnClassIsGeneric>,
    jit_info_table_find: FnJitInfoTableFind,
    jit_info_get_method: FnJitInfoGetMethod,
    jit_info_get_code_start: FnJitInfoGetCodeStart,
    jit_info_get_code_size: FnJitInfoGetCodeSize,
}

/// Locate a loaded module by any of the candidate names.
unsafe fn find_module() -> Option<HMODULE> {
    for name in MONO_MODULE_NAMES {
        let h = unsafe { GetModuleHandleA(name.as_ptr()) };
        if !h.is_null() {
            return Some(h);
        }
    }
    None
}

/// Resolve a required export, transmuting the raw `FARPROC` to the typed fn
/// pointer. Returns `None` if the symbol is missing so [`MonoApi::load`] can
/// fail cleanly instead of resolving to a null call later.
unsafe fn sym(handle: HMODULE, name: &[u8]) -> Option<*const c_void> {
    debug_assert_eq!(
        name.last(),
        Some(&0),
        "GetProcAddress needs a NUL-terminated name"
    );
    let p = unsafe { GetProcAddress(handle, name.as_ptr()) };
    p.map(|f| f as *const c_void)
}

impl MonoApi {
    /// Find the in-process Mono runtime and resolve the export subset we need.
    /// Returns `None` if Mono isn't loaded yet or a required export is absent
    /// (e.g. the target is an il2cpp build, which exposes a different API).
    ///
    /// # Safety
    /// Must run in the target process with Mono mapped. The resolved pointers
    /// are only valid for that process's lifetime.
    pub unsafe fn load() -> Option<Self> {
        let handle = unsafe { find_module()? };

        macro_rules! req {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { sym(handle, $name)? };
                unsafe { std::mem::transmute::<*const c_void, $ty>(p) }
            }};
        }

        let api = MonoApi {
            handle,
            get_root_domain: req!(b"mono_get_root_domain\0", FnGetRootDomain),
            thread_attach: req!(b"mono_thread_attach\0", FnThreadAttach),
            assembly_foreach: req!(b"mono_assembly_foreach\0", FnAssemblyForeach),
            assembly_get_image: req!(b"mono_assembly_get_image\0", FnAssemblyGetImage),
            image_get_name: req!(b"mono_image_get_name\0", FnImageGetName),
            class_from_name: req!(b"mono_class_from_name\0", FnClassFromName),
            class_get_method_from_name: req!(
                b"mono_class_get_method_from_name\0",
                FnClassGetMethodFromName
            ),
            method_desc_new: req!(b"mono_method_desc_new\0", FnMethodDescNew),
            method_desc_search_in_image: req!(
                b"mono_method_desc_search_in_image\0",
                FnMethodDescSearchInImage
            ),
            method_desc_free: req!(b"mono_method_desc_free\0", FnMethodDescFree),
            compile_method: req!(b"mono_compile_method\0", FnCompileMethod),
            method_get_class: req!(b"mono_method_get_class\0", FnMethodGetClass),
            class_is_generic: unsafe { sym(handle, b"mono_class_is_generic\0") }
                .map(|p| unsafe { std::mem::transmute::<*const c_void, FnClassIsGeneric>(p) }),
            jit_info_table_find: req!(b"mono_jit_info_table_find\0", FnJitInfoTableFind),
            jit_info_get_method: req!(b"mono_jit_info_get_method\0", FnJitInfoGetMethod),
            jit_info_get_code_start: req!(b"mono_jit_info_get_code_start\0", FnJitInfoGetCodeStart),
            jit_info_get_code_size: req!(b"mono_jit_info_get_code_size\0", FnJitInfoGetCodeSize),
        };
        Some(api)
    }

    /// The loaded Mono module handle, returned to the client by `InitMono`.
    pub fn module_handle(&self) -> u64 {
        self.handle as u64
    }

    /// Attach the calling thread to Mono's root domain. Required before any
    /// other Mono call from a thread Mono didn't create (our server thread).
    ///
    /// # Safety
    /// Mono must be initialised in-process.
    pub unsafe fn attach_current_thread(&self) {
        unsafe {
            let domain = (self.get_root_domain)();
            (self.thread_attach)(domain);
        }
    }

    /// `mono_get_root_domain()` as an opaque handle.
    ///
    /// # Safety
    /// Mono must be initialised in-process.
    pub unsafe fn root_domain(&self) -> u64 {
        unsafe { (self.get_root_domain)() as u64 }
    }

    /// Enumerate `(image_handle, image_name)` for every loaded assembly.
    ///
    /// # Safety
    /// Mono must be initialised and the calling thread attached.
    pub unsafe fn enum_images(&self) -> Vec<(u64, String)> {
        // mono_assembly_foreach hands each assembly to a C callback; collect
        // them into a thread-local-free Vec via a context pointer.
        struct Ctx {
            api: *const MonoApi,
            out: Vec<(u64, String)>,
        }

        unsafe extern "C" fn collect(assembly: *mut c_void, user: *mut c_void) {
            unsafe {
                let ctx = &mut *(user as *mut Ctx);
                let api = &*ctx.api;
                let image = (api.assembly_get_image)(assembly);
                if image.is_null() {
                    return;
                }
                let name_ptr = (api.image_get_name)(image);
                let name = if name_ptr.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(name_ptr)
                        .to_string_lossy()
                        .into_owned()
                };
                ctx.out.push((image as u64, name));
            }
        }

        let mut ctx = Ctx {
            api: self,
            out: Vec::new(),
        };
        unsafe {
            (self.assembly_foreach)(collect, (&mut ctx as *mut Ctx).cast());
        }
        ctx.out
    }

    /// `mono_class_from_name(image, namespace, class)`.
    ///
    /// # Safety
    /// `image` must be a valid handle from [`Self::enum_images`].
    pub unsafe fn find_class(&self, image: u64, namespace: &str, class: &str) -> u64 {
        let (Ok(ns), Ok(cn)) = (CString::new(namespace), CString::new(class)) else {
            return 0;
        };
        unsafe { (self.class_from_name)(image as MonoImage, ns.as_ptr(), cn.as_ptr()) as u64 }
    }

    /// `mono_class_get_method_from_name(class, method, -1)` (any arg count).
    ///
    /// # Safety
    /// `class` must be a valid handle from [`Self::find_class`].
    pub unsafe fn find_method(&self, class: u64, method: &str) -> u64 {
        let Ok(mn) = CString::new(method) else {
            return 0;
        };
        unsafe { (self.class_get_method_from_name)(class as MonoClass, mn.as_ptr(), -1) as u64 }
    }

    /// Resolve `"Namespace.Class:Method"` within one image via method-desc.
    ///
    /// # Safety
    /// `image` must be a valid handle from [`Self::enum_images`].
    pub unsafe fn find_method_by_desc(&self, image: u64, desc: &str) -> u64 {
        let Ok(c) = CString::new(desc) else {
            return 0;
        };
        unsafe {
            let md = (self.method_desc_new)(c.as_ptr(), 1);
            if md.is_null() {
                return 0;
            }
            let method = (self.method_desc_search_in_image)(md, image as MonoImage);
            (self.method_desc_free)(md);
            method as u64
        }
    }

    /// JIT-compile a method and return its native code address. Mirrors CE's
    /// guard: generic classes can't be compiled blind, so skip them (the caller
    /// gets 0 and can fall back to a concrete instantiation strategy later).
    ///
    /// # Safety
    /// `method` must be a valid handle from a find call.
    pub unsafe fn compile_method(&self, method: u64) -> u64 {
        if method == 0 {
            return 0;
        }
        unsafe {
            let class = (self.method_get_class)(method as MonoMethod);
            if !class.is_null()
                && let Some(is_generic) = self.class_is_generic
                && is_generic(class) != 0
            {
                return 0;
            }
            (self.compile_method)(method as MonoMethod) as u64
        }
    }

    /// Reverse lookup: find the JIT info covering `address`. Returns
    /// `(jit_info, method, code_start, code_size)`, all zero when not found.
    ///
    /// # Safety
    /// Mono must be initialised; `domain` 0 means the root domain.
    pub unsafe fn jit_info(&self, domain: u64, address: u64) -> (u64, u64, u64, u32) {
        unsafe {
            let domain = if domain == 0 {
                (self.get_root_domain)()
            } else {
                domain as MonoDomain
            };
            let ji = (self.jit_info_table_find)(domain, address as *mut c_void);
            if ji.is_null() {
                return (0, 0, 0, 0);
            }
            (
                ji as u64,
                (self.jit_info_get_method)(ji) as u64,
                (self.jit_info_get_code_start)(ji) as u64,
                (self.jit_info_get_code_size)(ji) as u32,
            )
        }
    }
}
