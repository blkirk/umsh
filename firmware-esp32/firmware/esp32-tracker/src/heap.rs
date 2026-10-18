//! The global allocator.
//!
//! Allocations come from esp-alloc's heap in internal RAM. The radio, the
//! executor, and anything an interrupt or a DMA transfer touches need it
//! there.
//!
//! The communicator is the exception. Its node and interface hold plain
//! data, touched only by the tasks that own them, and that data grows
//! with the chats. Anything allocated while one of those tasks is polled
//! ([`InPsram`]) goes to PSRAM instead. An allocation is freed to the heap
//! its address falls in, so it may be dropped anywhere.

use core::alloc::{GlobalAlloc, Layout};

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

struct Allocator;

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        #[cfg(feature = "communicator")]
        if scope::active() {
            return unsafe { crate::external::alloc(layout) };
        }
        unsafe { GlobalAlloc::alloc(&esp_alloc::HEAP, layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        #[cfg(feature = "communicator")]
        if crate::external::holds(ptr) {
            return unsafe { crate::external::dealloc(ptr, layout) };
        }
        unsafe { GlobalAlloc::dealloc(&esp_alloc::HEAP, ptr, layout) }
    }
}

#[cfg(feature = "communicator")]
pub(crate) use scope::{InPsram, in_psram};

#[cfg(feature = "communicator")]
mod scope {
    use core::future::Future;
    use core::pin::Pin;
    use core::sync::atomic::{AtomicBool, Ordering};
    use core::task::{Context, Poll};

    /// Whether an [`InPsram`] task is being polled.
    static ACTIVE: AtomicBool = AtomicBool::new(false);

    unsafe extern "C" {
        static _stack_end: u8;
        static _stack_start: u8;
    }

    /// A task whose allocations go to PSRAM.
    pub(crate) struct InPsram<F>(pub F);

    impl<F: Future> Future for InPsram<F> {
        type Output = F::Output;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
            // SAFETY: the future is pinned whenever this is, and never
            // moved out.
            let task = unsafe { self.map_unchecked_mut(|scoped| &mut scoped.0) };
            in_psram(|| task.poll(cx))
        }
    }

    /// Runs `f` with what it allocates going to PSRAM.
    pub(crate) fn in_psram<T>(f: impl FnOnce() -> T) -> T {
        let outer = ACTIVE.swap(true, Ordering::Relaxed);
        let out = f();
        ACTIVE.store(outer, Ordering::Relaxed);
        out
    }

    /// Whether an allocation being made now is the scoped task's. The
    /// scope covers the main stack at thread level: the executor's tasks
    /// run there, while the radio's threads have stacks of their own and
    /// an interrupt raises the level.
    pub(super) fn active() -> bool {
        if !ACTIVE.load(Ordering::Relaxed) {
            return false;
        }
        let (sp, ps): (usize, u32);
        unsafe {
            core::arch::asm!(
                "mov {sp}, a1",
                "rsr.ps {ps}",
                sp = out(reg) sp,
                ps = out(reg) ps,
                options(nomem, nostack),
            );
        }
        let main_stack =
            core::ptr::addr_of!(_stack_end) as usize..core::ptr::addr_of!(_stack_start) as usize;
        main_stack.contains(&sp) && ps & 0xf == 0
    }
}
