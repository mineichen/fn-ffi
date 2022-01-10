use std::marker::PhantomData;

pub trait RFnMut<TParam, TResult> {
    fn call(&mut self, p: TParam) -> TResult;
}

impl<TFn: FnMut(TParam) -> TResult, TParam, TResult> RFnMut<TParam, TResult> for TFn {
    fn call(&mut self, p: TParam) -> TResult {
        (self)(p)
    }
}

/// FFI-Safe lambda without allocations
#[repr(C)]
#[cfg_attr(feature = "abi_stable", derive(abi_stable::StableAbi))]
pub struct RRefFnMut<'a, TParam, TResult> {
    ptr: extern "C" fn(usize, TParam) -> TResult,
    inner: usize,
    p: PhantomData<&'a ()>,
}

impl<'a, T, TParam, TResult> From<&'a mut T> for RRefFnMut<'a, TParam, TResult>
where
    T: 'a + FnMut(TParam) -> TResult,
{
    fn from(inner: &mut T) -> Self {
        let inner: usize = unsafe { std::mem::transmute(inner) };

        extern "C" fn ptr<T, TParam, TResult>(inner: usize, p: TParam) -> TResult
        where
            T: FnMut(TParam) -> TResult,
        {
            let function: &mut T = unsafe { std::mem::transmute(inner) };
            (function)(p)
        }
        Self {
            ptr: ptr::<T, TParam, TResult>,
            inner,
            p: PhantomData,
        }
    }
}
impl<'a, TParam, TResult> RFnMut<TParam, TResult> for RRefFnMut<'a, TParam, TResult> {
    fn call(&mut self, p: TParam) -> TResult {
        (self.ptr)(self.inner, p)
    }
}

/// FFI-Safe lambda which requires 1 heap allocation for boxing
#[repr(C)]
#[cfg_attr(feature = "abi_stable", derive(abi_stable::StableAbi))]
pub struct RBoxFnMut<TParam, TResult> {
    caller: extern "C" fn(usize, TParam) -> TResult,
    remover: extern "C" fn(usize),
    inner: usize,
}


impl<TFn, TParam, TResult> From<TFn> for RBoxFnMut<TParam, TResult>
where
    TFn: FnMut(TParam) -> TResult,
{
    fn from(inner: TFn) -> Self {
        let box_inner = Box::new(inner);
        let inner = Box::into_raw(box_inner);
        let inner = inner as usize;

        extern "C" fn caller<TFn, TParam, TResult>(that: usize, param: TParam) -> TResult
        where
            TFn: FnMut(TParam) -> TResult,
        {
            (unsafe { &mut *(that as *mut TFn) })(param)
        }
        extern "C" fn dropper<TFn, TParam, TResult>(that: usize)
        where
            TFn: FnMut(TParam) -> TResult,
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

impl<'a, TParam, TResult> Drop for RBoxFnMut<TParam, TResult> {
    fn drop(&mut self) {
        (self.remover)(self.inner);
    }
}

impl<TParam, TResult> RFnMut<TParam, TResult> for RBoxFnMut<TParam, TResult> {
    fn call(&mut self, p: TParam) -> TResult {
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
        let lambda = &mut move |_| *cell.borrow_mut() += 1;
        let mut i = RRefFnMut::from(lambda);
        i.call(());
        assert_eq!(1, cell.take());
    }

    #[test]
    fn move_value() {
        fn return_from_stack(a: String) -> RBoxFnMut<(), usize> {
            (move |_| a.len()).into()
        }
        let mut boxed = return_from_stack("foo".to_owned());
        assert_eq!(3, boxed.call(()));
    }

    #[test]
    fn transform_borrowed() {
        fn create_borrowing<'a>(s: &mut String) -> RBoxFnMut<&'static str, ()> {
            (move |ex: &'static str| {
                s.extend(ex.chars());
            }).into()
        }
        let mut inner = "Hello".to_owned();
        let mut boxed = create_borrowing(&mut inner);
        boxed.call("!");
        boxed.call("!!");
        assert_eq!("Hello!!!", inner);
    }

    #[test]
    fn transform_moved() {
        fn return_from_stack(mut str: String) -> RBoxFnMut<&'static str, String> {
            (move |input: &'static str| {
                str.extend(input.chars());
                str.clone()
            })
            .into()
        }
        let mut boxed = return_from_stack("Hello".to_owned());
        boxed.call("!");
        assert_eq!("Hello!!!", boxed.call("!!"));
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
        fn return_from_stack(a: Foo) -> RBoxFnMut<(), usize> {
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
