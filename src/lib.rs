/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this file,
 * You can obtain one at http://mozilla.org/MPL/2.0/. */

#![crate_name = "js"]
#![crate_type = "rlib"]

#![feature(filling_drop)]
#![feature(link_args)]
#![feature(const_fn)]

#![allow(drop_with_repr_extern)]
#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, improper_ctypes)]

#[macro_use]
extern crate heapsize;
#[macro_use]
extern crate lazy_static;
extern crate libc;
#[macro_use]
extern crate log;
extern crate mozjs_sys;
extern crate num_traits;

pub mod jsapi {
    use libc::FILE;

    #[repr(C)]
    #[derive(Debug)]
    pub struct Heap<T: ::rust::GCMethods<T> + Copy> {
        pub ptr: ::std::cell::UnsafeCell<T>,
    }

    unsafe impl Sync for JSClass {}

    // With MSVC 2013, char16_t isn't a native type,
    // so it gets put in the bindings output.
    #[cfg(target_os = "windows")]
    #[cfg(target_env = "msvc")]
    pub type char16_t = ::std::os::raw::c_ushort;

    #[cfg(target_os = "linux")]
    #[cfg(target_pointer_width = "64")]
    #[cfg(not(feature = "debugmozjs"))]
    include!("jsapi_linux_64.rs");

    #[cfg(target_os = "linux")]
    #[cfg(target_pointer_width = "64")]
    #[cfg(feature = "debugmozjs")]
    include!("jsapi_linux_64_debug.rs");

    #[cfg(target_os = "macos")]
    #[cfg(target_pointer_width = "64")]
    #[cfg(not(feature = "debugmozjs"))]
    include!("jsapi_macos_64.rs");

    #[cfg(target_os = "macos")]
    #[cfg(target_pointer_width = "64")]
    #[cfg(feature = "debugmozjs")]
    include!("jsapi_macos_64_debug.rs");

    #[cfg(target_os = "windows")]
    #[cfg(target_env = "gnu")]
    #[cfg(target_pointer_width = "64")]
    #[cfg(not(feature = "debugmozjs"))]
    include!("jsapi_windows_gcc_64.rs");

    #[cfg(target_os = "windows")]
    #[cfg(target_env = "gnu")]
    #[cfg(target_pointer_width = "64")]
    #[cfg(feature = "debugmozjs")]
    include!("jsapi_windows_gcc_64_debug.rs");

    #[cfg(target_os = "windows")]
    #[cfg(target_env = "msvc")]
    #[cfg(target_pointer_width = "64")]
    #[cfg(not(feature = "debugmozjs"))]
    include!("jsapi_windows_msvc14_64.rs");

    #[cfg(target_os = "windows")]
    #[cfg(target_env = "msvc")]
    #[cfg(target_pointer_width = "64")]
    #[cfg(feature = "debugmozjs")]
    include!("jsapi_windows_msvc14_64_debug.rs");

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[cfg(target_pointer_width = "32")]
    #[cfg(not(feature = "debugmozjs"))]
    include!("jsapi_linux_32.rs");

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[cfg(target_pointer_width = "32")]
    #[cfg(feature = "debugmozjs")]
    include!("jsapi_linux_32_debug.rs");
}

#[macro_use]
pub mod rust;

mod consts;
pub mod conversions;
pub mod error;
pub mod glue;
pub mod jsval;


pub use consts::*;

use heapsize::HeapSizeOf;
use jsapi::{JSContext, Heap};
use jsval::JSVal;
use rust::GCMethods;

#[inline(always)]
pub unsafe fn JS_ARGV(_cx: *mut JSContext, vp: *mut JSVal) -> *mut JSVal {
    vp.offset(2)
}

#[inline(always)]
pub unsafe fn JS_CALLEE(_cx: *mut JSContext, vp: *mut JSVal) -> JSVal {
    *vp
}

// This is measured properly by the heap measurement implemented in SpiderMonkey.
impl<T: Copy + GCMethods<T>> HeapSizeOf for Heap<T> {
    fn heap_size_of_children(&self) -> usize {
        0
    }
}
known_heap_size!(0, JSVal);

impl jsapi::ObjectOpResult {
    /// Set this ObjectOpResult to true and return true.
    pub fn succeed(&mut self) -> bool {
        self.code_ = jsapi::ObjectOpResult_SpecialCodes::OkCode as usize;
        true
    }
}
