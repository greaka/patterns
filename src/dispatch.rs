pub(crate) fn get_or_init() -> Dispatch {
    use core::sync::atomic::{AtomicU8, Ordering::Relaxed};
    static LAZY: AtomicU8 = AtomicU8::new(to_u8(None));

    const fn to_u8(val: Option<Dispatch>) -> u8 {
        unsafe { core::mem::transmute(val) }
    }

    /// # Safety
    /// the value must have been created using [`to_u8`]
    const unsafe fn from_u8(val: u8) -> Option<Dispatch> {
        unsafe { core::mem::transmute(val) }
    }

    #[cold]
    fn init() -> Dispatch {
        let val = {
            #[cfg(target_arch = "aarch64")]
            {
                if std::arch::is_aarch64_feature_detected!("neon") {
                    Dispatch::Neon
                } else {
                    Dispatch::Plain
                }
            }
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                if std::arch::is_x86_feature_detected!("avx2") {
                    Dispatch::Avx2
                } else if std::arch::is_x86_feature_detected!("sse4.2") {
                    Dispatch::SSE4
                } else {
                    Dispatch::Plain
                }
            }
            #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
            {
                Dispatch::Simd128
            }
            #[cfg(all(target_arch = "wasm32", not(target_feature = "simd128")))]
            {
                Dispatch::Plain
            }
        };

        LAZY.store(to_u8(Some(val)), Relaxed);

        val
    }

    if let Some(val) = unsafe { from_u8(LAZY.load(Relaxed)) } {
        val
    } else {
        init()
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Dispatch {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    Avx2,
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    SSE4,
    #[cfg(target_arch = "aarch64")]
    Neon,
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    Simd128,
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    Plain,
}
