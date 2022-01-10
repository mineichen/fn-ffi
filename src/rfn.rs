use std::marker::PhantomData;

pub trait RFn<'a, TParam, TResult: 'a> {
    fn call(&'a self, p: TParam) -> TResult;
}

impl<'a, TFn: Fn(TParam) -> TResult, TParam, TResult: 'a> RFn<'a, TParam, TResult> for TFn {
    fn call(&'a self, p: TParam) -> TResult {
        (self)(p)
    }
}

/// FFI-Safe lambda without allocations
#[repr(C)]
#[cfg_attr(feature = "abi_stable", derive(abi_stable::StableAbi))]
pub struct RRefFn<'a, TParam, TResult> {
    ptr: extern "C" fn(usize, TParam) -> TResult,
    inner: usize,
    p: PhantomData<&'a ()>,
}

impl<'a, T, TParam, TResult> From<&'a T> for RRefFn<'a, TParam, TResult>
where
    T: 'a + Fn(TParam) -> TResult,
{
    fn from(inner: &T) -> Self {
        let inner: usize = unsafe { std::mem::transmute(inner) };

        extern "C" fn ptr<T, TParam, TResult>(inner: usize, p: TParam) -> TResult
        where
            T: Fn(TParam) -> TResult,
        {
            let function: &T = unsafe { std::mem::transmute(inner) };
            (function)(p)
        }
        Self {
            ptr: ptr::<T, TParam, TResult>,
            inner,
            p: PhantomData,
        }
    }
}
impl<'a, TParam, TResult: 'a> RFn<'a, TParam, TResult> for RRefFn<'a, TParam, TResult> {
    fn call(&'a self, p: TParam) -> TResult {
        (self.ptr)(self.inner, p)
    }
}

/// FFI-Safe lambda which requires 1 heap allocation for boxing
#[repr(C)]
#[cfg_attr(feature = "abi_stable", derive(abi_stable::StableAbi))]
pub struct RBoxFn<TParam, TResult> {
    caller: extern "C" fn(usize, TParam) -> TResult,
    remover: extern "C" fn(usize),
    inner: usize
}

impl<TFn, TParam, TResult> From<TFn> for RBoxFn<TParam, TResult>
where
    TFn: Fn(TParam) -> TResult,
{
    fn from(inner: TFn) -> Self {
        let box_inner = Box::new(inner);
        let inner = Box::into_raw(box_inner);
        let inner = inner as usize;

        extern "C" fn caller<TFn, TParam, TResult>(that: usize, param: TParam) -> TResult
        where
            TFn: Fn(TParam) -> TResult,
        {
            (unsafe { &*(that as *mut TFn) })(param)
        }
        extern "C" fn dropper<TFn, TParam, TResult>(that: usize)
        where
            TFn: Fn(TParam) -> TResult,
        {
            drop(unsafe { Box::from_raw(that as *mut TFn) });
        }
        Self {
            caller: caller::<TFn, TParam, TResult>,
            remover: dropper::<TFn, TParam, TResult>,
            inner,
        }
    }
}

impl<'a, TParam, TResult> Drop for RBoxFn<TParam, TResult> {
    fn drop(&mut self) {
        (self.remover)(self.inner);
    }
}

impl<'a, TParam, TResult: 'a> RFn<'a, TParam, TResult> for RBoxFn<TParam, TResult> {
    fn call(&self, p: TParam) -> TResult {
        (self.caller)(self.inner, p)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU8, Ordering};

    use super::*;
    #[test]
    fn execute_lambda() {
        let cell = &std::cell::RefCell::new(0);
        let lambda = &move |_| *cell.borrow_mut() += 1;
        let i = RRefFn::from(lambda);
        i.call(());
        assert_eq!(1, cell.take());
    }

    #[test]
    fn return_captured_ref() {
        fn foo<'a>(inner: &'a String) -> RBoxFn<(), &'a str> {
            (|_| inner.as_ref()).into()
        }
        let inner = "Value".to_owned();
        let inner = foo(&inner);
        let a = inner.call(());
        let b = inner.call(());

        assert_eq!(a, b);
        assert_eq!(a, "Value");
    }

    #[test]
    fn move_value() {
        fn return_from_stack(a: String) -> RBoxFn<(), usize> {
            (move |_| a.len()).into()
        }
        let boxed = return_from_stack("foo".to_owned());
        assert_eq!(3, boxed.call(()));
    }

    #[test]
    fn drop_box_fn() {
        static DROP_EVENT: AtomicU8 = AtomicU8::new(0);
        struct Foo(u8);
        impl Foo {
            fn bar(&self) -> u8 {
                self.0
            }
        }
        impl Drop for Foo {
            fn drop(&mut self) {
                DROP_EVENT.store(self.0, Ordering::SeqCst);
            }
        }
        fn return_from_stack(a: Foo) -> RBoxFn<(), usize> {
            let b = "test".to_owned();
            (move |_| {
                println!("Never called, but transferred to closure: {}", a.bar());
                b.len() + 1
            })
            .into()
        }

        let closure = return_from_stack(Foo(42));
        assert_eq!(0, DROP_EVENT.load(Ordering::SeqCst));
        drop(closure);
        assert_eq!(42, DROP_EVENT.load(Ordering::SeqCst));
    }
}
