# FFI-Safe lambdas
Provides structures with `#[repr(C)]`. Furthermore, custom Traits `RFn`, `RFnMut` and `RFnOnce` are implemented for both the FFI-structs and their std-equivalent. 
Multiple or no parameters must be achieved by using tuples.

abi_stable support can be enabled via feature-flag to implement `StableAbi`.