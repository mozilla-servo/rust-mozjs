/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! High-level, safe bindings for JS typed array APIs. Allows creating new
//! typed arrays or wrapping existing JS reflectors, and prevents reinterpreting
//! existing buffers as different types except in well-defined cases.

use glue::GetFloat32ArrayLengthAndData;
use glue::GetFloat64ArrayLengthAndData;
use glue::GetInt16ArrayLengthAndData;
use glue::GetInt32ArrayLengthAndData;
use glue::GetInt8ArrayLengthAndData;
use glue::GetUint16ArrayLengthAndData;
use glue::GetUint32ArrayLengthAndData;
use glue::GetUint8ArrayLengthAndData;
use glue::GetUint8ClampedArrayLengthAndData;
use jsapi::GetArrayBufferLengthAndData;
use jsapi::GetArrayBufferViewLengthAndData;
use jsapi::HandleObject;
use jsapi::Heap;
use jsapi::JSContext;
use jsapi::JS_GetArrayBufferData;
use jsapi::JS_GetArrayBufferViewType;
use jsapi::JS_GetFloat32ArrayData;
use jsapi::JS_GetFloat64ArrayData;
use jsapi::JS_GetInt16ArrayData;
use jsapi::JS_GetInt32ArrayData;
use jsapi::JS_GetInt8ArrayData;
use jsapi::JS_GetUint16ArrayData;
use jsapi::JS_GetUint32ArrayData;
use jsapi::JS_GetUint8ArrayData;
use jsapi::JS_GetUint8ClampedArrayData;
use jsapi::JS_NewArrayBuffer;
use jsapi::JS_NewFloat32Array;
use jsapi::JS_NewFloat64Array;
use jsapi::JS_NewInt16Array;
use jsapi::JS_NewInt32Array;
use jsapi::JS_NewInt8Array;
use jsapi::JS_NewUint16Array;
use jsapi::JS_NewUint32Array;
use jsapi::JS_NewUint8Array;
use jsapi::JS_NewUint8ClampedArray;
use jsapi::JSObject;
use jsapi::MutableHandleObject;
use jsapi::Type;
use jsapi::UnwrapArrayBuffer;
use jsapi::UnwrapArrayBufferView;
use jsapi::UnwrapFloat32Array;
use jsapi::UnwrapFloat64Array;
use jsapi::UnwrapInt16Array;
use jsapi::UnwrapInt32Array;
use jsapi::UnwrapInt8Array;
use jsapi::UnwrapUint16Array;
use jsapi::UnwrapUint32Array;
use jsapi::UnwrapUint8Array;
use jsapi::UnwrapUint8ClampedArray;
use std::ptr;
use std::slice;

pub enum CreateWith<'a, T: 'a> {
    Length(u32),
    Slice(&'a [T]),
}

/// A rooted typed array.
pub struct TypedArray<T: TypedArrayElement> {
    object: Box<Heap<*mut JSObject>>,
    computed: Option<(*mut T::Element, u32)>,
}

impl<T: TypedArrayElement> TypedArray<T> {
    /// Create a typed array representation that wraps an existing JS reflector.
    /// This operation will fail if attempted on a JS object that does not match
    /// the expected typed array details.
    pub fn from(cx: *mut JSContext,
                object: *mut JSObject)
                -> Result<Self, ()> {
        rooted!(in (cx) let object = object);
        if object.is_null() {
            return Err(());
        }
        unsafe {
            let unwrapped = T::unwrap_array(object.get());
            if unwrapped.is_null() {
                return Err(());
            }

            let array = Box::new(Heap::new(ptr::null_mut()));
            array.set(unwrapped);

            Ok(TypedArray {
                object: array,
                computed: None,
            })
        }
    }

    fn data(&mut self) -> (*mut T::Element, u32) {
        if let Some(data) = self.computed {
            return data;
        }

        let data = unsafe { T::length_and_data(self.object.get()) };
        self.computed = Some(data);
        data
    }

    /// # Unsafety
    ///
    /// The returned slice can be invalidated if the underlying typed array
    /// is neutered.
    pub unsafe fn as_slice(&mut self) -> &[T::Element] {
        let (pointer, length) = self.data();
        slice::from_raw_parts(pointer as *const T::Element, length as usize)
    }

    /// # Unsafety
    ///
    /// The returned slice can be invalidated if the underlying typed array
    /// is neutered.
    ///
    /// The underlying `JSObject` can be aliased, which can lead to
    /// Undefined Behavior due to mutable aliasing.
    pub unsafe fn as_mut_slice(&mut self) -> &mut [T::Element] {
        let (pointer, length) = self.data();
        slice::from_raw_parts_mut(pointer, length as usize)
    }

    pub fn object(&self) -> &Heap<*mut JSObject> {
        &self.object
    }
}

impl<T: TypedArrayElementCreator + TypedArrayElement> TypedArray<T> {
    /// Create a new JS typed array, optionally providing initial data that will
    /// be copied into the newly-allocated buffer. Returns the new JS reflector.
    pub unsafe fn create(cx: *mut JSContext,
                         with: CreateWith<T::Element>,
                         result: MutableHandleObject)
                         -> Result<(), ()> {
        let length = match with {
            CreateWith::Length(len) => len,
            CreateWith::Slice(slice) => slice.len() as u32,
        };

        result.set(T::create_new(cx, length));
        if result.get().is_null() {
            return Err(());
        }

        if let CreateWith::Slice(data) = with {
            TypedArray::<T>::update_raw(data, result.handle());
        }

        Ok(())
    }

    ///  Update an existed JS typed array
    pub unsafe fn update(&mut self, data: &[T::Element]) {
        TypedArray::<T>::update_raw(data, self.object.handle());
    }

    unsafe fn update_raw(data: &[T::Element], result: HandleObject) {
        let (buf, length) = T::length_and_data(result.get());
        assert!(data.len() <= length as usize);
        ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len());
    }
}

/// Internal trait used to associate an element type with an underlying representation
/// and various functions required to manipulate typed arrays of that element type.
pub trait TypedArrayElement {
    /// Underlying primitive representation of this element type.
    type Element;
    /// Unwrap a typed array JS reflector for this element type.
    unsafe fn unwrap_array(obj: *mut JSObject) -> *mut JSObject;
    /// Retrieve the length and data of a typed array's buffer for this element type.
    unsafe fn length_and_data(obj: *mut JSObject) -> (*mut Self::Element, u32);
}

/// Internal trait for creating new typed arrays.
pub trait TypedArrayElementCreator: TypedArrayElement {
    /// Create a new typed array.
    unsafe fn create_new(cx: *mut JSContext, length: u32) -> *mut JSObject;
    /// Get the data.
    unsafe fn get_data(obj: *mut JSObject) -> *mut Self::Element;
}

macro_rules! typed_array_element {
    ($t: ident,
     $element: ty,
     $unwrap: ident,
     $length_and_data: ident) => (
        /// A kind of typed array.
        pub struct $t;

        impl TypedArrayElement for $t {
            type Element = $element;
            unsafe fn unwrap_array(obj: *mut JSObject) -> *mut JSObject {
                $unwrap(obj)
            }

            unsafe fn length_and_data(obj: *mut JSObject) -> (*mut Self::Element, u32) {
                let mut len = 0;
                let mut shared = false;
                let mut data = ptr::null_mut();
                $length_and_data(obj, &mut len, &mut shared, &mut data);
                assert!(!shared);
                (data, len)
            }
        }
    );

    ($t: ident,
     $element: ty,
     $unwrap: ident,
     $length_and_data: ident,
     $create_new: ident,
     $get_data: ident) => (
        typed_array_element!($t, $element, $unwrap, $length_and_data);

        impl TypedArrayElementCreator for $t {
            unsafe fn create_new(cx: *mut JSContext, length: u32) -> *mut JSObject {
                $create_new(cx, length)
            }

            unsafe fn get_data(obj: *mut JSObject) -> *mut Self::Element {
                let mut shared = false;
                let data = $get_data(obj, &mut shared, ptr::null_mut());
                assert!(!shared);
                data
            }
        }
    );
}

typed_array_element!(Uint8,
                     u8,
                     UnwrapUint8Array,
                     GetUint8ArrayLengthAndData,
                     JS_NewUint8Array,
                     JS_GetUint8ArrayData);
typed_array_element!(Uint16,
                     u16,
                     UnwrapUint16Array,
                     GetUint16ArrayLengthAndData,
                     JS_NewUint16Array,
                     JS_GetUint16ArrayData);
typed_array_element!(Uint32,
                     u32,
                     UnwrapUint32Array,
                     GetUint32ArrayLengthAndData,
                     JS_NewUint32Array,
                     JS_GetUint32ArrayData);
typed_array_element!(Int8,
                     i8,
                     UnwrapInt8Array,
                     GetInt8ArrayLengthAndData,
                     JS_NewInt8Array,
                     JS_GetInt8ArrayData);
typed_array_element!(Int16,
                     i16,
                     UnwrapInt16Array,
                     GetInt16ArrayLengthAndData,
                     JS_NewInt16Array,
                     JS_GetInt16ArrayData);
typed_array_element!(Int32,
                     i32,
                     UnwrapInt32Array,
                     GetInt32ArrayLengthAndData,
                     JS_NewInt32Array,
                     JS_GetInt32ArrayData);
typed_array_element!(Float32,
                     f32,
                     UnwrapFloat32Array,
                     GetFloat32ArrayLengthAndData,
                     JS_NewFloat32Array,
                     JS_GetFloat32ArrayData);
typed_array_element!(Float64,
                     f64,
                     UnwrapFloat64Array,
                     GetFloat64ArrayLengthAndData,
                     JS_NewFloat64Array,
                     JS_GetFloat64ArrayData);
typed_array_element!(ClampedU8,
                     u8,
                     UnwrapUint8ClampedArray,
                     GetUint8ClampedArrayLengthAndData,
                     JS_NewUint8ClampedArray,
                     JS_GetUint8ClampedArrayData);
typed_array_element!(ArrayBufferU8,
                     u8,
                     UnwrapArrayBuffer,
                     GetArrayBufferLengthAndData,
                     JS_NewArrayBuffer,
                     JS_GetArrayBufferData);
typed_array_element!(ArrayBufferViewU8,
                     u8,
                     UnwrapArrayBufferView,
                     GetArrayBufferViewLengthAndData);

/// The Uint8ClampedArray type.
pub type Uint8ClampedArray = TypedArray<ClampedU8>;
/// The Uint8Array type.
pub type Uint8Array = TypedArray<Uint8>;
/// The Int8Array type.
pub type Int8Array = TypedArray<Int8>;
/// The Uint16Array type.
pub type Uint16Array = TypedArray<Uint16>;
/// The Int16Array type.
pub type Int16Array = TypedArray<Int16>;
/// The Uint32Array type.
pub type Uint32Array = TypedArray<Uint32>;
/// The Int32Array type.
pub type Int32Array = TypedArray<Int32>;
/// The Float32Array type.
pub type Float32Array = TypedArray<Float32>;
/// The Float64Array type.
pub type Float64Array = TypedArray<Float64>;
/// The ArrayBuffer type.
pub type ArrayBuffer = TypedArray<ArrayBufferU8>;
/// The ArrayBufferView type
pub type ArrayBufferView = TypedArray<ArrayBufferViewU8>;

impl ArrayBufferView {
    pub fn get_array_type(&self) -> Type {
        unsafe { JS_GetArrayBufferViewType(self.object.get()) }
    }
}

#[macro_export]
macro_rules! typedarray {
    (in($cx:expr) let $name:ident : $ty:ident = $init:expr) => {
        let mut __root = $crate::jsapi::Rooted::new_unrooted();
        let $name = $crate::typedarray::$ty::from($cx, &mut __root, $init);
    };
    (in($cx:expr) let mut $name:ident : $ty:ident = $init:expr) => {
        let mut __root = $crate::jsapi::Rooted::new_unrooted();
        let mut $name = $crate::typedarray::$ty::from($cx, &mut __root, $init);
    }
}
