/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this file,
 * You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Rust wrappers around the raw JS apis

use libc::{size_t, c_uint, c_char};
use heapsize::HeapSizeOf;
use std::char;
use std::ffi;
use std::ptr;
use std::slice;
use std::mem;
use std::u32;
use std::default::Default;
use std::ops::{Deref, DerefMut};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use consts::{JSCLASS_RESERVED_SLOTS_MASK, JSCLASS_GLOBAL_SLOT_COUNT};
use consts::{JSCLASS_IS_DOMJSCLASS, JSCLASS_IS_GLOBAL};
use jsapi;
use jsapi::{AutoObjectVector, CallArgs, CompartmentOptions, ContextFriendFields};
use jsapi::{Evaluate2, Handle, HandleBase, HandleObject, HandleValue, Heap};
use jsapi::{HeapObjectPostBarrier, HeapValuePostBarrier, InitSelfHostedCode};
use jsapi::{IsWindowSlow, JS_BeginRequest, JS_DefineFunctions, JS_DefineProperties};
use jsapi::{JS_DestroyRuntime, JS_EndRequest, JS_EnterCompartment, JS_GetContext};
use jsapi::{JS_GlobalObjectTraceHook, JS_Init, JS_LeaveCompartment, JS_NewRuntime};
use jsapi::{JS_SetGCParameter, JS_SetNativeStackQuota, JS_WrapValue, JSAutoCompartment};
use jsapi::{JSClass, JSCLASS_RESERVED_SLOTS_SHIFT, JSClassOps, JSCompartment, JSContext};
use jsapi::{JSErrorReport, JSFlatString, JSFunction, JSFunctionSpec, JSGCParamKey};
use jsapi::{JSID_VOID, JSJitGetterCallArgs, JSJitMethodCallArgs, JSJitSetterCallArgs};
use jsapi::{JSNativeWrapper, JSObject, JSPropertySpec, JSRuntime, JSScript};
use jsapi::{JSString, MutableHandle, MutableHandleBase, MutableHandleValue};
use jsapi::{NullHandleValue, Object, ObjectGroup,ReadOnlyCompileOptions, Rooted};
use jsapi::{RootedBase, RuntimeOptionsRef, SetWarningReporter, Symbol, ToBooleanSlow};
use jsapi::{ToInt32Slow, ToInt64Slow, ToNumberSlow, ToStringSlow, ToUint16Slow};
use jsapi::{ToUint32Slow, ToUint64Slow, ToWindowIfWindowProxy, UndefinedHandleValue};
use jsapi::{Value, jsid};
use jsval::{ObjectValue, UndefinedValue};
use glue::{CreateAutoObjectVector, CreateCallArgsFromVp, AppendToAutoObjectVector, DeleteAutoObjectVector};
use glue::{NewCompileOptions, DeleteCompileOptions};
use default_heapsize;

// From Gecko:
// Our "default" stack is what we use in configurations where we don't have a compelling reason to
// do things differently. This is effectively 1MB on 64-bit platforms.
const STACK_QUOTA: usize = 128 * 8 * 1024;

// From Gecko:
// The JS engine permits us to set different stack limits for system code,
// trusted script, and untrusted script. We have tests that ensure that
// we can always execute 10 "heavy" (eval+with) stack frames deeper in
// privileged code. Our stack sizes vary greatly in different configurations,
// so satisfying those tests requires some care. Manual measurements of the
// number of heavy stack frames achievable gives us the following rough data,
// ordered by the effective categories in which they are grouped in the
// JS_SetNativeStackQuota call (which predates this analysis).
//
// (NB: These numbers may have drifted recently - see bug 938429)
// OSX 64-bit Debug: 7MB stack, 636 stack frames => ~11.3k per stack frame
// OSX64 Opt: 7MB stack, 2440 stack frames => ~3k per stack frame
//
// Linux 32-bit Debug: 2MB stack, 426 stack frames => ~4.8k per stack frame
// Linux 64-bit Debug: 4MB stack, 455 stack frames => ~9.0k per stack frame
//
// Windows (Opt+Debug): 900K stack, 235 stack frames => ~3.4k per stack frame
//
// Linux 32-bit Opt: 1MB stack, 272 stack frames => ~3.8k per stack frame
// Linux 64-bit Opt: 2MB stack, 316 stack frames => ~6.5k per stack frame
//
// We tune the trusted/untrusted quotas for each configuration to achieve our
// invariants while attempting to minimize overhead. In contrast, our buffer
// between system code and trusted script is a very unscientific 10k.
const SYSTEM_CODE_BUFFER: usize = 10 * 1024;

// Gecko's value on 64-bit.
const TRUSTED_SCRIPT_BUFFER: usize = 8 * 12800;

trait ToResult {
    fn to_result(self) -> Result<(), ()>;
}

impl ToResult for bool {
    fn to_result(self) -> Result<(), ()> {
        if self {
            Ok(())
        } else {
            Err(())
        }
    }
}

// ___________________________________________________________________________
// friendly Rustic API to runtimes

/// A wrapper for the `JSRuntime` and `JSContext` structures in SpiderMonkey.
pub struct Runtime {
    rt: *mut JSRuntime,
    cx: *mut JSContext,
}

impl Runtime {
    /// Creates a new `JSRuntime` and `JSContext`.
    pub fn new() -> Runtime {
        unsafe {
            struct TopRuntime(*mut JSRuntime);
            unsafe impl Sync for TopRuntime {}

            lazy_static! {
                static ref PARENT: TopRuntime = {
                    unsafe {
                        assert!(JS_Init());
                        let runtime = JS_NewRuntime(
                            default_heapsize, ChunkSize as u32, ptr::null_mut());
                        assert!(!runtime.is_null());
                        let context = JS_GetContext(runtime);
                        assert!(!context.is_null());
                        InitSelfHostedCode(context);
                        TopRuntime(runtime)
                    }
                };
            }

            let js_runtime =
                JS_NewRuntime(default_heapsize, ChunkSize as u32, PARENT.0);
            assert!(!js_runtime.is_null());

            // Unconstrain the runtime's threshold on nominal heap size, to avoid
            // triggering GC too often if operating continuously near an arbitrary
            // finite threshold. This leaves the maximum-JS_malloc-bytes threshold
            // still in effect to cause periodical, and we hope hygienic,
            // last-ditch GCs from within the GC's allocator.
            JS_SetGCParameter(
                js_runtime, JSGCParamKey::JSGC_MAX_BYTES, u32::MAX);

            JS_SetNativeStackQuota(
                js_runtime,
                STACK_QUOTA,
                STACK_QUOTA - SYSTEM_CODE_BUFFER,
                STACK_QUOTA - SYSTEM_CODE_BUFFER - TRUSTED_SCRIPT_BUFFER);

            let js_context = JS_GetContext(js_runtime);
            assert!(!js_context.is_null());

            InitSelfHostedCode(js_context);

            let runtimeopts = RuntimeOptionsRef(js_runtime);
            (*runtimeopts).set_baseline_(true);
            (*runtimeopts).set_ion_(true);
            (*runtimeopts).set_nativeRegExp_(true);

            SetWarningReporter(js_runtime, Some(report_warning));

            JS_BeginRequest(js_context);

            Runtime {
                rt: js_runtime,
                cx: js_context,
            }
        }
    }

    /// Returns the `JSRuntime` object.
    pub fn rt(&self) -> *mut JSRuntime {
        self.rt
    }

    /// Returns the `JSContext` object.
    pub fn cx(&self) -> *mut JSContext {
        self.cx
    }

    pub fn evaluate_script(&self, glob: HandleObject, script: &str, filename: &str,
                           line_num: u32, rval: MutableHandleValue)
                    -> Result<(),()> {
        let script_utf16: Vec<u16> = script.encode_utf16().collect();
        let filename_cstr = ffi::CString::new(filename.as_bytes()).unwrap();
        debug!("Evaluating script from {} with content {}", filename, script);
        // SpiderMonkey does not approve of null pointers.
        let (ptr, len) = if script_utf16.len() == 0 {
            static empty: &'static [u16] = &[];
            (empty.as_ptr(), 0)
        } else {
            (script_utf16.as_ptr(), script_utf16.len() as c_uint)
        };
        assert!(!ptr.is_null());
        let _ac = JSAutoCompartment::new(self.cx(), glob.get());
        let options = CompileOptionsWrapper::new(self.cx(), filename_cstr.as_ptr(), line_num);

        unsafe {
            if !Evaluate2(self.cx(), options.ptr, ptr as *const u16, len as size_t, rval) {
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

impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            JS_EndRequest(self.cx);
            JS_DestroyRuntime(self.rt);
        }
    }
}

// This is measured through `glue::CollectServoSizes`.
impl HeapSizeOf for Runtime {
    fn heap_size_of_children(&self) -> usize {
        0
    }
}

// ___________________________________________________________________________
// Rooting API for standard JS things

pub trait RootKind {
    #[inline(always)]
    fn rootKind() -> jsapi::RootKind;
}

impl RootKind for *mut JSObject {
    #[inline(always)]
    fn rootKind() -> jsapi::RootKind { jsapi::RootKind::Object }
}

impl RootKind for *mut JSFlatString {
    #[inline(always)]
    fn rootKind() -> jsapi::RootKind { jsapi::RootKind::String }
}

impl RootKind for *mut JSFunction {
    #[inline(always)]
    fn rootKind() -> jsapi::RootKind { jsapi::RootKind::Object }
}

impl RootKind for *mut JSString {
    #[inline(always)]
    fn rootKind() -> jsapi::RootKind { jsapi::RootKind::String }
}

impl RootKind for *mut Symbol {
    #[inline(always)]
    fn rootKind() -> jsapi::RootKind { jsapi::RootKind::Symbol }
}

impl RootKind for *mut JSScript {
    #[inline(always)]
    fn rootKind() -> jsapi::RootKind { jsapi::RootKind::Script }
}

impl RootKind for jsid {
    #[inline(always)]
    fn rootKind() -> jsapi::RootKind { jsapi::RootKind::Id }
}

impl RootKind for Value {
    #[inline(always)]
    fn rootKind() -> jsapi::RootKind { jsapi::RootKind::Value }
}

impl<T> Rooted<T> {
    pub fn new_unrooted(initial: T) -> Rooted<T> {
        Rooted {
            _base: RootedBase { _phantom0: PhantomData },
            stack: ptr::null_mut(),
            prev: ptr::null_mut(),
            ptr: initial,
        }
    }

    pub unsafe fn add_to_root_stack(&mut self, cx: *mut JSContext) where T: RootKind {
        let ctxfriend: &mut ContextFriendFields = mem::transmute(cx);

        let kind = T::rootKind() as usize;
        self.stack = &mut ctxfriend.roots.stackRoots_[kind] as *mut _ as *mut _;
        self.prev = ctxfriend.roots.stackRoots_[kind] as *mut _;

        ctxfriend.roots.stackRoots_[kind] = self as *mut _ as usize as _;
    }

    pub unsafe fn remove_from_root_stack(&mut self) {
        assert!(*self.stack == mem::transmute(&*self));
        *self.stack = self.prev;
    }
}

/// Rust API for keeping a Rooted value in the context's root stack.
/// Example usage: `rooted!(in(cx) let x = UndefinedValue());`.
/// `RootedGuard::new` also works, but the macro is preferred.
pub struct RootedGuard<'a, T: 'a> {
    root: &'a mut Rooted<T>
}

impl<'a, T> RootedGuard<'a, T> {
    pub fn new(cx: *mut JSContext, root: &'a mut Rooted<T>) -> Self where T: RootKind {
        unsafe {
            root.add_to_root_stack(cx);
        }
        RootedGuard {
            root: root
        }
    }

    pub fn handle(&self) -> Handle<T> {
        unsafe {
            Handle::from_marked_location(&self.root.ptr)
        }
    }

    pub fn handle_mut(&mut self) -> MutableHandle<T> {
        unsafe {
            MutableHandle::from_marked_location(&mut self.root.ptr)
        }
    }

    pub fn get(&self) -> T where T: Copy {
        self.root.ptr
    }

    pub fn set(&mut self, v: T) {
        self.root.ptr = v;
    }
}

impl<'a, T> Deref for RootedGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.root.ptr
    }
}

impl<'a, T> DerefMut for RootedGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.root.ptr
    }
}

impl<'a, T> Drop for RootedGuard<'a, T> {
    fn drop(&mut self) {
        unsafe {
            self.root.remove_from_root_stack();
        }
    }
}

#[macro_export]
macro_rules! rooted {
    (in($cx:expr) let $name:ident = $init:expr) => {
        let mut __root = $crate::jsapi::Rooted::new_unrooted($init);
        let $name = $crate::rust::RootedGuard::new($cx, &mut __root);
    };
    (in($cx:expr) let mut $name:ident = $init:expr) => {
        let mut __root = $crate::jsapi::Rooted::new_unrooted($init);
        let mut $name = $crate::rust::RootedGuard::new($cx, &mut __root);
    }
}

impl<T> Handle<T> {
    pub fn get(&self) -> T
        where T: Copy
    {
        unsafe { *self.ptr }
    }

    pub unsafe fn from_marked_location(ptr: *const T) -> Handle<T> {
        Handle {
            _base: HandleBase { _phantom0: PhantomData },
            ptr: ptr,
        }
    }
}

impl<T> Deref for Handle<T> {
    type Target = T;

    fn deref<'a>(&'a self) -> &'a T {
        unsafe { &*self.ptr }
    }
}

impl<T> MutableHandle<T> {
    pub unsafe fn from_marked_location(ptr: *mut T) -> MutableHandle<T> {
        MutableHandle {
            _base: MutableHandleBase { _phantom0: PhantomData },
            ptr: ptr,
        }
    }

    pub fn handle(&self) -> Handle<T> {
        unsafe {
            Handle::from_marked_location(self.ptr as *const _)
        }
    }

    pub fn get(&self) -> T
        where T: Copy
    {
        unsafe { *self.ptr }
    }

    pub fn set(&self, v: T)
        where T: Copy
    {
        unsafe { *self.ptr = v }
    }
}

impl<T> Deref for MutableHandle<T> {
    type Target = T;

    fn deref<'a>(&'a self) -> &'a T {
        unsafe { &*self.ptr }
    }
}

impl<T> DerefMut for MutableHandle<T> {
    fn deref_mut<'a>(&'a mut self) -> &'a mut T {
        unsafe { &mut *self.ptr }
    }
}

impl HandleValue {
    pub fn null() -> HandleValue {
        NullHandleValue
    }

    pub fn undefined() -> HandleValue {
        UndefinedHandleValue
    }
}

const ConstNullValue: *mut JSObject = 0 as *mut JSObject;

impl HandleObject {
    pub fn null() -> HandleObject {
        unsafe {
            HandleObject::from_marked_location(&ConstNullValue)
        }
    }
}

impl Default for jsid {
    fn default() -> jsid { JSID_VOID }
}

impl Default for Value {
    fn default() -> Value { UndefinedValue() }
}

impl Default for CompartmentOptions {
    fn default() -> Self { unsafe { ::std::mem::zeroed() } }
}

const ChunkShift: usize = 20;
const ChunkSize: usize = 1 << ChunkShift;

#[cfg(target_pointer_width = "32")]
const ChunkLocationOffset: usize = ChunkSize - 2 * 4 - 8;

pub trait GCMethods<T> {
    unsafe fn initial() -> T;
    unsafe fn post_barrier(v: *mut T, prev: T, next: T);
}

impl GCMethods<jsid> for jsid {
    unsafe fn initial() -> jsid { JSID_VOID }
    unsafe fn post_barrier(_: *mut jsid, _: jsid, _: jsid) {}
}

impl GCMethods<*mut JSObject> for *mut JSObject {
    unsafe fn initial() -> *mut JSObject { ptr::null_mut() }
    unsafe fn post_barrier(v: *mut *mut JSObject,
                           prev: *mut JSObject, next: *mut JSObject) {
        HeapObjectPostBarrier(v, prev, next);
    }
}

impl GCMethods<*mut JSString> for *mut JSString {
    unsafe fn initial() -> *mut JSString { ptr::null_mut() }
    unsafe fn post_barrier(_: *mut *mut JSString, _: *mut JSString, _: *mut JSString) {}
}

impl GCMethods<*mut JSScript> for *mut JSScript {
    unsafe fn initial() -> *mut JSScript { ptr::null_mut() }
    unsafe fn post_barrier(_: *mut *mut JSScript, _: *mut JSScript, _: *mut JSScript) { }
}

impl GCMethods<*mut JSFunction> for *mut JSFunction {
    unsafe fn initial() -> *mut JSFunction { ptr::null_mut() }
    unsafe fn post_barrier(v: *mut *mut JSFunction,
                           prev: *mut JSFunction, next: *mut JSFunction) {
        HeapObjectPostBarrier(mem::transmute(v),
                              mem::transmute(prev), mem::transmute(next));
    }
}

impl GCMethods<Value> for Value {
    unsafe fn initial() -> Value { UndefinedValue() }
    unsafe fn post_barrier(v: *mut Value, prev: Value, next: Value) {
        HeapValuePostBarrier(v, &prev, &next);
    }
}

impl<T: GCMethods<T> + Copy> Heap<T> {
    pub fn set(&mut self, v: T) {
        unsafe {
            let ptr = self.ptr.get();
            let prev = *ptr;
            *ptr = v;
            T::post_barrier(ptr, prev, v);
        }
    }

    pub fn get(&self) -> T {
        unsafe { *self.ptr.get() }
    }

    pub fn get_unsafe(&self) -> *mut T {
        self.ptr.get()
    }

    pub fn handle(&self) -> Handle<T> {
        unsafe {
            Handle::from_marked_location(self.ptr.get() as *const _)
        }
    }
}

impl Default for Heap<*mut JSObject> {
    fn default() -> Heap<*mut JSObject> {
        Heap {
            ptr: UnsafeCell::new(ptr::null_mut())
        }
    }
}

impl Default for Heap<Value> {
    fn default() -> Heap<Value> {
        Heap {
            ptr: UnsafeCell::new(Value::default())
        }
    }
}

impl<T: GCMethods<T> + Copy> Drop for Heap<T> {
    fn drop(&mut self) {
        unsafe {
            let ptr = self.ptr.get();
            T::post_barrier(ptr, *ptr, T::initial());
        }
    }
}


// ___________________________________________________________________________
// Implementations for various things in jsapi.rs

impl JSAutoCompartment {
    pub fn new(cx: *mut JSContext, target: *mut JSObject) -> JSAutoCompartment {
        JSAutoCompartment {
            cx_: cx,
            oldCompartment_: unsafe { JS_EnterCompartment(cx, target) }
        }
    }
}

impl Drop for JSAutoCompartment {
    fn drop(&mut self) {
        unsafe { JS_LeaveCompartment(self.cx_, self.oldCompartment_); }
    }
}

impl JSJitMethodCallArgs {
    #[inline]
    pub fn get(&self, i: u32) -> HandleValue {
        unsafe {
            if i < self._base.argc_ {
                HandleValue::from_marked_location(self._base.argv_.offset(i as isize))
            } else {
                UndefinedHandleValue
            }
        }
    }

    #[inline]
    pub fn index(&self, i: u32) -> HandleValue {
        assert!(i < self._base.argc_);
        unsafe {
            HandleValue::from_marked_location(self._base.argv_.offset(i as isize))
        }
    }

    #[inline]
    pub fn index_mut(&self, i: u32) -> MutableHandleValue {
        assert!(i < self._base.argc_);
        unsafe {
            MutableHandleValue::from_marked_location(self._base.argv_.offset(i as isize))
        }
    }

    #[inline]
    pub fn rval(&self) -> MutableHandleValue {
        unsafe {
            MutableHandleValue::from_marked_location(self._base.argv_.offset(-2))
        }
    }
}

// XXX need to hack up bindgen to convert this better so we don't have
//     to duplicate so much code here
impl CallArgs {
    #[inline]
    pub unsafe fn from_vp(vp: *mut Value, argc: u32) -> CallArgs {
        CreateCallArgsFromVp(argc, vp)
    }

    #[inline]
    pub fn index(&self, i: u32) -> HandleValue {
        assert!(i < self._base.argc_);
        unsafe {
            HandleValue::from_marked_location(self._base.argv_.offset(i as isize))
        }
    }

    #[inline]
    pub fn index_mut(&self, i: u32) -> MutableHandleValue {
        assert!(i < self._base.argc_);
        unsafe {
            MutableHandleValue::from_marked_location(self._base.argv_.offset(i as isize))
        }
    }

    #[inline]
    pub fn get(&self, i: u32) -> HandleValue {
        unsafe {
            if i < self._base.argc_ {
                HandleValue::from_marked_location(self._base.argv_.offset(i as isize))
            } else {
                UndefinedHandleValue
            }
        }
    }

    #[inline]
    pub fn rval(&self) -> MutableHandleValue {
        unsafe {
            MutableHandleValue::from_marked_location(self._base.argv_.offset(-2))
        }
    }

    #[inline]
    pub fn thisv(&self) -> HandleValue {
        unsafe {
            HandleValue::from_marked_location(self._base.argv_.offset(-1))
        }
    }

    #[inline]
    pub fn calleev(&self) -> HandleValue {
        unsafe {
            HandleValue::from_marked_location(self._base.argv_.offset(-2))
        }
    }

    #[inline]
    pub fn callee(&self) -> *mut JSObject {
        self.calleev().to_object()
    }
}

impl JSJitGetterCallArgs {
    #[inline]
    pub fn rval(&self) -> MutableHandleValue {
        self._base
    }
}

impl JSJitSetterCallArgs {
    #[inline]
    pub fn get(&self, i: u32) -> HandleValue {
        assert!(i == 0);
        self._base.handle()
    }
}

// ___________________________________________________________________________
// Wrappers around things in jsglue.cpp

pub struct AutoObjectVectorWrapper {
    pub ptr: *mut AutoObjectVector
}

impl AutoObjectVectorWrapper {
    pub fn new(cx: *mut JSContext) -> AutoObjectVectorWrapper {
        AutoObjectVectorWrapper {
            ptr: unsafe {
                 CreateAutoObjectVector(cx)
            }
        }
    }

    pub fn append(&self, obj: *mut JSObject) -> bool {
        unsafe {
            AppendToAutoObjectVector(self.ptr, obj)
        }
    }
}

impl Drop for AutoObjectVectorWrapper {
    fn drop(&mut self) {
        unsafe { DeleteAutoObjectVector(self.ptr) }
    }
}

pub struct CompileOptionsWrapper {
    pub ptr: *mut ReadOnlyCompileOptions
}

impl CompileOptionsWrapper {
    pub fn new(cx: *mut JSContext, file: *const ::libc::c_char, line: c_uint) -> CompileOptionsWrapper {
        CompileOptionsWrapper {
            ptr: unsafe { NewCompileOptions(cx, file, line) }
        }
    }
}

impl Drop for CompileOptionsWrapper {
    fn drop(&mut self) {
        unsafe { DeleteCompileOptions(self.ptr) }
    }
}

// ___________________________________________________________________________
// Fast inline converters

#[inline]
pub unsafe fn ToBoolean(v: HandleValue) -> bool {
    let val = *v.ptr;

    if val.is_boolean() {
        return val.to_boolean();
    }

    if val.is_int32() {
        return val.to_int32() != 0;
    }

    if val.is_null_or_undefined() {
        return false;
    }

    if val.is_double() {
        let d = val.to_double();
        return !d.is_nan() && d != 0f64;
    }

    if val.is_symbol() {
        return true;
    }

    ToBooleanSlow(v)
}

#[inline]
pub unsafe fn ToNumber(cx: *mut JSContext, v: HandleValue) -> Result<f64, ()> {
    let val = *v.ptr;
    if val.is_number() {
        return Ok(val.to_number());
    }

    let mut out = Default::default();
    if ToNumberSlow(cx, val, &mut out) {
        Ok(out)
    } else {
        Err(())
    }
}

#[inline]
unsafe fn convert_from_int32<T: Default + Copy>(
    cx: *mut JSContext,
    v: HandleValue,
    conv_fn: unsafe extern "C" fn(*mut JSContext, HandleValue, *mut T) -> bool)
        -> Result<T, ()> {

    let val = *v.ptr;
    if val.is_int32() {
        let intval: i64 = val.to_int32() as i64;
        // TODO: do something better here that works on big endian
        let intval = *(&intval as *const i64 as *const T);
        return Ok(intval);
    }

    let mut out = Default::default();
    if conv_fn(cx, v, &mut out) {
        Ok(out)
    } else {
        Err(())
    }
}

#[inline]
pub unsafe fn ToInt32(cx: *mut JSContext, v: HandleValue) -> Result<i32, ()> {
    convert_from_int32::<i32>(cx, v, ToInt32Slow)
}

#[inline]
pub unsafe fn ToUint32(cx: *mut JSContext, v: HandleValue) -> Result<u32, ()> {
    convert_from_int32::<u32>(cx, v, ToUint32Slow)
}

#[inline]
pub unsafe fn ToUint16(cx: *mut JSContext, v: HandleValue) -> Result<u16, ()> {
    convert_from_int32::<u16>(cx, v, ToUint16Slow)
}

#[inline]
pub unsafe fn ToInt64(cx: *mut JSContext, v: HandleValue) -> Result<i64, ()> {
    convert_from_int32::<i64>(cx, v, ToInt64Slow)
}

#[inline]
pub unsafe fn ToUint64(cx: *mut JSContext, v: HandleValue) -> Result<u64, ()> {
    convert_from_int32::<u64>(cx, v, ToUint64Slow)
}

#[inline]
pub unsafe fn ToString(cx: *mut JSContext, v: HandleValue) -> *mut JSString {
    let val = *v.ptr;
    if val.is_string() {
        return val.to_string();
    }

    ToStringSlow(cx, v)
}

pub unsafe extern fn report_warning(_cx: *mut JSContext, _: *const c_char, report: *mut JSErrorReport) {
    fn latin1_to_string(bytes: &[u8]) -> String {
        bytes.iter().map(|c| char::from_u32(*c as u32).unwrap()).collect()
    }

    let fnptr = (*report).filename;
    let fname = if !fnptr.is_null() {
        let c_str = ffi::CStr::from_ptr(fnptr);
        latin1_to_string(c_str.to_bytes())
    } else {
        "none".to_string()
    };

    let lineno = (*report).lineno;
    let column = (*report).column;

    let msg_ptr = (*report).ucmessage;
    let msg_len = (0usize..).find(|&i| *msg_ptr.offset(i as isize) == 0).unwrap();
    let msg_slice = slice::from_raw_parts(msg_ptr, msg_len);
    let msg = String::from_utf16_lossy(msg_slice);

    warn!("Warning at {}:{}:{}: {}\n", fname, lineno, column, msg);
}

impl JSNativeWrapper {
    fn is_zeroed(&self) -> bool {
        let JSNativeWrapper { op, info } = *self;
        op.is_none() && info.is_null()
    }
}

/// Defines methods on `obj`. The last entry of `methods` must contain zeroed
/// memory.
///
/// # Failures
///
/// Returns `Err` on JSAPI failure.
///
/// # Panics
///
/// Panics if the last entry of `methods` does not contain zeroed memory.
///
/// # Safety
///
/// - `cx` must be valid.
/// - This function calls into unaudited C++ code.
pub unsafe fn define_methods(cx: *mut JSContext, obj: HandleObject,
                             methods: &'static [JSFunctionSpec])
                             -> Result<(), ()> {
    assert!({
        match methods.last() {
            Some(&JSFunctionSpec { name, call, nargs, flags, selfHostedName }) => {
                name.is_null() && call.is_zeroed() && nargs == 0 && flags == 0 &&
                selfHostedName.is_null()
            },
            None => false,
        }
    });

    JS_DefineFunctions(cx, obj, methods.as_ptr()).to_result()
}

/// Defines attributes on `obj`. The last entry of `properties` must contain
/// zeroed memory.
///
/// # Failures
///
/// Returns `Err` on JSAPI failure.
///
/// # Panics
///
/// Panics if the last entry of `properties` does not contain zeroed memory.
///
/// # Safety
///
/// - `cx` must be valid.
/// - This function calls into unaudited C++ code.
pub unsafe fn define_properties(cx: *mut JSContext, obj: HandleObject,
                                properties: &'static [JSPropertySpec])
                                -> Result<(), ()> {
    assert!({
        match properties.last() {
            Some(&JSPropertySpec { name, flags, getter, setter }) => {
                name.is_null() && flags == 0 && getter.is_zeroed() && setter.is_zeroed()
            },
            None => false,
        }
    });

    JS_DefineProperties(cx, obj, properties.as_ptr()).to_result()
}

static SIMPLE_GLOBAL_CLASS_OPS: JSClassOps = JSClassOps {
    addProperty: None,
    delProperty: None,
    getProperty: None,
    setProperty: None,
    enumerate: None,
    resolve: None,
    mayResolve: None,
    finalize: None,
    call: None,
    hasInstance: None,
    construct: None,
    trace: Some(JS_GlobalObjectTraceHook),
};

/// This is a simple `JSClass` for global objects, primarily intended for tests.
pub static SIMPLE_GLOBAL_CLASS: JSClass = JSClass {
    name: b"Global\0" as *const u8 as *const _,
    flags: JSCLASS_IS_GLOBAL | ((JSCLASS_GLOBAL_SLOT_COUNT & JSCLASS_RESERVED_SLOTS_MASK) << JSCLASS_RESERVED_SLOTS_SHIFT),
    cOps: &SIMPLE_GLOBAL_CLASS_OPS as *const JSClassOps,
    reserved: [0 as *mut _; 3]
};

#[inline]
unsafe fn get_object_group(obj: *mut JSObject) -> *mut ObjectGroup {
    assert!(!obj.is_null());
    (*(obj as *mut Object)).group
}

#[inline]
pub unsafe fn get_object_class(obj: *mut JSObject) -> *const JSClass {
    (*get_object_group(obj)).clasp as *const _
}

#[inline]
pub unsafe fn get_object_compartment(obj: *mut JSObject) -> *mut JSCompartment {
    (*get_object_group(obj)).compartment
}

#[inline]
pub unsafe fn get_context_compartment(cx: *mut JSContext) -> *mut JSCompartment {
    (*(cx as *mut ContextFriendFields)).compartment_
}

#[inline]
pub fn is_dom_class(class: &JSClass) -> bool {
    class.flags & JSCLASS_IS_DOMJSCLASS != 0
}

#[inline]
pub unsafe fn is_dom_object(obj: *mut JSObject) -> bool {
    is_dom_class(&*get_object_class(obj))
}

#[inline]
pub unsafe fn is_window(obj: *mut JSObject) -> bool {
    (*get_object_class(obj)).flags & JSCLASS_IS_GLOBAL != 0 && IsWindowSlow(obj)
}

#[inline]
pub unsafe fn try_to_outerize(rval: MutableHandleValue) {
    let obj = rval.to_object();
    if is_window(obj) {
        let obj = ToWindowIfWindowProxy(obj);
        assert!(!obj.is_null());
        rval.set(ObjectValue(&*obj));
    }
}

#[inline]
pub unsafe fn maybe_wrap_object_value(cx: *mut JSContext, rval: MutableHandleValue) {
    assert!(rval.is_object());
    let obj = rval.to_object();
    if get_object_compartment(obj) == get_context_compartment(cx) {
        if is_dom_object(obj) {
            return try_to_outerize(rval);
        }
    }
    assert!(JS_WrapValue(cx, rval));
}

#[inline]
pub unsafe fn maybe_wrap_object_or_null_value(cx: *mut JSContext,
                                              rval: MutableHandleValue) {
    assert!(rval.is_object_or_null());
    if !rval.is_null() {
        maybe_wrap_object_value(cx, rval);
    }
}

#[inline]
pub unsafe fn maybe_wrap_value(cx: *mut JSContext, rval: MutableHandleValue) {
    if rval.is_object_or_null() {
        maybe_wrap_object_value(cx, rval);
    } else {
        assert!(JS_WrapValue(cx, rval));
    }
}
