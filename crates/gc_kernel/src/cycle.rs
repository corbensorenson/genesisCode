use std::cell::UnsafeCell;
use std::fmt;
use std::ops::Deref;

use rust_cc::{Cc, Context, Finalize, Trace};

/// Trace-aware shared ownership with deterministic clone-on-write mutation.
///
/// Collection is explicit and cannot run while `make_mut` holds its unique borrow.
pub struct Shared<T: Trace + 'static>(Cc<CowNode<T>>);

struct CowNode<T> {
    value: UnsafeCell<T>,
}

impl<T: Trace + 'static> Shared<T> {
    pub fn new(value: T) -> Self {
        Self(Cc::new(CowNode {
            value: UnsafeCell::new(value),
        }))
    }

    pub fn ptr_eq(left: &Self, right: &Self) -> bool {
        Cc::ptr_eq(&left.0, &right.0)
    }

    pub fn strong_count(&self) -> u32 {
        self.0.strong_count()
    }

    pub fn make_mut(&mut self) -> &mut T
    where
        T: Clone,
    {
        if self.0.strong_count() != 1 || self.0.weak_count() != 0 {
            self.0 = Cc::new(CowNode {
                value: UnsafeCell::new((**self).clone()),
            });
        }
        // The node is uniquely strongly and weakly owned, and collection is explicit.
        unsafe { &mut *self.0.value.get() }
    }

    #[cfg(test)]
    pub(crate) fn weak_alive_probe(&self) -> Box<dyn Fn() -> bool> {
        let weak = self.0.downgrade();
        Box::new(move || weak.upgrade().is_some())
    }

    #[cfg(test)]
    pub(crate) fn as_ptr(&self) -> *const T {
        self.deref()
    }
}

impl<T: Trace + 'static> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Trace + 'static> Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Shared access is immutable; mutation requires unique ownership through `make_mut`.
        unsafe { &*self.0.value.get() }
    }
}

impl<T: Trace + 'static> AsRef<T> for Shared<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T: Trace + fmt::Debug + 'static> fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

impl<T: Trace + 'static> Finalize for Shared<T> {}

unsafe impl<T: Trace + 'static> Trace for Shared<T> {
    fn trace(&self, ctx: &mut Context<'_>) {
        self.0.trace(ctx);
    }
}

impl<T: Trace> Finalize for CowNode<T> {}

unsafe impl<T: Trace> Trace for CowNode<T> {
    fn trace(&self, ctx: &mut Context<'_>) {
        // Collection only runs at safe points where no unique mutation is active.
        unsafe { (&*self.value.get()).trace(ctx) };
    }
}

pub(crate) fn collect_cycles() {
    rust_cc::collect_cycles();
}
