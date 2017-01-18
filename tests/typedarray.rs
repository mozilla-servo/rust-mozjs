/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#[macro_use]
extern crate js;

use js::jsapi::CompartmentOptions;
use js::jsapi::JSAutoCompartment;
use js::jsapi::JS_NewGlobalObject;
use js::jsapi::OnNewGlobalHookOption;
use js::jsval::UndefinedValue;
use js::rust::AsHandle;
use js::rust::AsHandleMut;
use js::rust::Runtime as Runtime_;
use js::rust::SIMPLE_GLOBAL_CLASS;
use js::typedarray::Uint32Array;
use std::ptr;

#[test]
fn typedarray() {
    let rt = Runtime_::new();
    let cx = rt.cx();

    unsafe {
        rooted!(in(cx) let global =
            JS_NewGlobalObject(cx, &SIMPLE_GLOBAL_CLASS, ptr::null_mut(),
                               OnNewGlobalHookOption::FireOnNewGlobalHook,
                               &CompartmentOptions::default())
        );

        let _ac = JSAutoCompartment::new(cx, global.get());

        rooted!(in(cx) let mut rval = UndefinedValue());
        assert!(rt.evaluate_script(global.handle(), "new Uint8Array([0, 2, 4])",
                                   "test", 1, rval.handle_mut()).is_ok());
        assert!(rval.is_object());

        typedarray!(in(cx) let array: Uint8Array = rval.to_object());
        assert_eq!(array.unwrap().as_slice(), &[0, 2, 4][..]);

        typedarray!(in(cx) let array: Uint16Array = rval.to_object());
        assert!(array.is_err());

        rooted!(in(cx) let mut rval = ptr::null_mut());
        assert!(Uint32Array::create(cx, 5, Some(&[1, 3, 5]), rval.handle_mut()).is_ok());

        typedarray!(in(cx) let array: Uint32Array = rval.get());
        assert_eq!(array.unwrap().as_slice(), &[1, 3, 5, 0, 0][..]);
    }
}
