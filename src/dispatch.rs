pub(crate) use target_impl::Dispatch;

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
        let val = target_impl::detect();

        LAZY.store(to_u8(Some(val)), Relaxed);

        val
    }

    if target_impl::default_feature() == target_impl::WIDEST {
        return target_impl::WIDEST;
    }

    if let Some(val) = unsafe { from_u8(LAZY.load(Relaxed)) } {
        val
    } else {
        init()
    }
}

#[cfg(target_arch = "aarch64")]
mod target_impl {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Dispatch {
        Neon,
        Plain,
    }
    pub(super) const WIDEST: Dispatch = Dispatch::Neon;

    pub(super) fn detect() -> Dispatch {
        if std::arch::is_aarch64_feature_detected!("neon") {
            Dispatch::Neon
        } else {
            Dispatch::Plain
        }
    }
    #[allow(unreachable_code)]
    pub(super) const fn default_feature() -> Dispatch {
        #[cfg(target_feature = "neon")]
        return Dispatch::Neon;
        Dispatch::Plain
    }
}
#[cfg(target_arch = "arm")]
mod target_impl {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Dispatch {
        Neon,
        Plain,
    }
    pub(super) const WIDEST: Dispatch = Dispatch::Neon;

    pub(super) fn detect() -> Dispatch {
        if std::arch::is_arm_feature_detected!("neon") {
            Dispatch::Neon
        } else {
            Dispatch::Plain
        }
    }
    #[allow(unreachable_code)]
    pub(super) const fn default_feature() -> Dispatch {
        #[cfg(target_feature = "neon")]
        return Dispatch::Neon;
        Dispatch::Plain
    }
}
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod target_impl {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Dispatch {
        Avx512,
        Avx2,
        Sse4,
        Plain,
    }
    pub(super) const WIDEST: Dispatch = Dispatch::Avx512;

    pub(super) fn detect() -> Dispatch {
        if std::arch::is_x86_feature_detected!("avx512f")
            && std::arch::is_x86_feature_detected!("avx512bw")
        {
            Dispatch::Avx512
        } else if std::arch::is_x86_feature_detected!("avx2") {
            Dispatch::Avx2
        } else if std::arch::is_x86_feature_detected!("sse4.2") {
            Dispatch::Sse4
        } else {
            Dispatch::Plain
        }
    }
    #[allow(unreachable_code)]
    pub(super) const fn default_feature() -> Dispatch {
        #[cfg(target_feature = "avx512f")]
        return Dispatch::Avx512;
        #[cfg(target_feature = "avx2")]
        return Dispatch::Avx2;
        #[cfg(target_feature = "sse4.2")]
        return Dispatch::Sse4;
        Dispatch::Plain
    }
}
#[cfg(target_arch = "mips")]
mod target_impl {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Dispatch {
        Msa,
        Plain,
    }
    pub(super) const WIDEST: Dispatch = Dispatch::Msa;

    pub(super) fn detect() -> Dispatch {
        if std::arch::is_mips_feature_detected!("msa") {
            Dispatch::Msa
        } else {
            Dispatch::Plain
        }
    }
    #[allow(unreachable_code)]
    pub(super) const fn default_feature() -> Dispatch {
        #[cfg(target_feature = "msa")]
        return Dispatch::Msa;
        Dispatch::Plain
    }
}
#[cfg(target_arch = "mips64")]
mod target_impl {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Dispatch {
        Msa,
        Plain,
    }
    pub(super) const WIDEST: Dispatch = Dispatch::Msa;

    pub(super) fn detect() -> Dispatch {
        if std::arch::is_mips64_feature_detected!("msa") {
            Dispatch::Msa
        } else {
            Dispatch::Plain
        }
    }
    #[allow(unreachable_code)]
    pub(super) const fn default_feature() -> Dispatch {
        #[cfg(target_feature = "msa")]
        return Dispatch::Msa;
        Dispatch::Plain
    }
}
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
mod target_impl {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Dispatch {
        V,
        Plain,
    }
    pub(super) const WIDEST: Dispatch = Dispatch::V;

    pub(super) fn detect() -> Dispatch {
        if std::arch::is_riscv_feature_detected!("v") {
            V
        } else {
            Plain
        }
    }
    #[allow(unreachable_code)]
    pub(super) const fn default_feature() -> Dispatch {
        #[cfg(target_feature = "v")]
        return Dispatch::V;
        Dispatch::Plain
    }
}
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod target_impl {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Dispatch {
        Simd128,
    }
    pub(super) const WIDEST: Dispatch = Dispatch::Simd128;

    pub(super) fn detect() -> Dispatch {
        Dispatch::Simd128
    }
    pub(super) const fn default_feature() -> Dispatch {
        WIDEST
    }
}
#[cfg(all(target_arch = "wasm32", not(target_feature = "simd128")))]
mod target_impl {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Dispatch {
        Plain,
    }
    pub(super) const WIDEST: Dispatch = Dispatch::Plain;

    pub(super) fn detect() -> Dispatch {
        Dispatch::Plain
    }
    pub(super) const fn default_feature() -> Dispatch {
        WIDEST
    }
}
