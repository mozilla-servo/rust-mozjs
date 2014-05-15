/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this file,
 * You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Rust wrappers around the raw JS apis

use libc;
use libc::types::os::arch::c95::{size_t, c_uint};
use libc::uintptr_t;
use libc::c_char;
use std::cmp;
use std::ptr;
use std::rc;
use std::rt::Runtime;
use std::string;
use jsapi::{JSContext, JSRuntime, JSGCStatus, JS_NewRuntime, JSObject};
use jsapi::{JS_SetNativeStackBounds, JS_SetGCCallback, JS_DestroyContext};
use jsapi::{JS_EnterCompartment, JS_LeaveCompartment};
use jsapi::{JS_SetErrorReporter, JS_NO_HELPER_THREADS};
use jsapi::{JS_EvaluateUCScript, JS_BeginRequest, JS_EndRequest};
use jsapi::{JS_NewContext, JSErrorReport, JSJITCOMPILER_ION_ENABLE};
use jsapi::{JSMutableHandleValue, JS_DestroyRuntime};
use jsapi::{JS_SetGlobalJitCompilerOption, JSJITCOMPILER_BASELINE_ENABLE};
use jsapi::{JSJITCOMPILER_PARALLEL_COMPILATION_ENABLE, JSHandleObject};
use jsval::{JSVal, NullValue};
//use glue::{CompartmentOptions_SetVersion};
use glue::{/*CompartmentOptions_SetTraceGlobal,*/ ContextOptions_SetVarObjFix};
use default_stacksize;
use default_heapsize;
use ERR;

// ___________________________________________________________________________
// friendly Rustic API to runtimes

pub type rt = rc::Rc<rt_rsrc>;

pub struct rt_rsrc {
    pub ptr : *mut JSRuntime,
}

impl Drop for rt_rsrc {
    fn drop(&mut self) {
        unsafe {
            JS_DestroyRuntime(self.ptr);
        }
    }
}

pub fn new_runtime(p: *mut JSRuntime) -> rt {
    return rc::Rc::new(rt_rsrc {
        ptr: p
    })
}

pub trait RtUtils {
    fn cx(&self) -> rc::Rc<Cx>;
}

impl RtUtils for rc::Rc<rt_rsrc> {
    fn cx(&self) -> rc::Rc<Cx> {
        unsafe {
            new_context(JS_NewContext(self.deref().ptr,
                                      default_stacksize as size_t), self.clone())
        }
    }
}

unsafe extern fn gc_callback(rt: *mut JSRuntime, _status: JSGCStatus, _data: *mut libc::c_void) {
    use std::rt::local::Local;
    use std::rt::task::Task;
    let mut task = Local::borrow(None::<Task>);
    let (start, end) = task.get().stack_bounds();
    JS_SetNativeStackBounds(rt, cmp::min(start, end) as uintptr_t, cmp::max(start, end) as uintptr_t);
}

pub fn rt() -> rt {
    unsafe {
        let runtime = JS_NewRuntime(default_heapsize, JS_NO_HELPER_THREADS, ptr::mut_null());
        JS_SetGCCallback(runtime, Some(gc_callback), ptr::mut_null());
        JS_SetGlobalJitCompilerOption(runtime, JSJITCOMPILER_ION_ENABLE, 1);
        JS_SetGlobalJitCompilerOption(runtime, JSJITCOMPILER_BASELINE_ENABLE, 1);
        JS_SetGlobalJitCompilerOption(runtime, JSJITCOMPILER_PARALLEL_COMPILATION_ENABLE, 0);
        return new_runtime(runtime);
    }
}

// ___________________________________________________________________________
// contexts

pub struct Cx {
    pub ptr: *mut JSContext,
    pub rt: rt,
}

#[unsafe_destructor]
impl Drop for Cx {
    fn drop(&mut self) {
        unsafe {
            JS_DestroyContext(self.ptr);
        }
    }
}

pub fn new_context(ptr: *mut JSContext, rt: rt) -> rc::Rc<Cx> {
    return rc::Rc::new(Cx {
        ptr: ptr,
        rt: rt,
    })
}

impl Cx {
    pub fn set_default_options_and_version(&self) {
        unsafe {
            ContextOptions_SetVarObjFix(self.ptr, true);
            //CompartmentOptions_SetVersion(self.ptr, JSVERSION_LATEST);
        }
    }

    pub fn set_logging_error_reporter(&self) {
        unsafe {
            JS_SetErrorReporter(self.ptr, Some(reportError));
        }
    }

    pub fn set_error_reporter(&self, reportfn: unsafe extern "C" fn(*mut JSContext, *const c_char, *mut JSErrorReport)) {
        unsafe {
            JS_SetErrorReporter(self.ptr, Some(reportfn));
        }
    }

    pub fn evaluate_script(&self, glob: *mut JSObject, script: String, filename: String, line_num: uint)
                    -> Result<(),()> {
        let script_utf16: Vec<u16> = script.as_slice().utf16_units().collect();
        let filename_cstr = filename.to_c_str();
        let mut rval: JSVal = NullValue();
        debug!("Evaluating script from {:s} with content {}", filename, script);
        // SpiderMonkey does not approve of null pointers.
        let (ptr, len) = if script_utf16.len() == 0 {
            static empty: &'static [u16] = &[];
            (empty.as_ptr(), 0)
        } else {
            (script_utf16.as_ptr(), script_utf16.len() as c_uint)
        };
        assert!(ptr.is_not_null());
        let globhandle = JSHandleObject {
            unnamed_field1: &glob,
        };
        let rvalhandle = JSMutableHandleValue {
            unnamed_field1: &mut rval,
        };
        unsafe {
            if ERR == JS_EvaluateUCScript(self.ptr, globhandle, ptr, len,
                                          filename_cstr.as_ptr(),
                                          line_num as c_uint, rvalhandle) {
                debug!("...err!");
                Err(())
            } else {
                // we could return the script result but then we'd have
                // to root it and so forth and, really, who cares?
                debug!("...ok!");
                Ok(())
            }
        }
    }
}

pub unsafe extern fn reportError(_cx: *mut JSContext, msg: *const c_char, report: *mut JSErrorReport) {
    let fnptr = (*report).filename;
    let fname = if fnptr.is_not_null() {string::raw::from_buf(fnptr as *const i8 as *const u8)} else {"none".to_string()};
    let lineno = (*report).lineno;
    let msg = string::raw::from_buf(msg as *const i8 as *const u8);
    error!("Error at {:s}:{}: {:s}\n", fname, lineno, msg);
}

pub fn with_compartment<R>(cx: *mut JSContext, object: *mut JSObject, cb: || -> R) -> R {
    unsafe {
        let _ar = JSAutoRequest::new(cx);
        let old_compartment = JS_EnterCompartment(cx, object);
        let result = cb();
        JS_LeaveCompartment(cx, old_compartment);
        result
    }
}

pub struct JSAutoRequest {
    cx: *mut JSContext,
}

impl JSAutoRequest {
    pub fn new(cx: *mut JSContext) -> JSAutoRequest {
        unsafe {
            JS_BeginRequest(cx);
        }
        JSAutoRequest {
            cx: cx,
        }
    }
}

impl Drop for JSAutoRequest {
    fn drop(&mut self) {
        unsafe {
            JS_EndRequest(self.cx);
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::rt;
    use super::RtUtils;

    #[test]
    pub fn dummy() {
        let rt = rt();
        let cx = rt.cx();
        cx.deref().set_default_options_and_version();
        cx.deref().set_logging_error_reporter();
    }

}