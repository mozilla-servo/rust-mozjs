/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this file,
 * You can obtain one at http://mozilla.org/MPL/2.0/. */

#![crate_name = "js"]
#![crate_type = "rlib"]

#![feature(core_intrinsics)]
#![feature(link_args)]
#![feature(str_utf16)]
#![feature(unsafe_no_drop_flag)]
#![feature(const_fn)]

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, improper_ctypes, raw_pointer_derive)]

extern crate libc;
#[macro_use]
extern crate log;
#[macro_use]
extern crate heapsize;
extern crate rustc_serialize as serialize;
extern crate mozjs_sys;

#[cfg(target_os = "linux")]
#[cfg(target_pointer_width = "64")]
mod jsapi_linux_64;

#[cfg(target_os = "macos")]
#[cfg(target_pointer_width = "64")]
mod jsapi_macos_64;

#[cfg(target_os = "windows")]
#[cfg(target_pointer_width = "64")]
mod jsapi_windows_gcc_64;

#[cfg(not(target_os = "windows"))]
#[cfg(target_pointer_width = "32")]
mod jsapi_linux_32;

pub mod jsapi {
    #[cfg(target_os = "linux")]
    #[cfg(target_pointer_width = "64")]
    pub use jsapi_linux_64::*;

    #[cfg(target_os = "macos")]
    #[cfg(target_pointer_width = "64")]
    pub use jsapi_macos_64::*;

    #[cfg(target_os = "windows")]
    #[cfg(target_pointer_width = "64")]
    pub use jsapi_windows_gcc_64::*;

    #[cfg(not(target_os = "windows"))]
    #[cfg(target_pointer_width = "32")]
    pub use jsapi_linux_32::*;
}

pub mod rust;
pub mod glue;
pub mod jsval;

use jsapi::{JSContext, JSProtoKey, Heap};
use jsval::JSVal;
use rust::GCMethods;

use libc::c_uint;

use heapsize::HeapSizeOf;

pub const default_heapsize: u32 = 32_u32 * 1024_u32 * 1024_u32;
pub const default_stacksize: usize = 8192;

pub const JSID_TYPE_STRING: i64 = 0;
pub const JSID_TYPE_INT: i64 = 1;
pub const JSID_TYPE_VOID: i64 = 2;
pub const JSID_TYPE_OBJECT: i64 = 4;
pub const JSID_TYPE_DEFAULT_XML_NAMESPACE: i64 = 6;
pub const JSID_TYPE_MASK: i64 = 7;

pub const JSFUN_CONSTRUCTOR: u32 = 0x400; /* native that can be called as a ctor */

pub const JSPROP_ENUMERATE: c_uint = 0x01;
pub const JSPROP_READONLY: c_uint  = 0x02;
pub const JSPROP_PERMANENT: c_uint = 0x04;
pub const JSPROP_GETTER: c_uint = 0x10;
pub const JSPROP_SETTER: c_uint = 0x20;
pub const JSPROP_SHARED: c_uint =    0x40;
pub const JSPROP_NATIVE_ACCESSORS: c_uint = 0x08;

pub const JSCLASS_RESERVED_SLOTS_SHIFT: c_uint = 8;
pub const JSCLASS_RESERVED_SLOTS_WIDTH: c_uint = 8;
pub const JSCLASS_RESERVED_SLOTS_MASK: c_uint = ((1 << JSCLASS_RESERVED_SLOTS_WIDTH) - 1) as c_uint;

pub const JSCLASS_HIGH_FLAGS_SHIFT: c_uint =
    JSCLASS_RESERVED_SLOTS_SHIFT + JSCLASS_RESERVED_SLOTS_WIDTH;
pub const JSCLASS_IS_GLOBAL: c_uint = 1 << (JSCLASS_HIGH_FLAGS_SHIFT + 1);
pub const JSCLASS_GLOBAL_APPLICATION_SLOTS: c_uint = 4;
pub const JSCLASS_GLOBAL_SLOT_COUNT: c_uint = JSCLASS_GLOBAL_APPLICATION_SLOTS + JSProtoKey::JSProto_LIMIT as u32 * 3 + 31;

pub const JSCLASS_IS_DOMJSCLASS: u32 = 1 << 4;
pub const JSCLASS_IMPLEMENTS_BARRIERS: u32 = 1 << 5;
pub const JSCLASS_USERBIT1: u32 = 1 << 7;

pub const JSCLASS_IS_PROXY: u32 = 1 << (JSCLASS_HIGH_FLAGS_SHIFT+4);

pub const JSSLOT_PROXY_PRIVATE: u32 = 1;

pub const JS_DEFAULT_ZEAL_FREQ: u32 = 100;

pub const JSITER_ENUMERATE: c_uint   = 0x1;
pub const JSITER_FOREACH: c_uint     = 0x2;
pub const JSITER_KEYVALUE: c_uint    = 0x4;
pub const JSITER_OWNONLY: c_uint     = 0x8;
pub const JSITER_HIDDEN: c_uint      = 0x10;
pub const JSITER_SYMBOLS: c_uint     = 0x20;
pub const JSITER_SYMBOLSONLY: c_uint = 0x40;

#[link(name = "jsglue")]
extern { }

#[cfg(target_os = "android")]
#[link(name = "stdc++")]
extern { }

#[cfg(target_os = "android")]
#[link(name = "gcc")]
extern { }

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

