/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#[macro_use]
extern crate mozjs;
extern crate libc;

use mozjs::jsapi::CompartmentOptions;
use mozjs::jsapi::JSAutoCompartment;
use mozjs::jsapi::JSClass;
use mozjs::jsapi::JSCLASS_FOREGROUND_FINALIZE;
use mozjs::jsapi::JSClassOps;
use mozjs::jsapi::JSFreeOp;
use mozjs::jsapi::JS_NewGlobalObject;
use mozjs::jsapi::JS_NewObject;
use mozjs::jsapi::JSObject;
use mozjs::jsapi::OnNewGlobalHookOption;
use mozjs::rust::{Runtime, SIMPLE_GLOBAL_CLASS};
use std::ptr;

#[test]
fn runtime() {
    unsafe {
        let runtime = Runtime::new().unwrap();

        let cx = runtime.cx();
        let h_option = OnNewGlobalHookOption::FireOnNewGlobalHook;
        let c_option = CompartmentOptions::default();

        rooted!(in(cx) let global = JS_NewGlobalObject(cx,
                                                       &SIMPLE_GLOBAL_CLASS,
                                                       ptr::null_mut(),
                                                       h_option,
                                                       &c_option));
        let _ac = JSAutoCompartment::new(cx, global.get());
        rooted!(in(cx) let _object = JS_NewObject(cx, &CLASS as *const _));
    }

    assert!(Runtime::new().is_err());
}

unsafe extern fn finalize(_fop: *mut JSFreeOp, _object: *mut JSObject) {
    assert!(!Runtime::get().is_null());
}

static CLASS_OPS: JSClassOps = JSClassOps {
    addProperty: None,
    delProperty: None,
    enumerate: None,
    newEnumerate: None,
    resolve: None,
    mayResolve: None,
    finalize: Some(finalize),
    call: None,
    hasInstance: None,
    construct: None,
    trace: None,
};

static CLASS: JSClass = JSClass {
    name: b"EventTargetPrototype\0" as *const u8 as *const libc::c_char,
    flags: JSCLASS_FOREGROUND_FINALIZE,
    cOps: &CLASS_OPS as *const JSClassOps,
    reserved: [0 as *mut _; 3]
};
