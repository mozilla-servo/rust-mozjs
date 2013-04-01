use jsapi::*;

pub extern fn JS_PropertyStub(cx: *JSContext, obj: JSHandleObject, id: JSHandleId, vp: JSMutableHandleValue) -> JSBool {
    unsafe {
        bindgen::JS_PropertyStub(cx, obj, id, vp)
    }
}

pub extern fn JS_StrictPropertyStub(cx: *JSContext, obj: JSHandleObject, id: JSHandleId, strict: JSBool, vp: JSMutableHandleValue) -> JSBool {
    unsafe {
        bindgen::JS_StrictPropertyStub(cx, obj, id, strict, vp)
    }
}

pub extern fn JS_EnumerateStub(cx: *JSContext, obj: JSHandleObject) -> JSBool {
    unsafe {
        bindgen::JS_EnumerateStub(cx, obj)
    }
}

pub extern fn JS_ResolveStub(cx: *JSContext, obj: JSHandleObject, id: JSHandleId) -> JSBool {
    unsafe {
        bindgen::JS_ResolveStub(cx, obj, id)
    }
}

pub extern fn JS_ConvertStub(cx: *JSContext, obj: JSHandleObject, _type: JSType, vp: JSMutableHandleValue) -> JSBool {
    unsafe {
        bindgen::JS_ConvertStub(cx, obj, _type, vp)
    }
}

pub extern fn JS_ArrayIterator(cx: *JSContext, argc: libc::c_uint, vp: *JSVal) -> JSBool {
    unsafe {
        bindgen::JS_ArrayIterator(cx, argc, vp)
    }
}

