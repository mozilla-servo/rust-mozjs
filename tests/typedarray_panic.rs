/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#[macro_use]
extern crate mozjs;

use mozjs::jsapi::CompartmentOptions;
use mozjs::jsapi::JSAutoCompartment;
use mozjs::jsapi::JSObject;
use mozjs::jsapi::JS_NewGlobalObject;
use mozjs::jsapi::OnNewGlobalHookOption;
use mozjs::rust::Runtime as Runtime_;
use mozjs::rust::SIMPLE_GLOBAL_CLASS;
use mozjs::typedarray::{CreateWith, Uint32Array};
use std::ptr;

#[test]
#[should_panic]
fn typedarray_update_panic() {
    let rt = Runtime_::new().unwrap();
    let cx = rt.cx();

    unsafe {
        rooted!(in(cx) let global =
            JS_NewGlobalObject(cx, &SIMPLE_GLOBAL_CLASS, ptr::null_mut(),
                               OnNewGlobalHookOption::FireOnNewGlobalHook,
                               &CompartmentOptions::default())
        );

        let _ac = JSAutoCompartment::new(cx, global.get());
        rooted!(in(cx) let mut rval = ptr::null_mut::<JSObject>());
        let _ = Uint32Array::create(cx, CreateWith::Slice(&[1, 2, 3, 4, 5]), rval.handle_mut());
        typedarray!(in(cx) let mut array: Uint32Array = rval.get());
        array.as_mut().unwrap().update(&[0, 2, 4, 6, 8, 10]);
    }
}
