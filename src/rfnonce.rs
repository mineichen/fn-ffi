pub trait RFnOnce<TParam, TResult> {
    fn call(self, p: TParam) -> TResult;
}

impl<TFn: FnOnce(TParam) -> TResult, TParam, TResult> RFnOnce<TParam, TResult> for TFn {
    fn call(self, p: TParam) -> TResult {
        (self)(p)
    }
}

/// FFI-Safe lambda which requires 1 heap allocation for boxing
#[repr(C)]
#[cfg_attr(feature = "abi_stable", derive(abi_stable::StableAbi))]
pub struct RBoxFnOnce<TParam, TResult> {
    caller: extern "C" fn(usize, TParam) -> TResult,
    remover: extern "C" fn(usize),
    inner: usize,
}

impl<T, TParam, TResult> From<T> for RBoxFnOnce<TParam, TResult>
where
    T: FnOnce(TParam) -> TResult,
{
    fn from(inner: T) -> Self {
        let box_inner = Box::new(inner);
        let inner: usize = Box::into_raw(box_inner) as usize;

        extern "C" fn caller<T, TParam, TResult>(that: usize, param: TParam) -> TResult
        where
            T: FnOnce(TParam) -> TResult,
        {
            let function = unsafe { Box::from_raw(that as *mut T) };
            (function)(param)
        }
        extern "C" fn dropper<T, TParam, TResult>(that: usize)
        where
            T: FnOnce(TParam) -> TResult,
        {
            drop(unsafe { Box::from_raw(that as *mut T) });
        }
        Self {
            caller: caller::<T, TParam, TResult>,
            remover: dropper::<T, TParam, TResult>,
            inner,
        }
    }
}

impl<TParam, TResult> Drop for RBoxFnOnce<TParam, TResult> {
    fn drop(&mut self) {
        if self.inner != 0 {
            (self.remover)(self.inner);
        }
    }
}
impl<TParam, TResult> RFnOnce<TParam, TResult> for RBoxFnOnce<TParam, TResult> {
    fn call(mut self, p: TParam) -> TResult {
        let inner = self.inner;
        self.inner = 0;
        (self.caller)(inner, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};

    fn api(f: impl Into<RBoxFnOnce<usize, usize>>) {
        assert_eq!(5, f.into().call(1));
    }

    #[test]
    fn test() {
        let ctx = "Test".to_string();
        api(move |x| {
            let a = ctx;
            x + a.len()
        })
    }

    #[test]
    fn execute_box_fn_once() {
        fn return_from_stack() -> RBoxFnOnce<&'static str, String> {
            let mut i = "Test".to_string();
            RBoxFnOnce::from(move |suffix: &'static str| {
                i.extend(suffix.chars());
                i
            })
        }

        assert_eq!("Test!!".to_string(), (return_from_stack()).call("!!"));
    }

    #[test]
    fn drop_box_fn_once() {
        static DROP_EVENT: AtomicU8 = AtomicU8::new(0);
        struct Foo(u8);
        impl Drop for Foo {
            fn drop(&mut self) {
                DROP_EVENT.store(self.0, Ordering::SeqCst);
            }
        }
        let foo = Foo(42);
        drop(RBoxFnOnce::from(move |_: ()| {
            let inner = foo;
            panic!("Never called, but transferred to closure: {}", inner.0)
        }));
        assert_eq!(42, DROP_EVENT.load(Ordering::SeqCst));
    }
}
